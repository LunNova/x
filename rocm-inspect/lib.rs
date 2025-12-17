// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! ROCm/HIP binary inspection library.
//!
//! Provides utilities for parsing and analyzing AMDGPU code objects,
//! Clang offload bundles, and HIP fat binaries.

pub mod isa;

use goblin::elf::{Elf, header::EM_AMDGPU};
use std::fs;
use std::path::Path;

pub use isa::{format_features, gfx_target_from_elf_flags};

pub const OFFLOAD_BUNDLE_MAGIC: &[u8] = b"__CLANG_OFFLOAD_BUNDLE__";
pub const COMPRESSED_BUNDLE_MAGIC: &[u8] = b"CCOB";
pub const ELF_MAGIC: &[u8] = b"\x7fELF";
const EM_X86_64: u16 = 62;

#[derive(Debug, Clone)]
pub struct CodeObject {
	pub bundle_entry_id: Option<String>,
	pub isa: String,
	pub features: String,
	pub size: u64,
	pub source_file: String,
	pub kernel_names: Vec<String>,
}

pub fn analyze_file(path: &Path) -> Result<Vec<CodeObject>, Box<dyn std::error::Error>> {
	let data = fs::read(path)?;
	analyze_data(&data)
}

pub fn analyze_data(data: &[u8]) -> Result<Vec<CodeObject>, Box<dyn std::error::Error>> {
	if data.starts_with(OFFLOAD_BUNDLE_MAGIC) {
		parse_bundle(data)
	} else if data.starts_with(COMPRESSED_BUNDLE_MAGIC) {
		let uncompressed = decompress_bundle(data)?;
		parse_bundle(&uncompressed)
	} else if data.starts_with(ELF_MAGIC) {
		let elf = Elf::parse(data)?;
		if elf.header.e_machine == EM_AMDGPU {
			let obj = extract_code_object_info(data, None)?;
			Ok(vec![obj])
		} else if elf.header.e_machine == EM_X86_64 {
			search_embedded_bundles(data, &elf)
		} else {
			Err(format!("Unsupported ELF machine type: {}", elf.header.e_machine).into())
		}
	} else {
		Err("Unknown file format".into())
	}
}

pub fn decompress_bundle(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
	use flate2::read::ZlibDecoder;
	use std::io::Read;

	if data.len() < 24 {
		return Err("Compressed bundle header too short".into());
	}

	let method = u16::from_le_bytes(data[6..8].try_into()?);
	let total_size = u32::from_le_bytes(data[8..12].try_into()?) as usize;
	let uncompressed_size = u32::from_le_bytes(data[12..16].try_into()?) as usize;

	// total_size includes the 24-byte header; saturating_sub + min defend against malformed sizes
	let compressed_data_size = total_size.saturating_sub(24);
	let compressed_data = &data[24..24 + compressed_data_size.min(data.len() - 24)];

	let uncompressed = match method {
		0 => {
			// zlib compression (RFC 1950)
			let mut decoder = ZlibDecoder::new(compressed_data);
			let mut buf = Vec::new();
			decoder.read_to_end(&mut buf)?;
			buf
		}
		1 => {
			// zstd compression
			zstd::decode_all(compressed_data)?
		}
		_ => {
			return Err(format!("Unsupported compression method: {method}").into());
		}
	};

	if uncompressed.len() != uncompressed_size {
		return Err(format!(
			"Decompressed size mismatch: expected {}, got {}",
			uncompressed_size,
			uncompressed.len()
		)
		.into());
	}

	Ok(uncompressed)
}

pub fn parse_bundle(data: &[u8]) -> Result<Vec<CodeObject>, Box<dyn std::error::Error>> {
	if !data.starts_with(OFFLOAD_BUNDLE_MAGIC) {
		return Err("Invalid bundle magic".into());
	}

	if data.len() < 32 {
		return Err("Bundle header too short".into());
	}

	let num_objects = u64::from_le_bytes(data[24..32].try_into()?);
	let mut objects = Vec::new();
	let mut offset = 32;

	for _ in 0..num_objects {
		if offset + 24 > data.len() {
			return Err("Truncated bundle descriptor".into());
		}

		let co_offset = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
		let co_size = u64::from_le_bytes(data[offset + 8..offset + 16].try_into()?);
		let id_size = u64::from_le_bytes(data[offset + 16..offset + 24].try_into()?);

		let id_start = offset + 24;
		let id_end = id_start + id_size as usize;

		if id_end > data.len() {
			return Err("Bundle entry ID extends beyond file".into());
		}

		let bundle_entry_id = String::from_utf8(data[id_start..id_end].to_vec())?;

		// host- entries are x86 code, not GPU code; zero-size entries are placeholders
		if bundle_entry_id.starts_with("host-") || co_size == 0 {
			offset = id_end;
			continue;
		}

		let co_start = co_offset as usize;
		let co_end = co_start + co_size as usize;

		if co_end > data.len() {
			return Err(format!(
				"Code object extends beyond bundle: need offset {}..{}, bundle size {}",
				co_start,
				co_end,
				data.len()
			)
			.into());
		}

		let elf_data = &data[co_start..co_end];
		let mut obj = extract_code_object_info(elf_data, Some(bundle_entry_id.clone()))?;
		obj.size = co_size;

		objects.push(obj);
		offset = id_end;
	}

	Ok(objects)
}

pub fn search_embedded_bundles(data: &[u8], elf: &Elf) -> Result<Vec<CodeObject>, Box<dyn std::error::Error>> {
	let mut all_objects = Vec::new();

	for section in &elf.section_headers {
		if let Some(name) = elf.shdr_strtab.get_at(section.sh_name) {
			// Search sections that commonly contain embedded bundles
			if name.contains("fatbin") || name.contains("hip") || name == ".rodata" || name == ".data" {
				let start = section.sh_offset as usize;
				let end = start + section.sh_size as usize;

				if end > data.len() {
					continue;
				}

				let section_data = &data[start..end];

				let bundle_positions = find_all_bundle_positions(section_data);
				for bundle_offset in bundle_positions {
					let bundle_data = &section_data[bundle_offset..];

					match analyze_bundle_or_elf(bundle_data) {
						Ok(mut objects) => {
							all_objects.append(&mut objects);
						}
						Err(e) => {
							eprintln!(
								"Warning: Failed to parse bundle at offset 0x{:x} in {}: {}",
								start + bundle_offset,
								name,
								e
							);
							continue;
						}
					}
				}
			}
		}
	}

	if all_objects.is_empty() {
		Err("No embedded bundles found in host binary".into())
	} else {
		Ok(all_objects)
	}
}

pub fn find_all_bundle_positions(data: &[u8]) -> Vec<usize> {
	let mut positions = Vec::new();

	for i in 0..data.len() {
		// Check for compressed bundle magic (4 bytes)
		if i + COMPRESSED_BUNDLE_MAGIC.len() <= data.len() && &data[i..i + COMPRESSED_BUNDLE_MAGIC.len()] == COMPRESSED_BUNDLE_MAGIC {
			positions.push(i);
			continue;
		}

		// Check for uncompressed bundle magic (24 bytes)
		if i + OFFLOAD_BUNDLE_MAGIC.len() <= data.len() && &data[i..i + OFFLOAD_BUNDLE_MAGIC.len()] == OFFLOAD_BUNDLE_MAGIC {
			positions.push(i);
		}
	}

	positions
}

pub fn analyze_bundle_or_elf(data: &[u8]) -> Result<Vec<CodeObject>, Box<dyn std::error::Error>> {
	if data.starts_with(OFFLOAD_BUNDLE_MAGIC) {
		parse_bundle(data)
	} else if data.starts_with(COMPRESSED_BUNDLE_MAGIC) {
		// For compressed bundles, we need to limit to the total_size field
		if data.len() < 12 {
			return Err("Compressed bundle too short to read total_size".into());
		}
		let total_size = u32::from_le_bytes(data[8..12].try_into()?) as usize;

		if data.len() < total_size {
			return Err(format!("Compressed bundle truncated: need {} bytes, have {}", total_size, data.len()).into());
		}

		let bundle_data = &data[..total_size];
		let uncompressed = decompress_bundle(bundle_data)?;
		parse_bundle(&uncompressed)
	} else if data.starts_with(ELF_MAGIC) {
		let obj = extract_code_object_info(data, None)?;
		Ok(vec![obj])
	} else {
		Err("Not a recognized bundle or ELF".into())
	}
}

pub fn extract_code_object_info(elf_data: &[u8], bundle_entry_id: Option<String>) -> Result<CodeObject, Box<dyn std::error::Error>> {
	let elf = Elf::parse(elf_data)?;

	if elf.header.e_machine != EM_AMDGPU {
		return Err("Not an AMDGPU code object".into());
	}

	let e_flags = elf.header.e_flags;
	let isa = gfx_target_from_elf_flags(e_flags);
	let features = format_features(e_flags, elf.header.e_ident[8]);

	// .kd symbols are kernel descriptors - these reliably mark GPU kernels
	let mut kernel_names = Vec::new();
	for sym in &elf.syms {
		if let Some(name) = elf.strtab.get_at(sym.st_name) {
			if let Some(kernel_name) = name.strip_suffix(".kd") {
				let demangled = cpp_demangle::Symbol::new(kernel_name)
					.ok()
					.and_then(|sym| sym.demangle().ok())
					.unwrap_or_else(|| kernel_name.to_string());
				kernel_names.push(demangled);
			}
		}
	}

	Ok(CodeObject {
		bundle_entry_id,
		isa: isa.to_string(),
		features,
		size: elf_data.len() as u64,
		source_file: String::new(),
		kernel_names,
	})
}

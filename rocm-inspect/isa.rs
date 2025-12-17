// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! AMDGPU ISA and feature flag definitions.
//!
//! FIXME FIXME FIXME: This should be auto-generated from LLVM sources!
//! The ISA table and ABI version handling here is probably incomplete or wrong.
//! See: llvm-project/llvm/include/llvm/TargetParser/TargetParser.h
//! See: llvm-project/llvm/lib/TargetParser/TargetParser.cpp

/// Maps ELF e_flags to gfx target name.
///
/// The architecture ID is stored in the low 8 bits of e_flags.
pub fn gfx_target_from_elf_flags(e_flags: u32) -> &'static str {
	let arch_id = (e_flags & 0xFF) as u8;
	match arch_id {
		// GCN family
		0x020 => "gfx600",
		0x021 => "gfx601",
		0x022 => "gfx700",
		0x023 => "gfx701",
		0x024 => "gfx702",
		0x025 => "gfx703",
		0x026 => "gfx704",
		0x028 => "gfx801",
		0x029 => "gfx802",
		0x02a => "gfx803",
		0x02b => "gfx810",
		0x02c => "gfx900",
		0x02d => "gfx902",
		0x02e => "gfx904",
		0x02f => "gfx906",
		0x030 => "gfx908",
		0x031 => "gfx909",
		0x032 => "gfx90c",
		0x03a => "gfx602",
		0x03b => "gfx705",
		0x03c => "gfx805",
		0x03f => "gfx90a",
		0x040 => "gfx940",
		0x04b => "gfx941",
		0x04c => "gfx942",
		0x051 => "gfx9-generic",
		0x05f => "gfx9_4-generic",
		// RDNA 1
		0x033 => "gfx1010",
		0x034 => "gfx1011",
		0x035 => "gfx1012",
		0x042 => "gfx1013",
		0x052 => "gfx10_1-generic",
		// RDNA 2
		0x036 => "gfx1030",
		0x037 => "gfx1031",
		0x038 => "gfx1032",
		0x039 => "gfx1033",
		0x03d => "gfx1035",
		0x03e => "gfx1034",
		0x045 => "gfx1036",
		0x053 => "gfx10_3-generic",
		// RDNA 3
		0x041 => "gfx1100",
		0x044 => "gfx1103",
		0x046 => "gfx1101",
		0x047 => "gfx1102",
		0x043 => "gfx1150",
		0x04a => "gfx1151",
		0x055 => "gfx1152",
		0x054 => "gfx11-generic",
		// RDNA 4
		0x048 => "gfx1200",
		0x04e => "gfx1201",
		0x059 => "gfx12-generic",
		_ => "unknown",
	}
}

/// Decodes feature flags from ELF e_flags and ABI version.
///
/// The ABI version is stored in e_ident[EI_ABIVERSION] (index 8).
/// Feature encoding varies by ABI version.
pub fn format_features(e_flags: u32, abi_version: u8) -> String {
	let mut features = Vec::new();

	match abi_version {
		0 => {
			// ABI V2: simple flag at bit 0
			if (e_flags & 0x01) != 0 {
				features.push("xnack+".to_string());
			}
		}
		1 => {
			// ABI V3: boolean flags at 0x100 and 0x200
			if (e_flags & 0x100) != 0 {
				features.push("xnack+".to_string());
			}
			if (e_flags & 0x200) != 0 {
				features.push("sramecc+".to_string());
			}
		}
		_ => {
			// ABI V4+: 2-bit fields with 4 states each
			let xnack = match e_flags & 0x300 {
				0x000 => None,           // unsupported - don't display
				0x100 => Some("xnack"),  // any
				0x200 => Some("xnack-"), // off
				0x300 => Some("xnack+"), // on
				_ => None,
			};
			if let Some(xnack_str) = xnack {
				features.push(xnack_str.to_string());
			}

			let sramecc = match e_flags & 0xc00 {
				0x000 => None,             // unsupported - don't display
				0x400 => Some("sramecc"),  // any
				0x800 => Some("sramecc-"), // off
				0xc00 => Some("sramecc+"), // on
				_ => None,
			};
			if let Some(sramecc_str) = sramecc {
				features.push(sramecc_str.to_string());
			}
		}
	}

	if features.is_empty() { "-".to_string() } else { features.join(",") }
}

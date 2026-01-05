// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use crate::crate_roots;
use anyhow::Result;
use ra_ap_syntax::ast::{HasModuleItem, HasName};
use ra_ap_syntax::{AstNode, Edition, SourceFile, ast};
use std::path::{Path, PathBuf};

/// Cargo context for a source file - avoids repeated Cargo.toml lookups.
struct CargoContext {
	cargo_dir: Option<PathBuf>,
	crate_roots: Option<std::collections::HashSet<PathBuf>>,
}

/// Result of extracting large inline modules from a source file.
pub struct ExtractionResult {
	/// The modified source with large inline modules replaced by declarations
	pub modified_source: String,
	/// Files to write: (path, content) pairs
	pub extracted_files: Vec<(PathBuf, String)>,
	/// Warnings generated during extraction (e.g., no Cargo.toml found)
	pub warnings: Vec<String>,
}

struct ModuleExtraction {
	mod_start: usize,
	mod_end: usize,
	replacement: String,
	output_path: PathBuf,
	body_content: String,
}

/// Remove common leading whitespace from all lines.
fn dedent(s: &str) -> String {
	let lines: Vec<&str> = s.lines().collect();
	let min_indent = lines
		.iter()
		.filter(|line| !line.trim().is_empty())
		.map(|line| line.len() - line.trim_start().len())
		.min()
		.unwrap_or(0);

	lines
		.iter()
		.map(|line| {
			if line.len() >= min_indent {
				&line[min_indent..]
			} else {
				line.trim_start()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		.trim()
		.to_string()
		+ "\n"
}

/// Determine the file path for an extracted module using Cargo-aware logic.
/// Returns (path, `optional_warning`).
fn determine_module_path(source_path: &Path, mod_name: &str, ctx: &CargoContext) -> (PathBuf, Option<String>) {
	let source_dir = source_path.parent().unwrap_or(Path::new("."));
	let (can_sibling, warning) = ctx.can_have_sibling_modules(source_path);
	let force_mod_rs = ctx.use_mod_rs_form(source_path);

	let base_path = if can_sibling {
		if force_mod_rs {
			// tests/examples/benches: use mod_name/mod.rs to avoid new binary
			source_dir.join(mod_name).join("mod.rs")
		} else {
			// Normal crate root or mod.rs: sibling file
			source_dir.join(format!("{mod_name}.rs"))
		}
	} else {
		// Non-root: subdirectory named after source file stem
		// src/foo.rs → src/foo/bar.rs
		let stem = source_path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
		source_dir.join(stem).join(format!("{mod_name}.rs"))
	};

	// If the target file already exists, use mod.rs form to avoid overwriting
	let final_path = if base_path.extension().is_some_and(|ext| ext == "rs") && !base_path.ends_with("mod.rs") && base_path.exists() {
		let mod_name_from_path = base_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
		let parent = base_path.parent().unwrap_or(Path::new("."));
		parent.join(mod_name_from_path).join("mod.rs")
	} else {
		base_path
	};

	(final_path, warning)
}

/// Extract inline modules that exceed the line threshold into separate files.
pub fn extract_large_modules(source: &str, source_path: &Path, threshold: usize) -> Result<ExtractionResult> {
	// Rust scripts (shebang) can't have external modules
	if source.starts_with("#!") {
		return Ok(ExtractionResult {
			modified_source: source.to_string(),
			extracted_files: vec![],
			warnings: vec![],
		});
	}

	let parse = SourceFile::parse(source, Edition::Edition2024);
	let file = parse.tree();

	if !parse.errors().is_empty() {
		anyhow::bail!(
			"File has parse errors, skipping extraction:\n{}",
			parse.errors().iter().map(|e| format!("  {e}")).collect::<Vec<_>>().join("\n")
		);
	}

	let ctx = CargoContext::new(source_path);
	let mut warnings = Vec::new();

	let mut extractions: Vec<ModuleExtraction> = Vec::new();

	for item in file.items() {
		if let ast::Item::Module(m) = item {
			if let Some(item_list) = m.item_list() {
				let body_text = item_list.syntax().to_string();
				let line_count = body_text.lines().count();

				if line_count > threshold {
					let mod_name = m.name().expect("module with item_list has name").to_string();

					let (output_path, warning) = determine_module_path(source_path, &mod_name, &ctx);
					if let Some(w) = warning {
						if !warnings.contains(&w) {
							warnings.push(w);
						}
					}

					// Skip extraction if output would cross into Cargo special directory
					if ctx.crosses_into_special_dir(source_path, &output_path) {
						let w = format!(
							"Skipping extraction of `mod {mod_name}`: would create {} in Cargo special directory",
							output_path.display()
						);
						if !warnings.contains(&w) {
							warnings.push(w);
						}
						continue;
					}

					let inner = body_text
						.trim()
						.strip_prefix('{')
						.and_then(|s| s.strip_suffix('}'))
						.expect("item_list body is { ... }");
					let body_content = dedent(inner);

					let full_text = m.syntax().to_string();
					let brace_pos = full_text.find('{').expect("module with item_list has brace");
					let replacement = format!("{};", full_text[..brace_pos].trim_end());
					extractions.push(ModuleExtraction {
						mod_start: m.syntax().text_range().start().into(),
						mod_end: m.syntax().text_range().end().into(),
						replacement,
						output_path,
						body_content,
					});
				}
			}
		}
	}

	if extractions.is_empty() {
		return Ok(ExtractionResult {
			modified_source: source.to_string(),
			extracted_files: vec![],
			warnings,
		});
	}

	// Sort by position descending so we can replace from end to start
	extractions.sort_by(|a, b| b.mod_start.cmp(&a.mod_start));

	let mut modified_source = source.to_string();
	let mut extracted_files = Vec::new();

	for extraction in extractions {
		modified_source.replace_range(extraction.mod_start..extraction.mod_end, &extraction.replacement);
		extracted_files.push((extraction.output_path, extraction.body_content));
	}

	Ok(ExtractionResult {
		modified_source,
		extracted_files,
		warnings,
	})
}

impl CargoContext {
	fn new(source_path: &Path) -> Self {
		let cargo_toml = crate_roots::find_cargo_toml(source_path);
		let (cargo_dir, crate_roots) = match cargo_toml {
			Some(ref toml) => {
				let dir = toml.parent().map(Path::to_path_buf);
				let roots = crate_roots::parse_crate_roots(toml).ok();
				(dir, roots)
			}
			None => (None, None),
		};
		Self { cargo_dir, crate_roots }
	}

	fn can_have_sibling_modules(&self, source_path: &Path) -> (bool, Option<String>) {
		let file_name = source_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
		if file_name == "mod.rs" {
			return (true, None);
		}

		let is_root_name = file_name == "lib.rs" || file_name == "main.rs";
		let fallback = |msg: &str| {
			if is_root_name {
				(true, Some(msg.to_string()))
			} else {
				(false, None)
			}
		};

		let Some(ref roots) = self.crate_roots else {
			return fallback("No Cargo.toml found, using filename heuristics for module placement");
		};

		(roots.contains(source_path), None)
	}

	fn use_mod_rs_form(&self, source_path: &Path) -> bool {
		let Some(ref cargo_dir) = self.cargo_dir else { return false };
		let source_parent = source_path.parent().expect("source path has parent");
		["tests", "examples", "benches"]
			.iter()
			.any(|subdir| cargo_dir.join(subdir) == source_parent)
	}

	/// Check if extraction would create a file in a Cargo special directory from outside it.
	/// Extracting from tests/foo.rs to tests/bar/mod.rs is fine (both in tests/).
	/// Extracting from src/lib.rs to tests/foo.rs is not (crosses into tests/).
	fn crosses_into_special_dir(&self, source_path: &Path, output_path: &Path) -> bool {
		let Some(ref cargo_dir) = self.cargo_dir else { return false };
		let source_parent = source_path.parent().expect("source path has parent");
		let output_parent = output_path.parent().expect("output path has parent");

		["tests", "examples", "benches"].iter().any(|subdir| {
			let special_dir = cargo_dir.join(subdir);
			if !special_dir.is_dir() {
				return false;
			}
			let source_in = source_parent.starts_with(&special_dir) || source_parent == special_dir;
			let output_in = output_parent.starts_with(&special_dir) || output_parent == special_dir;
			// Problem: output lands in special dir but source isn't in it
			output_in && !source_in
		})
	}
}

// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Collect roots from a Cargo.toml array section and its default directory.
fn collect_target_roots(roots: &mut HashSet<PathBuf>, manifest: &toml::Value, cargo_dir: &Path, section: &str, default_dir: &str) {
	if let Some(items) = manifest.get(section).and_then(|v| v.as_array()) {
		for item in items {
			if let Some(path) = item.get("path").and_then(|v| v.as_str()) {
				insert_if_exists(roots, &cargo_dir.join(path));
			}
		}
	}

	let dir = cargo_dir.join(default_dir);
	for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
		let path = entry.path();
		if path.extension().is_some_and(|ext| ext == "rs") {
			insert_if_exists(roots, &path);
		}
	}
}

/// Find the nearest Cargo.toml by walking up from the source file's directory.
/// Stops if a directory has no .rs files (we've left the Rust project).
/// Expects source_path to already be canonical.
#[must_use]
pub fn find_cargo_toml(source_path: &Path) -> Option<PathBuf> {
	let mut current = source_path.parent()?;

	loop {
		let cargo_toml = current.join("Cargo.toml");
		if cargo_toml.exists() {
			return Some(cargo_toml);
		}

		// Check if there are any .rs files in this directory
		// If not, we've likely left the Rust project
		let has_rs_files = std::fs::read_dir(current).ok().is_some_and(|entries| {
			entries
				.filter_map(std::result::Result::ok)
				.any(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
		});

		if !has_rs_files {
			return None;
		}

		current = current.parent()?;
	}
}

fn insert_if_exists(roots: &mut HashSet<PathBuf>, path: &Path) {
	if path.exists() {
		roots.insert(path.to_path_buf());
	}
}

/// Parse crate roots from a Cargo.toml file.
/// Returns absolute paths to all crate root files.
pub fn parse_crate_roots(cargo_toml: &Path) -> anyhow::Result<HashSet<PathBuf>> {
	let cargo_dir = cargo_toml.parent().unwrap_or(Path::new("."));
	let content = std::fs::read_to_string(cargo_toml)?;
	let manifest: toml::Value = content.parse()?;

	let mut roots = HashSet::new();

	// lib: single target with default src/lib.rs
	if let Some(lib) = manifest.get("lib") {
		if let Some(path) = lib.get("path").and_then(|v| v.as_str()) {
			insert_if_exists(&mut roots, &cargo_dir.join(path));
		} else {
			insert_if_exists(&mut roots, &cargo_dir.join("src").join("lib.rs"));
		}
	} else {
		insert_if_exists(&mut roots, &cargo_dir.join("src").join("lib.rs"));
	}

	// bin: array with name-based default paths
	if let Some(bins) = manifest.get("bin").and_then(|v| v.as_array()) {
		for bin in bins {
			if let Some(path) = bin.get("path").and_then(|v| v.as_str()) {
				insert_if_exists(&mut roots, &cargo_dir.join(path));
			} else if let Some(name) = bin.get("name").and_then(|v| v.as_str()) {
				insert_if_exists(&mut roots, &cargo_dir.join("src").join("bin").join(format!("{name}.rs")));
				insert_if_exists(&mut roots, &cargo_dir.join("src").join("bin").join(name).join("main.rs"));
			}
		}
	}
	insert_if_exists(&mut roots, &cargo_dir.join("src").join("main.rs"));

	// test/example/bench: array targets + directory autodiscovery
	for (section, dir) in [("test", "tests"), ("example", "examples"), ("bench", "benches")] {
		collect_target_roots(&mut roots, &manifest, cargo_dir, section, dir);
	}

	Ok(roots)
}

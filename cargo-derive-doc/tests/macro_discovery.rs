// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Discover proc-macros from cargo metadata + dylib loading.
//!
//! Strategy:
//! 1. Run `cargo build --message-format=json` to get proc-macro dylib paths
//! 2. Load each dylib to get exported macro names
//! 3. Build a map: macro_name -> (crate_name, dylib_path, ProcMacro)

use ra_ap_paths::AbsPathBuf;
use ra_ap_proc_macro_api::{MacroDylib, ProcMacroClient, ProcMacroKind};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

fn find_proc_macro_srv() -> Option<AbsPathBuf> {
	let output = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
	if !output.status.success() {
		return None;
	}
	let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();

	for subdir in &["libexec", "lib"] {
		let path = format!("{}/{}/rust-analyzer-proc-macro-srv", sysroot, subdir);
		if Path::new(&path).exists() {
			return Some(AbsPathBuf::assert(path.into()));
		}
	}
	None
}

#[derive(Debug, Clone)]
struct DiscoveredMacro {
	name: String,
	kind: ProcMacroKind,
	crate_name: String,
	dylib_path: AbsPathBuf,
}

/// Get all proc-macro dylibs from cargo build output
fn get_proc_macro_dylibs(include_tests: bool) -> Vec<(String, AbsPathBuf)> {
	let manifest_dir = env!("CARGO_MANIFEST_DIR");

	let mut args = vec!["build", "--message-format=json"];
	if include_tests {
		args.push("--tests");
	}

	let output = Command::new("cargo")
		.args(&args)
		.current_dir(manifest_dir)
		.output()
		.expect("Failed to run cargo build");

	let stdout = String::from_utf8_lossy(&output.stdout);
	let mut dylibs = Vec::new();

	for line in stdout.lines() {
		if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
			if json.get("reason").and_then(|r| r.as_str()) != Some("compiler-artifact") {
				continue;
			}

			let target = match json.get("target") {
				Some(t) => t,
				None => continue,
			};

			let kinds = match target.get("kind").and_then(|k| k.as_array()) {
				Some(k) => k,
				None => continue,
			};

			let is_proc_macro = kinds.iter().any(|k| k.as_str() == Some("proc-macro"));
			if !is_proc_macro {
				continue;
			}

			let crate_name = match target.get("name").and_then(|n| n.as_str()) {
				Some(n) => n.to_string(),
				None => continue,
			};

			let filenames = match json.get("filenames").and_then(|f| f.as_array()) {
				Some(f) => f,
				None => continue,
			};

			for filename in filenames {
				if let Some(path) = filename.as_str() {
					if path.ends_with(".so") || path.ends_with(".dylib") || path.ends_with(".dll") {
						dylibs.push((crate_name.clone(), AbsPathBuf::assert(path.into())));
					}
				}
			}
		}
	}

	dylibs
}

/// Build a map of macro names to their info
fn build_macro_map(client: &ProcMacroClient, dylibs: &[(String, AbsPathBuf)]) -> HashMap<String, Vec<DiscoveredMacro>> {
	let mut map: HashMap<String, Vec<DiscoveredMacro>> = HashMap::new();

	for (crate_name, dylib_path) in dylibs {
		match client.load_dylib(MacroDylib::new(dylib_path.clone()), None) {
			Ok(macros) => {
				for mac in macros {
					let discovered = DiscoveredMacro {
						name: mac.name().to_string(),
						kind: mac.kind(),
						crate_name: crate_name.clone(),
						dylib_path: dylib_path.clone(),
					};
					map.entry(mac.name().to_string()).or_default().push(discovered);
				}
			}
			Err(e) => {
				eprintln!("Failed to load {}: {}", dylib_path, e);
			}
		}
	}

	map
}

#[test]
fn test_macro_discovery() {
	let start = std::time::Instant::now();

	// Find proc-macro-srv
	let srv_path = match find_proc_macro_srv() {
		Some(p) => p,
		None => {
			eprintln!("Skipping: proc-macro-srv not found");
			return;
		}
	};
	eprintln!("[{:?}] Found proc-macro-srv", start.elapsed());

	// Spawn client
	let env: Vec<(String, &Option<String>)> = vec![];
	let client = match ProcMacroClient::spawn(&srv_path, env, None) {
		Ok(c) => c,
		Err(e) => {
			eprintln!("Failed to spawn client: {}", e);
			return;
		}
	};
	eprintln!("[{:?}] Spawned proc-macro client", start.elapsed());

	// Get all proc-macro dylibs (include test dependencies to find error_set)
	let dylib_start = std::time::Instant::now();
	let dylibs = get_proc_macro_dylibs(true);
	eprintln!(
		"[{:?}] Found {} proc-macro dylibs in {:?}",
		start.elapsed(),
		dylibs.len(),
		dylib_start.elapsed()
	);

	for (name, path) in &dylibs {
		eprintln!("  {} -> {}", name, path);
	}

	// Build macro map
	let map_start = std::time::Instant::now();
	let macro_map = build_macro_map(&client, &dylibs);
	eprintln!("[{:?}] Built macro map in {:?}", start.elapsed(), map_start.elapsed());

	eprintln!("\n=== Discovered Macros ===");
	let mut names: Vec<_> = macro_map.keys().collect();
	names.sort();
	for name in names {
		let macros = &macro_map[name];
		if macros.len() == 1 {
			let m = &macros[0];
			eprintln!("  {} ({:?}) <- {}", name, m.kind, m.crate_name);
		} else {
			eprintln!("  {} (AMBIGUOUS: {} sources)", name, macros.len());
			for m in macros {
				eprintln!("    - {:?} from {}", m.kind, m.crate_name);
			}
		}
	}

	// Test: can we find error_set?
	eprintln!("\n=== Looking up error_set ===");
	if let Some(macros) = macro_map.get("error_set") {
		eprintln!("Found error_set! in {} crate(s)", macros.len());
		for m in macros {
			eprintln!("  {:?} from {} at {}", m.kind, m.crate_name, m.dylib_path);
		}
	} else {
		eprintln!("error_set! not found in macro map");
	}

	eprintln!("\n[{:?}] Total time", start.elapsed());
}

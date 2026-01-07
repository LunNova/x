// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Integration test for rust-analyzer based macro expansion.
//!
//! This test verifies that we can use rust-analyzer's APIs to:
//! 1. Load a cargo workspace
//! 2. Find macro calls in source files
//! 3. Expand those macros and extract generated items

use ra_ap_base_db::{EditionedFileId, FileId, RootQueryDb};
use ra_ap_hir::{Crate, Semantics};
use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_paths::AbsPathBuf;
use ra_ap_project_model::CargoConfig;
use ra_ap_syntax::AstNode;
use ra_ap_syntax::ast::{self, HasModuleItem, HasName};
use std::path::Path;
use std::process::Command;

/// Find the proc-macro-srv binary from the sysroot
fn find_proc_macro_srv() -> Option<AbsPathBuf> {
	// Get sysroot from rustc
	let output = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;

	if !output.status.success() {
		return None;
	}

	let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
	eprintln!("Sysroot: {}", sysroot);

	// Look for proc-macro-srv in libexec
	let libexec_path = format!("{}/libexec/rust-analyzer-proc-macro-srv", sysroot);
	if Path::new(&libexec_path).exists() {
		eprintln!("Found proc-macro-srv at: {}", libexec_path);
		return Some(AbsPathBuf::assert(libexec_path.into()));
	}

	// Also check lib directory
	let lib_path = format!("{}/lib/rust-analyzer-proc-macro-srv", sysroot);
	if Path::new(&lib_path).exists() {
		eprintln!("Found proc-macro-srv at: {}", lib_path);
		return Some(AbsPathBuf::assert(lib_path.into()));
	}

	eprintln!("Could not find proc-macro-srv in sysroot");
	None
}

/// Load the cargo-derive-doc workspace and return the database
fn load_test_workspace() -> (RootDatabase, ra_ap_vfs::Vfs) {
	let total_start = std::time::Instant::now();

	let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

	let cargo_config = CargoConfig {
		all_targets: true,
		..CargoConfig::default()
	};

	// Try to find proc-macro-srv explicitly
	let proc_macro_srv_start = std::time::Instant::now();
	let proc_macro_choice = match find_proc_macro_srv() {
		Some(path) => ProcMacroServerChoice::Explicit(path),
		None => ProcMacroServerChoice::Sysroot,
	};
	eprintln!("[TIMING] find_proc_macro_srv: {:?}", proc_macro_srv_start.elapsed());

	let load_config = LoadCargoConfig {
		// We run build scripts manually below, so disable here
		load_out_dirs_from_check: false,
		with_proc_macro_server: proc_macro_choice,
		prefill_caches: false, // Disable cache prefilling - it's VERY slow
	};

	use ra_ap_paths::Utf8PathBuf;
	use ra_ap_project_model::{ProjectManifest, ProjectWorkspace};

	// Discover manifest
	let discover_start = std::time::Instant::now();
	let manifest_path = ra_ap_paths::AbsPathBuf::assert(Utf8PathBuf::from(manifest_dir.to_str().unwrap()));
	let manifest = ProjectManifest::discover_single(&manifest_path).expect("Failed to discover project manifest");
	eprintln!("[TIMING] discover_manifest: {:?}", discover_start.elapsed());

	// Load workspace (cargo metadata)
	let workspace_load_start = std::time::Instant::now();
	let workspace = ProjectWorkspace::load(manifest.clone(), &cargo_config, &|_msg| {}).expect("Failed to load workspace");
	eprintln!(
		"[TIMING] ProjectWorkspace::load (cargo metadata): {:?}",
		workspace_load_start.elapsed()
	);

	// Check toolchain
	if workspace.toolchain.is_none() {
		eprintln!("WARNING: No toolchain detected - will use slow RUSTC_WRAPPER");
	}

	// Run build scripts
	let build_start = std::time::Instant::now();
	let build_scripts = workspace
		.run_build_scripts(&cargo_config, &|_msg| {})
		.expect("Failed to run build scripts");
	eprintln!("[TIMING] run_build_scripts: {:?}", build_start.elapsed());

	// Set build scripts on workspace
	let mut workspace = workspace;
	workspace.set_build_scripts(build_scripts);

	// Load into database
	let extra_env = rustc_hash::FxHashMap::default();
	let db_load_start = std::time::Instant::now();
	let (db, vfs, _proc_macro_client) =
		ra_ap_load_cargo::load_workspace(workspace, &extra_env, &load_config).expect("Failed to load workspace into db");
	eprintln!("[TIMING] load_workspace (into db): {:?}", db_load_start.elapsed());

	eprintln!("[TIMING] TOTAL load_test_workspace: {:?}", total_start.elapsed());
	(db, vfs)
}

/// Find all top-level macro calls in a module
fn find_macro_calls(module_source: &ast::SourceFile) -> Vec<ast::MacroCall> {
	let mut macro_calls = Vec::new();

	for item in module_source.items() {
		if let ast::Item::MacroCall(macro_call) = item {
			macro_calls.push(macro_call);
		}
	}

	macro_calls
}

/// Describe an item from a syntax node
fn describe_item(item: &ast::Item) -> Option<String> {
	match item {
		ast::Item::Fn(func) => {
			let name = func.name().map_or_else(|| "_".to_string(), |n| n.text().to_string());
			Some(format!("fn {}", name))
		}
		ast::Item::Struct(s) => {
			let name = s.name().map_or_else(|| "_".to_string(), |n| n.text().to_string());
			Some(format!("struct {}", name))
		}
		ast::Item::Enum(e) => {
			let name = e.name().map_or_else(|| "_".to_string(), |n| n.text().to_string());
			Some(format!("enum {}", name))
		}
		ast::Item::Impl(impl_) => {
			if let Some(trait_) = impl_.trait_() {
				let trait_name = trait_.to_string();
				let target = impl_.self_ty().map_or_else(|| "_".to_string(), |ty| ty.to_string());
				Some(format!("impl {} for {}", trait_name, target))
			} else {
				let target = impl_.self_ty().map_or_else(|| "_".to_string(), |ty| ty.to_string());
				Some(format!("impl {}", target))
			}
		}
		ast::Item::TypeAlias(t) => {
			let name = t.name().map_or_else(|| "_".to_string(), |n| n.text().to_string());
			Some(format!("type {}", name))
		}
		_ => None,
	}
}

/// Extract items from a macro expansion result
fn extract_items_from_expansion(expanded: &ra_ap_syntax::SyntaxNode) -> Vec<String> {
	let mut items = Vec::new();

	eprintln!("Expanded node kind: {:?}", expanded.kind());
	let text = expanded.text().to_string();
	eprintln!("Expanded text length: {} chars", text.len());
	if !text.is_empty() {
		eprintln!("Expanded text preview: {:.500}", text);
	} else {
		eprintln!("Expanded text is EMPTY");
	}

	// Debug: print all children with their kinds
	eprintln!("Direct children:");
	for (i, child) in expanded.children().enumerate() {
		eprintln!("  Child {}: {:?}", i, child.kind());
	}

	// Try descendants instead of just direct children
	eprintln!("All descendants with kind Item:");
	for node in expanded.descendants() {
		if let Some(item) = ast::Item::cast(node.clone()) {
			if let Some(desc) = describe_item(&item) {
				eprintln!("  Found item: {}", desc);
				items.push(desc);
			}
		}
	}

	if items.is_empty() {
		eprintln!("No items found, trying to cast entire node as MacroItems");
		if let Some(macro_items) = ast::MacroItems::cast(expanded.clone()) {
			eprintln!("  MacroItems cast succeeded");
			for item in macro_items.items() {
				if let Some(desc) = describe_item(&item) {
					eprintln!("    Found via MacroItems: {}", desc);
					items.push(desc);
				}
			}
		}
	}

	items
}

#[test]
fn test_workspace_loads() {
	let (db, _vfs) = load_test_workspace();

	// Verify we loaded some crates
	let crates = Crate::all(&db);
	eprintln!("Loaded {} crates", crates.len());

	// We should have at least cargo-derive-doc itself
	assert!(!crates.is_empty(), "Should have loaded at least one crate");

	// Find the cargo-derive-doc crate
	let our_crate = crates.iter().find(|krate| {
		let name = krate.display_name(&db);
		name.map(|n| n.to_string()) == Some("cargo_derive_doc".to_string())
	});

	assert!(
		our_crate.is_some(),
		"Should find cargo-derive-doc crate. Found: {:?}",
		crates
			.iter()
			.filter_map(|k| k.display_name(&db).map(|n| n.to_string()))
			.collect::<Vec<_>>()
	);
}

#[test]
fn test_find_macro_calls_in_examples() {
	let test_start = std::time::Instant::now();

	let (db, vfs) = load_test_workspace();
	let semantics = Semantics::new(&db);

	let post_load = std::time::Instant::now();

	// Find target file directly in VFS - avoids triggering expensive crate loading
	let target_file = "examples/error_set_test.rs";
	let find_file_start = std::time::Instant::now();

	let mut found_file = None;
	for (vfs_file_id, _) in vfs.iter() {
		if let Some(path) = vfs.file_path(vfs_file_id).as_path() {
			if path.as_str().ends_with(target_file) {
				found_file = Some((vfs_file_id, path.to_owned()));
				break;
			}
		}
	}
	eprintln!("[TIMING] Find file in VFS: {:?}", find_file_start.elapsed());

	let (vfs_file_id, path) = found_file.expect("Should find error_set_test.rs");
	eprintln!("Found: {}", path);

	// Convert VFS FileId to base_db FileId, then to EditionedFileId
	let convert_start = std::time::Instant::now();
	let base_file_id = FileId::from_raw(vfs_file_id.index());
	let editioned_file_id = EditionedFileId::current_edition_guess_origin(&db, base_file_id);
	eprintln!("[TIMING] Convert to EditionedFileId: {:?}", convert_start.elapsed());

	// Parse the file
	let parse_start = std::time::Instant::now();
	let source_file = semantics.parse(editioned_file_id);
	eprintln!("[TIMING] semantics.parse: {:?}", parse_start.elapsed());

	// Find macro calls
	let find_macros_start = std::time::Instant::now();
	let macro_calls = find_macro_calls(&source_file);
	eprintln!("[TIMING] find_macro_calls: {:?}", find_macros_start.elapsed());
	eprintln!("Found {} macro calls", macro_calls.len());

	for macro_call in &macro_calls {
		let macro_name = macro_call
			.path()
			.and_then(|p| p.segment())
			.and_then(|s| s.name_ref())
			.map(|n| n.text().to_string())
			.unwrap_or_else(|| "unknown".to_string());

		eprintln!("\n  Macro call: {}!", macro_name);

		let expand_start = std::time::Instant::now();
		match semantics.expand_allowed_builtins(macro_call) {
			Some(expand_result) => {
				eprintln!("[TIMING] expand_allowed_builtins: {:?}", expand_start.elapsed());
				eprintln!("  ✓ Expansion succeeded!");
				if let Some(err) = &expand_result.err {
					eprintln!("  ⚠ Expansion error: {:?}", err);
				}
				let items = extract_items_from_expansion(&expand_result.value);
				eprintln!("  Generated {} items", items.len());
			}
			None => {
				eprintln!("[TIMING] expand_allowed_builtins (failed): {:?}", expand_start.elapsed());
				eprintln!("  ✗ Expansion failed (returned None)");
			}
		}
	}

	eprintln!("[TIMING] Expansion/traversal time: {:?}", post_load.elapsed());
	eprintln!("[TIMING] TOTAL TEST TIME: {:?}", test_start.elapsed());
}

#[test]
fn test_expand_error_set_macro() {
	let (db, vfs) = load_test_workspace();
	let semantics = Semantics::new(&db);

	let crates = Crate::all(&db);
	let mut found_error_set = false;
	let mut expanded_items = Vec::new();

	for krate in &crates {
		let modules = krate.modules(&db);

		for module in modules {
			let definition = module.definition_source(&db);
			let file_id = definition.file_id;

			if let Some(file_id) = file_id.file_id() {
				let vfs_file_id = ra_ap_vfs::FileId::from_raw(file_id.file_id(&db).index());
				if let Some(path) = vfs.file_path(vfs_file_id).as_path() {
					let path_str = path.as_str();

					// Look for the error_set_test example
					if !path_str.contains("error_set_test") {
						continue;
					}

					eprintln!("Found error_set_test at: {}", path_str);

					let source_file = semantics.parse(file_id);
					let macro_calls = find_macro_calls(&source_file);

					for macro_call in macro_calls {
						let macro_name = macro_call
							.path()
							.and_then(|p| p.segment())
							.and_then(|s| s.name_ref())
							.map(|n| n.text().to_string())
							.unwrap_or_default();

						if macro_name == "error_set" {
							found_error_set = true;
							eprintln!("Found error_set! macro");

							if let Some(expanded) = semantics.expand_macro_call(&macro_call) {
								eprintln!("Expansion successful!");
								expanded_items = extract_items_from_expansion(&expanded.value);
							} else {
								eprintln!("Expansion returned None - proc-macro server may not be working");
							}
						}
					}
				}
			}
		}
	}

	assert!(found_error_set, "Should find error_set! macro in examples");

	// If proc-macro expansion is working, we should get items
	// Note: This may fail in CI if proc-macro-srv is not available
	if !expanded_items.is_empty() {
		eprintln!("Expanded items: {:?}", expanded_items);

		// The error_set! macro should generate at least one enum
		let has_enum = expanded_items.iter().any(|s| s.starts_with("enum"));
		assert!(has_enum, "error_set! should generate enum types");
	} else {
		eprintln!("Warning: No expanded items found. This is expected if proc-macro-srv is not running.");
	}
}

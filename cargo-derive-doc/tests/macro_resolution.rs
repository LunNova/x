// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test to see if RA's macro resolution is fast enough to use without triggering
//! full semantic analysis.

use ra_ap_base_db::{EditionedFileId, FileId};
use ra_ap_hir::{HasCrate, Semantics};
use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_paths::AbsPathBuf;
use ra_ap_project_model::CargoConfig;
use ra_ap_syntax::ast::{self, HasModuleItem};
use std::path::Path;
use std::process::Command;

fn find_proc_macro_srv() -> Option<AbsPathBuf> {
	let output = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
	if !output.status.success() {
		return None;
	}
	let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();

	let libexec_path = format!("{}/libexec/rust-analyzer-proc-macro-srv", sysroot);
	if Path::new(&libexec_path).exists() {
		return Some(AbsPathBuf::assert(libexec_path.into()));
	}
	let lib_path = format!("{}/lib/rust-analyzer-proc-macro-srv", sysroot);
	if Path::new(&lib_path).exists() {
		return Some(AbsPathBuf::assert(lib_path.into()));
	}
	None
}

fn load_workspace() -> (RootDatabase, ra_ap_vfs::Vfs) {
	let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

	let cargo_config = CargoConfig {
		all_targets: true,
		..CargoConfig::default()
	};

	let proc_macro_choice = match find_proc_macro_srv() {
		Some(path) => ProcMacroServerChoice::Explicit(path),
		None => ProcMacroServerChoice::Sysroot,
	};

	let load_config = LoadCargoConfig {
		load_out_dirs_from_check: false,
		with_proc_macro_server: proc_macro_choice,
		prefill_caches: false,
	};

	use ra_ap_paths::Utf8PathBuf;
	use ra_ap_project_model::{ProjectManifest, ProjectWorkspace};

	let manifest_path = ra_ap_paths::AbsPathBuf::assert(Utf8PathBuf::from(manifest_dir.to_str().unwrap()));
	let manifest = ProjectManifest::discover_single(&manifest_path).expect("Failed to discover manifest");

	let workspace = ProjectWorkspace::load(manifest.clone(), &cargo_config, &|_| {}).expect("Failed to load workspace");

	let build_scripts = workspace
		.run_build_scripts(&cargo_config, &|_| {})
		.expect("Failed to run build scripts");

	let mut workspace = workspace;
	workspace.set_build_scripts(build_scripts);

	let extra_env = rustc_hash::FxHashMap::default();
	let (db, vfs, _) = ra_ap_load_cargo::load_workspace(workspace, &extra_env, &load_config).expect("Failed to load into db");

	(db, vfs)
}

#[test]
fn test_macro_resolution_speed() {
	let total_start = std::time::Instant::now();

	eprintln!("\n=== Loading workspace ===");
	let load_start = std::time::Instant::now();
	let (db, vfs) = load_workspace();
	eprintln!("[{:?}] Workspace loaded", load_start.elapsed());

	let semantics = Semantics::new(&db);

	// Find the error_set_test.rs file
	let target_file = "examples/error_set_test.rs";
	let mut found_file = None;
	for (vfs_file_id, _) in vfs.iter() {
		if let Some(path) = vfs.file_path(vfs_file_id).as_path() {
			if path.as_str().ends_with(target_file) {
				found_file = Some((vfs_file_id, path.to_owned()));
				break;
			}
		}
	}

	let (vfs_file_id, path) = found_file.expect("Should find error_set_test.rs");
	eprintln!("[{:?}] Found file: {}", total_start.elapsed(), path);

	// Convert to EditionedFileId
	let base_file_id = FileId::from_raw(vfs_file_id.index());
	let editioned_file_id = EditionedFileId::current_edition_guess_origin(&db, base_file_id);

	// Parse the file
	let parse_start = std::time::Instant::now();
	let source_file = semantics.parse(editioned_file_id);
	eprintln!("[{:?}] File parsed in {:?}", total_start.elapsed(), parse_start.elapsed());

	// Find macro calls
	let mut macro_calls = Vec::new();
	for item in source_file.items() {
		if let ast::Item::MacroCall(macro_call) = item {
			macro_calls.push(macro_call);
		}
	}
	eprintln!("[{:?}] Found {} macro calls", total_start.elapsed(), macro_calls.len());

	// Try to resolve each macro call
	eprintln!("\n=== Resolving macro calls ===");
	for macro_call in &macro_calls {
		let macro_name = macro_call
			.path()
			.and_then(|p| p.segment())
			.and_then(|s| s.name_ref())
			.map(|n| n.text().to_string())
			.unwrap_or_else(|| "unknown".to_string());

		eprintln!("\nMacro: {}!", macro_name);

		let resolve_start = std::time::Instant::now();
		let resolved = semantics.resolve_macro_call(macro_call);
		let resolve_time = resolve_start.elapsed();

		match resolved {
			Some(mac) => {
				let mac_name = format!("{:?}", mac.name(&db));
				let krate = mac.krate(&db);
				let krate_name = krate.display_name(&db).map(|n| n.to_string());
				eprintln!("  ✓ Resolved in {:?}: {} from crate {:?}", resolve_time, mac_name, krate_name);
			}
			None => {
				eprintln!("  ✗ Failed to resolve in {:?}", resolve_time);
			}
		}
	}

	eprintln!("\n[{:?}] Total test time", total_start.elapsed());
}

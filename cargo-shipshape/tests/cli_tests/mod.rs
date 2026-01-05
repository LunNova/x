use super::*;
use assert_cmd::cargo_bin_cmd;
use std::os::unix::fs::PermissionsExt;

#[test]
fn test_dry_run_with_extraction() {
	let tempdir = tempfile::tempdir().unwrap();
	let temp_file = tempdir.path().join("lib.rs");
	// Module with >5 lines triggers extraction
	fs::write(
		&temp_file,
		"mod large {\n    fn a() {}\n    fn b() {}\n    fn c() {}\n    fn d() {}\n    fn e() {}\n    fn f() {}\n}\n",
	)
	.unwrap();

	let result = run_sort_items(&["--dry-run", "--extract-threshold", "5", temp_file.to_str().unwrap()]);
	assert!(result.success());

	// File unchanged, no extraction created
	assert!(!tempdir.path().join("large.rs").exists());
}

#[test]
fn test_extraction_creates_parent_dir() {
	let tempdir = tempfile::tempdir().unwrap();
	// Non-root file: foo.rs → foo/large.rs (needs parent mkdir)
	let temp_file = tempdir.path().join("foo.rs");
	fs::write(
		&temp_file,
		"mod large {\n    fn a() {}\n    fn b() {}\n    fn c() {}\n    fn d() {}\n    fn e() {}\n    fn f() {}\n}\n",
	)
	.unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", temp_file.to_str().unwrap()]);
	assert!(result.success());

	// Should have created foo/large.rs
	assert!(tempdir.path().join("foo").join("large.rs").exists());
}

#[test]
fn test_binary_help() {
	cargo_bin_cmd!("cargo-shipshape").arg("--help").assert().success();
}

#[test]
fn test_binary_shipshape_subcommand() {
	cargo_bin_cmd!("cargo-shipshape").args(["shipshape", "--help"]).assert().success();
}

#[test]
fn test_binary_no_args() {
	cargo_bin_cmd!("cargo-shipshape").assert().failure();
}

#[test]
fn test_check_mode_unsorted() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn b() {}\nfn a() {}\n").unwrap();

	let result = run_sort_items(&["--check", temp_file.to_str().unwrap()]);

	assert!(!result.success(), "Check mode should fail for unsorted file");

	let original = fs::read_to_string(&temp_file).unwrap();
	assert_eq!(original, "fn b() {}\nfn a() {}\n", "Check mode should not modify file");
}

#[test]
fn test_check_mode_sorted() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn a() {}\n\nfn b() {}\n").unwrap();

	let result = run_sort_items(&["--check", temp_file.to_str().unwrap()]);

	assert!(result.success(), "Check mode should pass for sorted file");
}

#[test]
fn test_dry_run() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	let original = "fn b() {}\nfn a() {}\n";
	fs::write(&temp_file, original).unwrap();

	let result = run_sort_items(&["--dry-run", temp_file.to_str().unwrap()]);

	assert!(result.success());

	let after = fs::read_to_string(&temp_file).unwrap();
	assert_eq!(after, original, "Dry run should not modify file");
}

#[test]
fn test_diff_output() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn b() {}\nfn a() {}\n").unwrap();

	let result = run_sort_items(&["--diff", "--dry-run", temp_file.to_str().unwrap()]);

	assert!(result.success());
}

#[test]
fn test_nonexistent_file() {
	let result = run_sort_items(&["/nonexistent/path/file.rs"]);

	assert!(!result.success());
}

#[test]
fn test_directory_without_recursive() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");

	let result = run_sort_items(&[tempdir.path().to_str().unwrap()]);

	assert!(!result.success(), "Should fail when no .rs files found");
}

#[test]
fn test_recursive_mode() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let subdir = tempdir.path().join("subdir");
	fs::create_dir(&subdir).unwrap();

	fs::write(tempdir.path().join("a.rs"), "fn b() {}\nfn a() {}\n").unwrap();
	fs::write(subdir.join("b.rs"), "fn d() {}\nfn c() {}\n").unwrap();

	let result = run_sort_items(&["--recursive", tempdir.path().to_str().unwrap()]);

	assert!(result.success(), "Recursive mode should succeed");

	let a_content = fs::read_to_string(tempdir.path().join("a.rs")).unwrap();
	let b_content = fs::read_to_string(subdir.join("b.rs")).unwrap();

	assert!(
		a_content.find("fn a()").unwrap() < a_content.find("fn b()").unwrap(),
		"a.rs should be sorted"
	);
	assert!(
		b_content.find("fn c()").unwrap() < b_content.find("fn d()").unwrap(),
		"subdir/b.rs should be sorted"
	);
}

#[test]
fn test_syntax_error_handling() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn broken( {}\n").unwrap();

	let result = run_sort_items(&[temp_file.to_str().unwrap()]);

	assert!(!result.success(), "Should fail for syntax errors");

	let after = fs::read_to_string(&temp_file).unwrap();
	assert_eq!(after, "fn broken( {}\n", "Broken files should not be modified");
}

#[test]
fn test_syntax_error_in_sort() {
	// --no-extract skips extraction, so parse error hits sort.rs instead
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn broken( {}\n").unwrap();

	let result = run_sort_items(&["--no-extract", temp_file.to_str().unwrap()]);

	assert!(!result.success(), "Should fail for syntax errors in sort");
}

#[test]
fn test_asm_expr_rejected() {
	// AsmExpr is an internal rust-analyzer node that shouldn't appear in normal code
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, r#"builtin#global_asm("nop")"#).unwrap();

	let path = temp_file.to_str().unwrap().to_string();
	let result = std::panic::catch_unwind(|| run_sort_items(&["--no-extract", &path]));

	assert!(result.is_err(), "Should panic for AsmExpr items");
}

#[test]
fn test_idempotent() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn b() {}\nfn a() {}\nstruct C;\n").unwrap();

	run_sort_items(&[temp_file.to_str().unwrap()]);
	let after_first = fs::read_to_string(&temp_file).unwrap();

	run_sort_items(&[temp_file.to_str().unwrap()]);
	let after_second = fs::read_to_string(&temp_file).unwrap();

	assert_eq!(after_first, after_second, "Sorting should be idempotent");
}

#[test]
fn test_multiple_files() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let file1 = tempdir.path().join("a.rs");
	let file2 = tempdir.path().join("b.rs");

	fs::write(&file1, "fn b() {}\nfn a() {}\n").unwrap();
	fs::write(&file2, "fn d() {}\nfn c() {}\n").unwrap();

	let result = run_sort_items(&[file1.to_str().unwrap(), file2.to_str().unwrap()]);

	assert!(result.success());

	let content1 = fs::read_to_string(&file1).unwrap();
	let content2 = fs::read_to_string(&file2).unwrap();

	assert!(content1.find("fn a()").unwrap() < content1.find("fn b()").unwrap());
	assert!(content2.find("fn c()").unwrap() < content2.find("fn d()").unwrap());
}

#[test]
fn test_write_error_readonly_file() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn b() {}\nfn a() {}\n").unwrap();

	// Make file read-only
	let mut perms = fs::metadata(&temp_file).unwrap().permissions();
	perms.set_mode(0o444);
	fs::set_permissions(&temp_file, perms).unwrap();

	let result = run_sort_items(&[temp_file.to_str().unwrap()]);

	// Restore permissions for cleanup
	let mut perms = fs::metadata(&temp_file).unwrap().permissions();
	perms.set_mode(0o644);
	fs::set_permissions(&temp_file, perms).unwrap();

	assert!(!result.success(), "Should fail when file is read-only");
}

#[test]
fn test_write_error_readonly_dir_for_extraction() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("lib.rs");

	// Large module to trigger extraction
	let content = format!(
		"mod large {{\n{}\n}}\n",
		(0..20).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&temp_file, &content).unwrap();

	// Make directory read-only (can't create new files)
	let mut perms = fs::metadata(tempdir.path()).unwrap().permissions();
	perms.set_mode(0o555);
	fs::set_permissions(tempdir.path(), perms).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", temp_file.to_str().unwrap()]);

	// Restore permissions for cleanup
	let mut perms = fs::metadata(tempdir.path()).unwrap().permissions();
	perms.set_mode(0o755);
	fs::set_permissions(tempdir.path(), perms).unwrap();

	assert!(!result.success(), "Should fail when directory is read-only");
}

#[test]
fn test_create_dir_error_for_extraction_subdir() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	// Non-root file: foo.rs → foo/large.rs (needs to create foo/ subdir)
	let temp_file = tempdir.path().join("foo.rs");

	let content = format!(
		"mod large {{\n{}\n}}\n",
		(0..20).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&temp_file, &content).unwrap();

	// Make directory read-only (can't create new subdirectories)
	let mut perms = fs::metadata(tempdir.path()).unwrap().permissions();
	perms.set_mode(0o555);
	fs::set_permissions(tempdir.path(), perms).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", temp_file.to_str().unwrap()]);

	// Restore permissions for cleanup
	let mut perms = fs::metadata(tempdir.path()).unwrap().permissions();
	perms.set_mode(0o755);
	fs::set_permissions(tempdir.path(), perms).unwrap();

	assert!(!result.success(), "Should fail when can't create extraction subdirectory");
}

#[test]
fn test_read_error_unreadable_file() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "fn a() {}\n").unwrap();

	// Make file unreadable (write-only)
	let mut perms = fs::metadata(&temp_file).unwrap().permissions();
	perms.set_mode(0o200);
	fs::set_permissions(&temp_file, perms).unwrap();

	let result = run_sort_items(&[temp_file.to_str().unwrap()]);

	// Restore permissions for cleanup
	let mut perms = fs::metadata(&temp_file).unwrap().permissions();
	perms.set_mode(0o644);
	fs::set_permissions(&temp_file, perms).unwrap();

	assert!(!result.success(), "Should fail when file is unreadable");
}

#[test]
fn test_file_without_trailing_newline() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	// Items on same line - no newlines between them
	fs::write(&temp_file, "fn b() {} fn a() {}").unwrap();

	let result = run_sort_items(&[temp_file.to_str().unwrap()]);
	assert!(result.success());

	let content = fs::read_to_string(&temp_file).unwrap();
	// Should add trailing newline and sort
	assert!(content.ends_with('\n'), "Should add trailing newline");
	assert!(content.find("fn a()").unwrap() < content.find("fn b()").unwrap());
}

#[test]
fn test_empty_file() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let temp_file = tempdir.path().join("test.rs");
	fs::write(&temp_file, "").unwrap();

	let result = run_sort_items(&[temp_file.to_str().unwrap()]);
	assert!(result.success());

	let content = fs::read_to_string(&temp_file).unwrap();
	assert_eq!(content, "", "Empty file should stay empty");
}

use super::*;

#[test]
fn test_module_extraction() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let main_file = tempdir.path().join("lib.rs");

	let large_mod = format!(
		"mod large {{\n{}\n}}\n\nfn main() {{}}\n",
		(0..50).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&main_file, &large_mod).unwrap();

	let result = run_sort_items(&["--extract-threshold", "10", main_file.to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	let main_content = fs::read_to_string(&main_file).unwrap();
	assert!(main_content.contains("mod large;"), "Main file should have module declaration");
	assert!(!main_content.contains("fn func_0"), "Main file should not have extracted functions");

	let extracted_file = tempdir.path().join("large.rs");
	assert!(extracted_file.exists(), "Extracted module file should exist");

	let extracted_content = fs::read_to_string(&extracted_file).unwrap();
	assert!(extracted_content.contains("fn func_0"), "Extracted file should have functions");
}

#[test]
fn test_multiple_module_extraction() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let main_file = tempdir.path().join("lib.rs");

	// Two large modules - extraction replaces from end-to-start to preserve byte offsets
	let content = format!(
		"mod alpha {{\n{}\n}}\n\nmod beta {{\n{}\n}}\n",
		(0..20).map(|i| format!("    fn a_{i}() {{}}")).collect::<Vec<_>>().join("\n"),
		(0..20).map(|i| format!("    fn b_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&main_file, &content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "10", main_file.to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// Verify main file has both module declarations (not inline bodies)
	let main_content = fs::read_to_string(&main_file).unwrap();
	assert!(main_content.contains("mod alpha;"), "should have alpha declaration");
	assert!(main_content.contains("mod beta;"), "should have beta declaration");
	assert!(!main_content.contains("fn a_0"), "alpha body should be extracted");
	assert!(!main_content.contains("fn b_0"), "beta body should be extracted");

	// Verify extracted files exist with correct content
	let alpha = fs::read_to_string(tempdir.path().join("alpha.rs")).unwrap();
	let beta = fs::read_to_string(tempdir.path().join("beta.rs")).unwrap();
	assert!(alpha.contains("fn a_0"), "alpha.rs should have alpha functions");
	assert!(beta.contains("fn b_0"), "beta.rs should have beta functions");
}

#[test]
fn test_no_extraction_disabled() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let main_file = tempdir.path().join("lib.rs");

	let large_mod = format!(
		"mod large {{\n{}\n}}\n",
		(0..50).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&main_file, &large_mod).unwrap();

	let result = run_sort_items(&["--no-extract", main_file.to_str().unwrap()]);

	assert!(result.success());

	let main_content = fs::read_to_string(&main_file).unwrap();
	assert!(
		main_content.contains("mod large {"),
		"Module should not be extracted with --no-extract"
	);

	let extracted_file = tempdir.path().join("large.rs");
	assert!(!extracted_file.exists(), "No file should be extracted with --no-extract");
}

#[test]
fn test_extraction_preserves_attributes() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let main_file = tempdir.path().join("lib.rs");

	let large_mod = format!(
		"/// Module documentation\n#[cfg(test)]\nmod tests {{\n{}\n}}\n",
		(0..20).map(|i| format!("    fn test_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&main_file, &large_mod).unwrap();

	run_sort_items(&["--extract-threshold", "5", main_file.to_str().unwrap()]);

	let main_content = fs::read_to_string(&main_file).unwrap();
	assert!(main_content.contains("/// Module documentation"), "Doc comment should be preserved");
	assert!(main_content.contains("#[cfg(test)]"), "Attribute should be preserved");
	assert!(main_content.contains("mod tests;"), "Module declaration should exist");
}

#[test]
fn test_extraction_uses_mod_dir_when_file_exists() {
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let main_file = tempdir.path().join("lib.rs");
	let existing_file = tempdir.path().join("existing.rs");

	fs::write(&existing_file, "// Existing file\n").unwrap();

	let large_mod = format!(
		"mod existing {{\n{}\n}}\n",
		(0..20).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
	);
	fs::write(&main_file, &large_mod).unwrap();

	run_sort_items(&["--extract-threshold", "5", main_file.to_str().unwrap()]);

	let mod_dir = tempdir.path().join("existing");
	let mod_file = mod_dir.join("mod.rs");

	assert!(mod_file.exists(), "Should create existing/mod.rs when existing.rs exists");
}

/// Helper to generate a large module body
fn large_module_body(count: usize) -> String {
	(0..count).map(|i| format!("    fn func_{i}() {{}}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn test_extraction_from_non_root_creates_subdir() {
	// src/foo.rs with large mod bar → src/foo/bar.rs
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let src_dir = tempdir.path().join("src");
	fs::create_dir_all(&src_dir).unwrap();

	// Create Cargo.toml
	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"
"#,
	)
	.unwrap();

	// Create src/lib.rs that references foo
	fs::write(src_dir.join("lib.rs"), "mod foo;\n").unwrap();

	// Create src/foo.rs with a large inline module
	let foo_content = format!("mod bar {{\n{}\n}}\n", large_module_body(20));
	fs::write(src_dir.join("foo.rs"), &foo_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", src_dir.join("foo.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// foo.rs is NOT a crate root, so bar should be extracted to src/foo/bar.rs
	let extracted_file = src_dir.join("foo").join("bar.rs");
	assert!(
		extracted_file.exists(),
		"Should create src/foo/bar.rs for non-root file, not src/bar.rs"
	);

	// Verify src/bar.rs was NOT created (wrong location)
	assert!(
		!src_dir.join("bar.rs").exists(),
		"Should NOT create src/bar.rs (sibling) for non-root file"
	);
}

#[test]
fn test_extraction_from_lib_rs_creates_sibling() {
	// src/lib.rs with large mod → sibling file
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let src_dir = tempdir.path().join("src");
	fs::create_dir_all(&src_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"
"#,
	)
	.unwrap();

	let lib_content = format!("mod extracted {{\n{}\n}}\n", large_module_body(20));
	fs::write(src_dir.join("lib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", src_dir.join("lib.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// lib.rs IS a crate root, so extracted should be a sibling
	let extracted_file = src_dir.join("extracted.rs");
	assert!(extracted_file.exists(), "Should create src/extracted.rs for crate root lib.rs");
}

#[test]
fn test_extraction_from_explicit_lib_path() {
	// Cargo.toml: [lib] path = "mylib.rs" → sibling file
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[lib]
path = "mylib.rs"
"#,
	)
	.unwrap();

	let lib_content = format!("mod extracted {{\n{}\n}}\n", large_module_body(20));
	fs::write(tempdir.path().join("mylib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tempdir.path().join("mylib.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// mylib.rs IS a crate root (explicit path), so extracted should be a sibling
	let extracted_file = tempdir.path().join("extracted.rs");
	assert!(
		extracted_file.exists(),
		"Should create extracted.rs as sibling for explicit lib path"
	);
}

#[test]
fn test_extraction_from_mod_rs() {
	// src/utils/mod.rs with large mod → src/utils/helper.rs (sibling)
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let utils_dir = tempdir.path().join("src").join("utils");
	fs::create_dir_all(&utils_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"
"#,
	)
	.unwrap();

	fs::write(tempdir.path().join("src").join("lib.rs"), "mod utils;\n").unwrap();

	let mod_content = format!("mod helper {{\n{}\n}}\n", large_module_body(20));
	fs::write(utils_dir.join("mod.rs"), &mod_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", utils_dir.join("mod.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// mod.rs can always have sibling modules
	let extracted_file = utils_dir.join("helper.rs");
	assert!(extracted_file.exists(), "Should create src/utils/helper.rs as sibling for mod.rs");
}

#[test]
fn test_extraction_from_test_file_uses_mod_rs() {
	// tests/integration.rs with large mod → tests/foo/mod.rs (not tests/foo.rs)
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let tests_dir = tempdir.path().join("tests");
	fs::create_dir_all(&tests_dir).unwrap();
	fs::create_dir_all(tempdir.path().join("src")).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"
"#,
	)
	.unwrap();

	fs::write(tempdir.path().join("src").join("lib.rs"), "").unwrap();

	let test_content = format!("mod helper {{\n{}\n}}\n\n#[test]\nfn it_works() {{}}\n", large_module_body(20));
	fs::write(tests_dir.join("integration.rs"), &test_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tests_dir.join("integration.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// tests/*.rs should use mod.rs form to avoid creating new test binaries
	let extracted_mod_rs = tests_dir.join("helper").join("mod.rs");
	let extracted_sibling = tests_dir.join("helper.rs");

	assert!(
		extracted_mod_rs.exists(),
		"Should create tests/helper/mod.rs for test file extraction"
	);
	assert!(
		!extracted_sibling.exists(),
		"Should NOT create tests/helper.rs (would become new test binary)"
	);
}

#[test]
fn test_extraction_no_cargo_toml_fallback() {
	// No Cargo.toml: lib.rs gets sibling, foo.rs gets subdir
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");

	// lib.rs without Cargo.toml - should still get sibling (fallback heuristic)
	let lib_content = format!("mod extracted {{\n{}\n}}\n", large_module_body(20));
	fs::write(tempdir.path().join("lib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tempdir.path().join("lib.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	assert!(
		tempdir.path().join("extracted.rs").exists(),
		"lib.rs should get sibling even without Cargo.toml (fallback)"
	);

	// Now test foo.rs without Cargo.toml - should get subdir
	let tempdir2 = tempfile::tempdir().expect("Failed to create temp dir");
	let foo_content = format!("mod bar {{\n{}\n}}\n", large_module_body(20));
	fs::write(tempdir2.path().join("foo.rs"), &foo_content).unwrap();

	let result2 = run_sort_items(&["--extract-threshold", "5", tempdir2.path().join("foo.rs").to_str().unwrap()]);

	assert!(result2.success(), "Extraction should succeed");

	assert!(
		tempdir2.path().join("foo").join("bar.rs").exists(),
		"foo.rs should get subdir without Cargo.toml"
	);
	assert!(
		!tempdir2.path().join("bar.rs").exists(),
		"foo.rs should NOT get sibling without Cargo.toml"
	);
}

#[test]
fn test_no_extraction_from_shebang_script() {
	// File starting with #!/usr/bin/env cargo → no extraction
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");

	let script_content = format!(
		"#!/usr/bin/env cargo +nightly -Zscript\n\nmod large {{\n{}\n}}\n\nfn main() {{}}\n",
		large_module_body(20)
	);
	fs::write(tempdir.path().join("script.rs"), &script_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tempdir.path().join("script.rs").to_str().unwrap()]);

	assert!(result.success(), "Should succeed");

	// Script should NOT have extraction (scripts can't have external modules)
	let script_after = fs::read_to_string(tempdir.path().join("script.rs")).unwrap();
	assert!(
		script_after.contains("mod large {"),
		"Shebang script should keep inline module (no extraction)"
	);
	assert!(!tempdir.path().join("large.rs").exists(), "Should not extract from shebang script");
}

#[test]
fn test_invalid_cargo_toml_fallback() {
	// Invalid TOML: lib.rs should still get sibling via fallback heuristics
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let src_dir = tempdir.path().join("src");
	fs::create_dir_all(&src_dir).unwrap();

	// Create invalid Cargo.toml (malformed TOML)
	fs::write(tempdir.path().join("Cargo.toml"), "this is not { valid toml").unwrap();

	let lib_content = format!("mod extracted {{\n{}\n}}\n", large_module_body(20));
	fs::write(src_dir.join("lib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", src_dir.join("lib.rs").to_str().unwrap()]);

	assert!(result.success(), "Should succeed with fallback despite invalid TOML");
	assert!(
		src_dir.join("extracted.rs").exists(),
		"lib.rs should get sibling using fallback when Cargo.toml is invalid"
	);
}

#[test]
fn test_lib_section_without_path() {
	// [lib] exists but has no path = "..." → should use default src/lib.rs
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let src_dir = tempdir.path().join("src");
	fs::create_dir_all(&src_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[lib]
name = "mylib"
"#,
	)
	.unwrap();

	let lib_content = format!("mod extracted {{\n{}\n}}\n", large_module_body(20));
	fs::write(src_dir.join("lib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", src_dir.join("lib.rs").to_str().unwrap()]);

	assert!(result.success());
	assert!(
		src_dir.join("extracted.rs").exists(),
		"lib.rs recognized as root with [lib] section but no path"
	);
}

#[test]
fn test_bin_with_explicit_path() {
	// [[bin]] path = "custom/main.rs" → recognized as crate root
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let custom_dir = tempdir.path().join("custom");
	fs::create_dir_all(&custom_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "mybin"
path = "custom/main.rs"
"#,
	)
	.unwrap();

	let bin_content = format!("mod extracted {{\n{}\n}}\n\nfn main() {{}}\n", large_module_body(20));
	fs::write(custom_dir.join("main.rs"), &bin_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", custom_dir.join("main.rs").to_str().unwrap()]);

	assert!(result.success());
	assert!(
		custom_dir.join("extracted.rs").exists(),
		"[[bin]] with explicit path should be recognized as crate root"
	);
}

#[test]
fn test_bin_with_name_only() {
	// [[bin]] name = "foo" (no path) → looks for src/bin/foo.rs or src/bin/foo/main.rs
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let src_dir = tempdir.path().join("src");
	let bin_dir = src_dir.join("bin");
	fs::create_dir_all(&bin_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "mybin"
"#,
	)
	.unwrap();

	// Need a .rs file in src/ so find_cargo_toml doesn't stop walking
	fs::write(src_dir.join("lib.rs"), "").unwrap();

	let bin_content = format!("mod extracted {{\n{}\n}}\n\nfn main() {{}}\n", large_module_body(20));
	fs::write(bin_dir.join("mybin.rs"), &bin_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", bin_dir.join("mybin.rs").to_str().unwrap()]);

	assert!(result.success());
	// [[bin]] with name should recognize src/bin/name.rs as crate root
	assert!(
		bin_dir.join("extracted.rs").exists(),
		"[[bin]] with name should find src/bin/name.rs as crate root"
	);
}

#[test]
fn test_test_with_explicit_path() {
	// [[test]] path = "custom_tests/foo.rs" → recognized as test root
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let custom_tests = tempdir.path().join("custom_tests");
	fs::create_dir_all(&custom_tests).unwrap();
	fs::create_dir_all(tempdir.path().join("src")).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[[test]]
name = "custom"
path = "custom_tests/foo.rs"
"#,
	)
	.unwrap();

	fs::write(tempdir.path().join("src").join("lib.rs"), "").unwrap();

	let test_content = format!("mod helper {{\n{}\n}}\n\n#[test]\nfn works() {{}}\n", large_module_body(20));
	fs::write(custom_tests.join("foo.rs"), &test_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", custom_tests.join("foo.rs").to_str().unwrap()]);

	assert!(result.success());
	// Since this is a test root (via explicit path), it should get sibling extraction
	assert!(
		custom_tests.join("helper.rs").exists(),
		"[[test]] with explicit path should be recognized as crate root"
	);
}

#[test]
fn test_extraction_not_blocked_by_existing_directory() {
	// Bug: [lib] path = "lib.rs" (no src/), existing tests/ dir for integration tests
	// Extracting `mod tests` from lib.rs should create tests.rs (sibling), NOT tests/mod.rs
	// The existence of tests/ directory should not force mod.rs form
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let tests_dir = tempdir.path().join("tests");
	fs::create_dir_all(&tests_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "test"
version = "0.1.0"
edition = "2021"

[lib]
path = "lib.rs"
"#,
	)
	.unwrap();

	// Existing integration test in tests/
	fs::write(tests_dir.join("integration.rs"), "#[test]\nfn it_works() {}\n").unwrap();

	// lib.rs with inline unit tests using super::*
	let lib_content = format!(
		"pub fn some_fn() {{}}\n\n#[cfg(test)]\nmod tests {{\n    use super::*;\n{}\n}}\n",
		(0..20)
			.map(|i| format!("    #[test]\n    fn test_{i}() {{ some_fn(); }}"))
			.collect::<Vec<_>>()
			.join("\n")
	);
	fs::write(tempdir.path().join("lib.rs"), &lib_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tempdir.path().join("lib.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction should succeed");

	// tests.rs as sibling is correct - it's just a module, not an integration test
	let good_tests_rs = tempdir.path().join("tests.rs");
	// tests/mod.rs is wrong - Cargo treats files in tests/ as integration tests
	let bad_tests_mod = tests_dir.join("mod.rs");

	assert!(
		!bad_tests_mod.exists(),
		"Should NOT create tests/mod.rs - files in tests/ are integration tests"
	);
	assert!(
		good_tests_rs.exists(),
		"Should create tests.rs as sibling - directory existence doesn't conflict"
	);

	// Verify the extracted content has use super::*
	let extracted = fs::read_to_string(&good_tests_rs).unwrap();
	assert!(extracted.contains("use super::*"), "Extracted module should preserve use super::*");
}

#[test]
fn test_extraction_within_special_dir_with_existing_subdir() {
	// Regression test: extraction within tests/ should work even when target subdir exists.
	// Previously broken because canonicalize() failed on non-existent dirs, masking the bug.
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let tests_dir = tempdir.path().join("tests");
	let helper_dir = tests_dir.join("helper");
	fs::create_dir_all(&helper_dir).unwrap();
	fs::create_dir_all(tempdir.path().join("src")).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "testcrate"
version = "0.1.0"
edition = "2021"
"#,
	)
	.unwrap();

	fs::write(tempdir.path().join("src").join("lib.rs"), "").unwrap();

	// Pre-existing file in helper/ subdir
	fs::write(helper_dir.join("utils.rs"), "pub fn util() {}\n").unwrap();

	// Integration test with large inline module
	let test_content = format!("mod helper {{\n{}\n}}\n\n#[test]\nfn it_works() {{}}\n", large_module_body(20));
	fs::write(tests_dir.join("integration.rs"), &test_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tests_dir.join("integration.rs").to_str().unwrap()]);

	assert!(result.success(), "Extraction within tests/ should succeed");

	// Should extract to tests/helper/mod.rs (subdir already exists)
	let extracted = tests_dir.join("helper").join("mod.rs");
	assert!(
		extracted.exists(),
		"Should create tests/helper/mod.rs even when helper/ already exists"
	);
}

#[test]
fn test_extraction_skips_when_output_lands_in_cargo_special_dir() {
	// Setup: lib.rs + tests.rs (module) + tests/integration.rs (integration test)
	// tests.rs has a large inline mod that would extract to tests/helpers.rs
	// But tests/ is Cargo's integration test directory - extraction must be skipped
	let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
	let tests_dir = tempdir.path().join("tests");
	fs::create_dir_all(&tests_dir).unwrap();

	fs::write(
		tempdir.path().join("Cargo.toml"),
		r#"[package]
name = "testcrate"
version = "0.1.0"
edition = "2021"

[lib]
path = "lib.rs"
"#,
	)
	.unwrap();

	// lib.rs references tests module
	fs::write(tempdir.path().join("lib.rs"), "#[cfg(test)]\nmod tests;\n").unwrap();

	// Existing integration test in tests/
	fs::write(tests_dir.join("integration.rs"), "#[test]\nfn integration_test() {}\n").unwrap();

	// tests.rs (unit test module) with large inline mod
	let tests_module_content = format!(
		"use super::*;\n\nmod helpers {{\n{}\n}}\n\n#[test]\nfn unit_test() {{}}\n",
		large_module_body(30)
	);
	fs::write(tempdir.path().join("tests.rs"), &tests_module_content).unwrap();

	let result = run_sort_items(&["--extract-threshold", "5", tempdir.path().join("tests.rs").to_str().unwrap()]);

	assert!(result.success(), "Should succeed (with warning)");

	// The module should NOT be extracted - output would be tests/helpers.rs
	// which Cargo treats as an integration test
	let bad_path = tests_dir.join("helpers.rs");
	assert!(
		!bad_path.exists(),
		"Should NOT create tests/helpers.rs - Cargo treats files in tests/ as integration tests"
	);

	// Module should remain inline
	let tests_after = fs::read_to_string(tempdir.path().join("tests.rs")).unwrap();
	assert!(
		tests_after.contains("mod helpers {"),
		"mod helpers should remain inline when extraction would land in Cargo special dir"
	);
}

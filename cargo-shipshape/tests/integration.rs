// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

mod cli_tests;

mod extraction_tests;

use std::fs;
use std::path::{Path, PathBuf};

/// Result from running cargo-shipshape
struct RunResult {
	exit_code: i32,
}

impl RunResult {
	fn success(&self) -> bool {
		self.exit_code == 0
	}
}

fn fixtures_dir() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn run_sort_items(args: &[&str]) -> RunResult {
	RunResult {
		exit_code: cargo_shipshape::run(args),
	}
}

mod fixture_tests {
	use super::*;

	macro_rules! fixture_test {
		($name:ident) => {
			#[test]
			fn $name() {
				let fixtures = fixtures_dir();
				let input_path = fixtures.join(concat!(stringify!($name), ".rs"));
				let expected_path = fixtures.join(concat!(stringify!($name), ".expected"));

				let input = fs::read_to_string(&input_path).expect(&format!("Failed to read input: {:?}", input_path));
				let expected = fs::read_to_string(&expected_path).expect(&format!("Failed to read expected: {:?}", expected_path));

				let tempdir = tempfile::tempdir().expect("Failed to create temp dir");
				let temp_file = tempdir.path().join("test.rs");
				fs::write(&temp_file, &input).expect("Failed to write temp file");

				let result = run_sort_items(&[temp_file.to_str().unwrap()]);

				// Allow failure for parse errors (checked by content comparison)
				let _ = result;

				let result_content = fs::read_to_string(&temp_file).expect("Failed to read result");

				if result_content != expected {
					eprintln!("=== INPUT ===\n{input}");
					eprintln!("=== EXPECTED ===\n{expected}");
					eprintln!("=== GOT ===\n{result_content}");
					eprintln!("=== DIFF ===");
					for diff in diff::lines(&expected, &result_content) {
						match diff {
							diff::Result::Left(l) => eprintln!("-{l}"),
							diff::Result::Right(r) => eprintln!("+{r}"),
							diff::Result::Both(b, _) => eprintln!(" {b}"),
						}
					}
					panic!("Fixture {} did not match expected output", stringify!($name));
				}
			}
		};
	}

	fixture_test!(all_item_types);
	fixture_test!(already_sorted);
	fixture_test!(async_functions);
	fixture_test!(attributes);
	fixture_test!(basic_sorting);
	fixture_test!(blank_line_preservation);
	fixture_test!(cfg_modules);
	fixture_test!(complex_impl);
	fixture_test!(const_generics);
	fixture_test!(doc_comments);
	fixture_test!(extern_block);
	fixture_test!(generics);
	fixture_test!(impl_adjacent_to_type);
	fixture_test!(impl_grouping);
	fixture_test!(inner_attributes);
	fixture_test!(license_header);
	fixture_test!(macro_call);
	fixture_test!(mixed_doc_comments);
	fixture_test!(mod_types);
	fixture_test!(multiline_items);
	fixture_test!(multiple_use_groups);
	fixture_test!(only_comments);
	fixture_test!(shebang);
	fixture_test!(trailing_whitespace);
	fixture_test!(tuple_structs);
	fixture_test!(use_order_preserved);
	fixture_test!(visibility);
	fixture_test!(where_clauses);
}

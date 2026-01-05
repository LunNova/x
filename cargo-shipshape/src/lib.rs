// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

pub mod crate_roots;
pub mod extract;
pub mod sort;

use anyhow::{Context, Result};
use argh::FromArgs;
use similar::TextDiff;
use std::borrow::Cow;
use std::path::PathBuf;

#[derive(FromArgs, Debug)]
/// Sort Rust file items by type and name
pub struct Args {
	/// check mode - exit 1 if files need sorting (for CI)
	#[argh(switch, short = 'c')]
	pub check: bool,

	/// show diff of what would change
	#[argh(switch)]
	pub diff: bool,

	/// don't write changes, just report
	#[argh(switch, short = 'n')]
	pub dry_run: bool,

	/// process all .rs files in directory recursively
	#[argh(switch, short = 'r')]
	pub recursive: bool,

	/// disable automatic extraction of large inline modules
	#[argh(switch)]
	pub no_extract: bool,

	/// line threshold for module extraction (default: 100)
	#[argh(option, default = "100")]
	pub extract_threshold: usize,

	/// files or directories to process (defaults to current directory)
	#[argh(positional)]
	pub paths: Vec<PathBuf>,
}

fn process_file(path: &std::path::Path, args: &Args) -> Result<bool> {
	let path = path
		.canonicalize()
		.with_context(|| format!("Failed to canonicalize {}", path.display()))?;
	let source = std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;

	let (working_source, extracted_files): (Cow<'_, str>, Vec<_>) = if args.no_extract {
		(Cow::Borrowed(&source), vec![])
	} else {
		let result = extract::extract_large_modules(&source, &path, args.extract_threshold)?;
		for warning in &result.warnings {
			eprintln!("Warning: {warning}");
		}
		(Cow::Owned(result.modified_source), result.extracted_files)
	};

	let sorted = sort::sort_items(&working_source)?;

	let has_changes = sorted != source || !extracted_files.is_empty();

	if !has_changes {
		return Ok(false);
	}

	if args.diff || args.dry_run {
		eprintln!("Would modify: {}", path.display());
		if args.diff {
			for change in TextDiff::from_lines(&source, &sorted).iter_all_changes() {
				print!("{}{change}", change.tag());
			}
		}
		for (extract_path, _) in &extracted_files {
			eprintln!("Would create: {}", extract_path.display());
		}
	}

	if !args.check && !args.dry_run {
		for (extract_path, content) in &extracted_files {
			let parent = extract_path.parent().expect("extract paths always have parent");
			std::fs::create_dir_all(parent).with_context(|| format!("Failed to create directory {}", parent.display()))?;
			std::fs::write(extract_path, content).with_context(|| format!("Failed to write {}", extract_path.display()))?;
			eprintln!("Extracted: {}", extract_path.display());
		}

		std::fs::write(&path, &sorted).with_context(|| format!("Failed to write {}", path.display()))?;
		eprintln!("Sorted: {}", path.display());
	}

	Ok(true)
}

/// Run the cargo-shipshape tool with the given command-line arguments.
pub fn run(args: &[&str]) -> i32 {
	let parsed = match Args::from_args(&["cargo-shipshape"], args) {
		Ok(args) => args,
		Err(early_exit) => {
			println!("{}", early_exit.output);
			return i32::from(early_exit.status.is_err());
		}
	};

	match run_with_args(&parsed) {
		Ok(code) => code,
		Err(err) => {
			eprintln!("Error: {err:?}");
			1
		}
	}
}

/// Run the cargo-shipshape tool with parsed arguments.
pub fn run_with_args(args: &Args) -> Result<i32> {
	let paths = if args.paths.is_empty() {
		vec![PathBuf::from(".")]
	} else {
		args.paths.clone()
	};

	let mut any_changes = false;
	let mut files_processed = 0;

	for path in paths {
		if args.recursive && path.is_dir() {
			for entry in walkdir::WalkDir::new(&path)
				.into_iter()
				.filter_map(std::result::Result::ok)
				.filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
			{
				let changed = process_file(entry.path(), args)?;
				any_changes |= changed;
				files_processed += 1;
			}
		} else if path.is_file() {
			let changed = process_file(&path, args)?;
			any_changes |= changed;
			files_processed += 1;
		} else if path.is_dir() {
			eprintln!("Skipping directory {} (use --recursive to process directories)", path.display());
		} else {
			eprintln!("Path does not exist: {}", path.display());
		}
	}

	if files_processed == 0 {
		eprintln!("No .rs files found to process");
		return Ok(1);
	}

	if args.check && any_changes {
		eprintln!("{files_processed} file(s) need sorting");
		Ok(1)
	} else {
		Ok(0)
	}
}

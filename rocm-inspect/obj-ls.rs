// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! rocm-obj-ls: List ROCm/HIP code objects in binaries.
//!
//! Spiritual successor to the deprecated roc-obj-ls.

use argh::FromArgs;
use owo_colors::{OwoColorize, Stream};
use rocm_inspect::CodeObject;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

#[derive(FromArgs)]
/// List ROCm/HIP code objects and ISAs in binaries
struct Args {
	#[argh(positional)]
	/// paths to files to analyze (.so, .hsaco, .o, executables)
	files: Vec<PathBuf>,

	#[argh(switch, short = 'v')]
	/// verbose output with additional details
	verbose: bool,
}

fn main() {
	let args: Args = argh::from_env();

	if args.files.is_empty() {
		eprintln!("Error: No files provided");
		std::process::exit(1);
	}

	let use_color = io::stdout().is_terminal();
	let single_file = args.files.len() == 1;
	let mut all_objects = Vec::new();

	for path in &args.files {
		match rocm_inspect::analyze_file(path) {
			Ok(mut objects) => {
				for obj in &mut objects {
					obj.source_file = path.display().to_string();
				}
				all_objects.extend(objects);
			}
			Err(e) => {
				eprintln!("Error analyzing {}: {}", path.display(), e);
			}
		}
	}

	if all_objects.is_empty() {
		eprintln!("No AMDGPU code objects found");
		return;
	}

	print_results(&all_objects, use_color, single_file, args.verbose);

	// use_color is a proxy for terminal detection - avoid polluting piped/redirected output
	if use_color {
		print_summary(&all_objects);
	}
}

fn print_results(objects: &[CodeObject], use_color: bool, single_file: bool, verbose: bool) {
	let (max_isa_len, max_features_len, max_file_len) = objects.iter().fold((0, 0, 0), |(isa, feat, file), o| {
		(isa.max(o.isa.len()), feat.max(o.features.len()), file.max(o.source_file.len()))
	});

	let isa_width = max_isa_len.max(3);
	let features_width = max_features_len.max(8);
	let file_width = max_file_len.max(4);

	// For single file, show file as header instead of column
	if single_file && !objects.is_empty() {
		eprintln!("{}", objects[0].source_file);
	}

	if single_file {
		if use_color {
			println!(
				"{:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
				"ISA".if_supports_color(Stream::Stdout, |t| t.bold()),
				"FEATURES".if_supports_color(Stream::Stdout, |t| t.bold()),
				"SIZE".if_supports_color(Stream::Stdout, |t| t.bold()),
				"KERNELS".if_supports_color(Stream::Stdout, |t| t.bold()),
				"BUNDLE_ID".if_supports_color(Stream::Stdout, |t| t.bold()),
			);
		} else {
			println!(
				"{:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  BUNDLE_ID",
				"ISA", "FEATURES", "SIZE", "KERNELS",
			);
		}
	} else if use_color {
		println!(
			"{:<file_width$}  {:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
			"FILE".if_supports_color(Stream::Stdout, |t| t.bold()),
			"ISA".if_supports_color(Stream::Stdout, |t| t.bold()),
			"FEATURES".if_supports_color(Stream::Stdout, |t| t.bold()),
			"SIZE".if_supports_color(Stream::Stdout, |t| t.bold()),
			"KERNELS".if_supports_color(Stream::Stdout, |t| t.bold()),
			"BUNDLE_ID".if_supports_color(Stream::Stdout, |t| t.bold()),
		);
	} else {
		println!(
			"{:<file_width$}  {:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  BUNDLE_ID",
			"FILE", "ISA", "FEATURES", "SIZE", "KERNELS",
		);
	}

	for obj in objects {
		let size_str = format_size(obj.size);
		let bundle_id = obj.bundle_entry_id.as_deref().unwrap_or("-");
		let kernel_count = obj.kernel_names.len();

		if single_file {
			if use_color {
				println!(
					"{:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
					obj.isa.if_supports_color(Stream::Stdout, |t| t.green()),
					obj.features,
					size_str.if_supports_color(Stream::Stdout, |t| t.yellow()),
					kernel_count,
					bundle_id.if_supports_color(Stream::Stdout, |t| t.dimmed()),
				);
			} else {
				println!(
					"{:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
					obj.isa, obj.features, size_str, kernel_count, bundle_id,
				);
			}
		} else if use_color {
			println!(
				"{:<file_width$}  {:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
				obj.source_file.if_supports_color(Stream::Stdout, |t| t.cyan()),
				obj.isa.if_supports_color(Stream::Stdout, |t| t.green()),
				obj.features,
				size_str.if_supports_color(Stream::Stdout, |t| t.yellow()),
				kernel_count,
				bundle_id.if_supports_color(Stream::Stdout, |t| t.dimmed()),
			);
		} else {
			println!(
				"{:<file_width$}  {:<isa_width$}  {:<features_width$}  {:>10}  {:>7}  {}",
				obj.source_file, obj.isa, obj.features, size_str, kernel_count, bundle_id,
			);
		}

		if verbose && !obj.kernel_names.is_empty() {
			for kernel_name in &obj.kernel_names {
				if use_color {
					println!("    {}", kernel_name.if_supports_color(Stream::Stdout, |t| t.blue()));
				} else {
					println!("    {kernel_name}");
				}
			}
		}
	}
}

fn print_summary(objects: &[CodeObject]) {
	use std::collections::BTreeSet;

	let unique_isas: BTreeSet<_> = objects.iter().map(|o| &o.isa).collect();

	eprintln!();
	eprintln!("Found {} code objects across {} unique ISAs", objects.len(), unique_isas.len());

	eprint!("ISAs: ");
	for (i, isa) in unique_isas.iter().enumerate() {
		if i > 0 {
			eprint!(", ");
		}
		eprint!("{isa}");
	}
	eprintln!();
}

fn format_size(bytes: u64) -> String {
	const KB: u64 = 1024;
	const MB: u64 = KB * 1024;
	const GB: u64 = MB * 1024;

	if bytes >= GB {
		format!("{:.1}G", bytes as f64 / GB as f64)
	} else if bytes >= MB {
		format!("{:.1}M", bytes as f64 / MB as f64)
	} else if bytes >= KB {
		format!("{:.1}K", bytes as f64 / KB as f64)
	} else {
		format!("{bytes}B")
	}
}

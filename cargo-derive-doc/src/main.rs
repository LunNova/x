// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

// FIXME: This entire file needs reviewed and cleaned up

use anyhow::Result;
use argh::FromArgs;
use quote::ToTokens;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use syn::{Attribute, File, Item, parse_file, spanned::Spanned};

const CARGO_DERIVE_DOC_WRAPPER: &str = "CARGO_DERIVE_DOC_WRAPPER";

#[derive(FromArgs, PartialEq, Debug)]
/// Add or update macro expansion documentation comments
#[argh(subcommand, name = "derive-doc")]
struct DeriveDoc {
	/// process a specific package in the workspace
	#[argh(option, short = 'p')]
	package: Option<String>,

	/// don't write changes, just show what would be done
	#[argh(switch, short = 'n')]
	dry_run: bool,

	/// include examples
	#[argh(switch)]
	examples: bool,

	/// include all targets (lib, bins, examples, tests, benches)
	#[argh(switch)]
	all_targets: bool,

	/// only process files in this directory (e.g., "examples")
	#[argh(option)]
	dir_filter: Option<String>,
}

fn main() {
	let result = if let Some(wrapper) = env::var_os(CARGO_DERIVE_DOC_WRAPPER) {
		do_rustc_wrapper(&wrapper)
	} else {
		do_cargo_derive_doc()
	};

	process::exit(match result {
		Ok(code) => code,
		Err(err) => {
			eprintln!("Error: {err}");
			1
		}
	});
}

fn do_rustc_wrapper(original_wrapper: &OsStr) -> Result<i32> {
	// We're being called as rustc wrapper
	let mut args = env::args_os().skip(1);

	let mut cmd = if original_wrapper != "/" {
		Command::new(original_wrapper)
	} else if let Some(rustc) = args.next() {
		Command::new(rustc)
	} else {
		return Ok(1);
	};

	// Check if this is the crate we want to process
	let args: Vec<_> = args.collect();
	let should_expand = should_process_crate(&args);

	if should_expand {
		// Run with -Zunpretty=expanded to get expansion
		let mut expand_cmd = Command::new(cmd.get_program());
		expand_cmd.args(&args);
		expand_cmd.arg("-Zunpretty=expanded");
		expand_cmd.env("RUSTC_BOOTSTRAP", "1");
		expand_cmd.stdout(Stdio::piped());

		let output = expand_cmd.output()?;
		if output.status.success() {
			let expanded = String::from_utf8_lossy(&output.stdout);

			// Find the source file being compiled
			if let Some(source_file) = find_source_file(&args) {
				process_expansion(&source_file, &expanded)?;
			}
		}
	}

	// Run the original compilation
	cmd.args(&args);
	let status = cmd.status()?;
	Ok(status.code().unwrap_or(1))
}

fn should_process_crate(args: &[OsString]) -> bool {
	// Only process files in our current workspace, not dependencies
	if let Some(source_file) = find_source_file(args) {
		let path_str = source_file.to_string_lossy();

		// Check if path contains registry (dependency)
		if path_str.contains("/.cargo/registry/") || path_str.contains("\\.cargo\\registry\\") {
			return false;
		}

		// Check directory filter if set
		if let Ok(dir_filter) = env::var("CARGO_DERIVE_DOC_DIR_FILTER")
			&& !path_str.contains(&dir_filter)
		{
			return false;
		}

		// Check for specific target crate if set
		if let Ok(target_crate) = env::var("CARGO_DERIVE_DOC_TARGET") {
			for (i, arg) in args.iter().enumerate() {
				if arg == "--crate-name"
					&& let Some(crate_name) = args.get(i + 1)
				{
					let matches = crate_name == target_crate.as_str();
					return matches;
				}
			}
			return false;
		}

		return true;
	}
	false
}

fn find_source_file(args: &[OsString]) -> Option<PathBuf> {
	// The source file is usually the last argument that ends with .rs
	args.iter()
		.filter_map(|arg| arg.to_str())
		.filter(|arg| arg.ends_with(".rs") && !arg.starts_with("--"))
		.next_back()
		.map(PathBuf::from)
}

fn process_expansion(source_file: &Path, expanded: &str) -> Result<()> {
	let dry_run = env::var("CARGO_DERIVE_DOC_DRY_RUN").is_ok();

	eprintln!("Processing {}", source_file.display());

	// Parse original and expanded to match up items
	let original_content = std::fs::read_to_string(source_file)?;
	let original_ast = parse_file(&original_content)?;
	let expanded_ast = parse_file(expanded)?;

	// Use diff-based matching for macro expansions
	let diff_expansions = match_expansions_with_diff(&original_content, expanded)?;

	// Find traditional derive expansions
	let derive_expansions = match_expansions(&original_ast, &expanded_ast)?;

	// Combine both approaches
	let mut all_expansions = derive_expansions;
	all_expansions.extend(diff_expansions);

	// Inject comments into the source text
	let (updated_content, removed_comments) = inject_comments(&original_content, &original_ast, &all_expansions)?;

	// Update file if we have new expansions or removed old comments
	if !all_expansions.is_empty() || removed_comments {
		if dry_run {
			println!("Would update {}:", source_file.display());
			println!("{updated_content}");
		} else {
			std::fs::write(source_file, updated_content)?;
			eprintln!("✓ Updated {}", source_file.display());
		}
	} else {
		eprintln!("No macro expansions found in {}", source_file.display());
	}

	Ok(())
}

fn match_expansions(original: &File, expanded: &File) -> Result<HashMap<String, Vec<String>>> {
	let mut expansions = HashMap::new();

	// Still handle derive macros the old way
	let derive_items = find_derive_items(original);

	// Find all new items (anything in expanded that wasn't in original)
	let mut new_items = Vec::new();
	for item in &expanded.items {
		if !contains_item(original, item)
			&& let Some(sig) = item_signature(item)
		{
			// Filter out obvious derive implementations here too
			if !is_obvious_derive_impl(&sig) {
				new_items.push(sig);
			}
		}
	}

	eprintln!("Found {} derive items", derive_items.len());
	eprintln!("Found {} new items from macro expansion", new_items.len());

	// For each derive item, find items that look related (existing logic)
	for (item_name, derives) in derive_items {
		let mut related_items = Vec::new();

		for new_item in &new_items {
			if new_item.contains(&item_name) || item_impl_for_name(&item_name, new_item) {
				related_items.push(new_item.clone());
			}
		}

		if !related_items.is_empty() {
			eprintln!("Matched {} ({:?}) with {} expansions", item_name, derives, related_items.len());
			expansions.insert(item_name, related_items);
		}
	}

	Ok(expansions)
}

fn match_expansions_with_diff(original: &str, expanded: &str) -> Result<HashMap<String, Vec<String>>> {
	let mut expansions = HashMap::new();

	// Create a diff between original and expanded
	let diff = TextDiff::from_lines(original, expanded);

	// Track macro calls that were removed and what was added nearby
	let mut removed_ranges = Vec::new();
	let mut added_ranges = Vec::new();

	let mut old_line = 0;
	let mut new_line = 0;

	for change in diff.iter_all_changes() {
		match change.tag() {
			ChangeTag::Delete => {
				let line_content = change.value().trim();
				// Look for macro calls being removed
				if line_content.contains("!") && (line_content.contains("{") || line_content.ends_with(";")) {
					// This looks like a macro call - record the line range
					removed_ranges.push((old_line, line_content.to_string()));
				}
				old_line += 1;
			}
			ChangeTag::Insert => {
				// Record what's being added
				let line_content = change.value().trim();
				if !line_content.is_empty() && !line_content.starts_with("//") {
					added_ranges.push((new_line, line_content.to_string()));
				}
				new_line += 1;
			}
			ChangeTag::Equal => {
				old_line += 1;
				new_line += 1;
			}
		}
	}

	eprintln!("Found {} removed macro calls", removed_ranges.len());
	eprintln!("Found {} added lines", added_ranges.len());

	// FIXME: This currently associates ALL generated items with EVERY macro call,
	// which causes duplicates when there are multiple macros. We need better
	// proximity-based matching to associate specific generated items with
	// specific macro calls based on line positions in the diff.
	for (removed_line, removed_content) in removed_ranges {
		if let Some(macro_name) = extract_macro_name(&removed_content) {
			eprintln!("Found macro call: {} at line {}", macro_name, removed_line + 1);

			// Parse both original and expanded to find only NEW items
			let original_ast = parse_file(original)?;
			let expanded_ast = parse_file(expanded)?;
			let mut generated_items = Vec::new();

			for item in &expanded_ast.items {
				if !contains_item(&original_ast, item)
					&& let Some(sig) = item_signature(item)
				{
					// Filter out common derive trait implementations that are obvious
					if !is_obvious_derive_impl(&sig) {
						generated_items.push(sig);
					}
				}
			}

			// For now, associate all generated items with this macro
			// (we could be more sophisticated about proximity later)
			if !generated_items.is_empty() {
				let key = format!("{macro_name}!");
				eprintln!("Matched macro {}! with {} expansions", macro_name, generated_items.len());
				expansions.insert(key, generated_items);
			}
		}
	}

	Ok(expansions)
}

fn extract_macro_name(line: &str) -> Option<String> {
	// Only match macros that start at the very beginning of the line (zero indentation)
	if !line.starts_with(|c: char| c.is_alphabetic()) {
		return None;
	}

	// Only consider lines that start with the macro name
	if let Some(bang_pos) = line.find('!') {
		let before_bang = &line[..bang_pos];
		// Check if it looks like a top-level macro invocation
		if before_bang.chars().all(|c| c.is_alphanumeric() || c == '_') && !before_bang.is_empty() {
			return Some(before_bang.to_string());
		}
	}
	None
}

fn is_obvious_derive_impl(signature: &str) -> bool {
	// Filter out implementations of well-known derive traits that are obvious
	signature.contains("::core::fmt::Debug for")
		|| signature.contains("::core::clone::Clone for")
		|| signature.contains("::core::marker::Copy for")
		|| signature.contains("::core::cmp::PartialEq for")
		|| signature.contains("::core::cmp::Eq for")
		|| signature.contains("::core::cmp::PartialOrd for")
		|| signature.contains("::core::cmp::Ord for")
		|| signature.contains("::core::hash::Hash for")
		|| signature.contains("::core::default::Default for")
		|| signature.contains("StructuralPartialEq for")
		|| signature.contains("StructuralEq for")
}

fn find_derive_items(file: &File) -> Vec<(String, Vec<String>)> {
	let mut items = Vec::new();

	for item in &file.items {
		match item {
			Item::Struct(s) => {
				if let Some(derives) = get_derives(&s.attrs) {
					items.push((s.ident.to_string(), derives));
				}
			}
			Item::Enum(e) => {
				if let Some(derives) = get_derives(&e.attrs) {
					items.push((e.ident.to_string(), derives));
				}
			}
			_ => {}
		}
	}

	items
}

fn get_derives(attrs: &[Attribute]) -> Option<Vec<String>> {
	for attr in attrs {
		if attr.path().is_ident("derive") {
			// Parse the derive attribute
			if let Ok(derives) = attr.parse_args_with(|input: syn::parse::ParseStream| {
				let mut derives = Vec::new();
				while !input.is_empty() {
					let path: syn::Path = input.parse()?;
					derives.push(quote::quote!(#path).to_string());
					if !input.is_empty() {
						input.parse::<syn::Token![,]>()?;
					}
				}
				Ok(derives)
			}) {
				return Some(derives);
			}
		}
	}
	None
}

fn contains_item(file: &File, item: &Item) -> bool {
	// Simple check - in real implementation would be more sophisticated
	match item {
		Item::Struct(s) => file.items.iter().any(|i| matches!(i, Item::Struct(s2) if s2.ident == s.ident)),
		Item::Enum(e) => file.items.iter().any(|i| matches!(i, Item::Enum(e2) if e2.ident == e.ident)),
		Item::Fn(f) => file.items.iter().any(|i| matches!(i, Item::Fn(f2) if f2.sig.ident == f.sig.ident)),
		Item::Type(t) => file.items.iter().any(|i| matches!(i, Item::Type(t2) if t2.ident == t.ident)),
		Item::Impl(impl_item) => {
			// Check if there's a matching impl block in the original
			file.items.iter().any(|i| {
				if let Item::Impl(original_impl) = i {
					// Compare trait and self type to see if it's the same impl
					impl_item.trait_ == original_impl.trait_
						&& impl_item.self_ty.to_token_stream().to_string() == original_impl.self_ty.to_token_stream().to_string()
				} else {
					false
				}
			})
		}
		_ => false,
	}
}

fn item_signature(item: &Item) -> Option<String> {
	fn clean_token_stream(s: String) -> String {
		// clean up :: spacing
		s.replace(" :: ", "::").replace("< ", "<").replace(" >", ">").replace("  ", "")
	}

	match item {
		Item::Struct(s) => Some(clean_token_stream(format!(
			"{} struct {}{}",
			s.vis.to_token_stream(),
			s.ident,
			s.generics.to_token_stream()
		))),
		Item::Enum(e) => Some(clean_token_stream(format!(
			"{} enum {}{}",
			e.vis.to_token_stream(),
			e.ident,
			e.generics.to_token_stream()
		))),
		Item::Fn(f) => Some(clean_token_stream(format!(
			"{} {}",
			f.vis.to_token_stream(),
			f.sig.to_token_stream()
		))),
		Item::Type(t) => Some(clean_token_stream(format!(
			"{} type {}{} = {}",
			t.vis.to_token_stream(),
			t.ident,
			t.generics.to_token_stream(),
			t.ty.to_token_stream()
		))),
		Item::Const(c) => Some(clean_token_stream(format!(
			"{} const {}: {}",
			c.vis.to_token_stream(),
			c.ident,
			c.ty.to_token_stream()
		))),
		Item::Impl(i) => {
			let trait_part = if let Some((_, path, _)) = &i.trait_ {
				format!("{} for ", path.to_token_stream())
			} else {
				String::new()
			};
			Some(format!(
				"impl {}",
				clean_token_stream(format!(
					"{} {}{}",
					i.generics.to_token_stream(),
					trait_part,
					i.self_ty.to_token_stream()
				))
			))
		}
		_ => None,
	}
}

fn item_impl_for_name(type_name: &str, item_signature: &str) -> bool {
	// Check if this signature looks like an impl for our type
	item_signature.contains(&"impl".to_string()) && item_signature.contains(type_name)
}

fn inject_comments(source: &str, _ast: &File, expansions: &HashMap<String, Vec<String>>) -> Result<(String, bool)> {
	// First, remove any existing generated comments
	let (cleaned_source, removed_comments) = remove_existing_comments(source);

	// Parse the cleaned source to get correct line numbers
	let cleaned_ast = parse_file(&cleaned_source)?;

	let mut injection_points = Vec::new();

	// Find where to inject comments for each item with derives (existing logic)
	for item in &cleaned_ast.items {
		match item {
			Item::Struct(s) => {
				if let Some(expansion_items) = expansions.get(&s.ident.to_string())
					&& let Some(derive_attr) = find_derive_attr(&s.attrs)
				{
					let span = derive_attr.span();
					if let Some(line) = get_line_number(span) {
						injection_points.push((line, expansion_items.clone()));
					}
				}
			}
			Item::Enum(e) => {
				if let Some(expansion_items) = expansions.get(&e.ident.to_string())
					&& let Some(derive_attr) = find_derive_attr(&e.attrs)
				{
					let span = derive_attr.span();
					if let Some(line) = get_line_number(span) {
						injection_points.push((line, expansion_items.clone()));
					}
				}
			}
			_ => {}
		}
	}

	// Find macro calls in the source and add injection points for them
	let lines: Vec<&str> = cleaned_source.lines().collect();
	for (line_idx, line) in lines.iter().enumerate() {
		if let Some(macro_name) = extract_macro_name(line) {
			let key = format!("{macro_name}!");
			if let Some(expansion_items) = expansions.get(&key) {
				// Inject before the macro call
				injection_points.push((line_idx + 1, expansion_items.clone()));
			}
		}
	}

	// Sort by line number (descending so we inject from bottom to top)
	injection_points.sort_by(|a, b| b.0.cmp(&a.0));

	let mut lines: Vec<String> = cleaned_source.lines().map(|s| s.to_string()).collect();

	// Inject comments
	for (line_num, items) in injection_points {
		if line_num > 0 && line_num <= lines.len() {
			let comment = format_expansion_comment(&items);
			lines.insert(line_num - 1, comment);
		}
	}

	Ok((lines.join("\n"), removed_comments))
}

fn remove_existing_comments(source: &str) -> (String, bool) {
	let lines: Vec<&str> = source.lines().collect();
	let mut result_lines = Vec::new();
	let mut i = 0;
	let mut removed_any = false;

	while i < lines.len() {
		let line = lines[i];

		// Check if this line starts a generated comment block
		if line.trim() == "// <generated by cargo-derive-doc>" {
			removed_any = true;
			// Skip lines until we find the end marker
			i += 1;
			while i < lines.len() {
				if lines[i].trim() == "// </generated by cargo-derive-doc>" {
					i += 1; // Skip the end marker too
					break;
				}
				i += 1;
			}
		} else {
			result_lines.push(line);
			i += 1;
		}
	}

	(result_lines.join("\n"), removed_any)
}

fn find_derive_attr(attrs: &[Attribute]) -> Option<&Attribute> {
	attrs.iter().find(|attr| attr.path().is_ident("derive"))
}

fn get_line_number(span: proc_macro2::Span) -> Option<usize> {
	// Use span-locations feature to get line info
	let start = span.start();
	Some(start.line)
}

fn format_expansion_comment(items: &[String]) -> String {
	let mut comment = String::from("// <generated by cargo-derive-doc>");
	comment.push_str("\n// Macro expansions:");
	for item in items {
		comment.push_str(&format!("\n//   {item}"));
	}
	comment.push_str("\n// </generated by cargo-derive-doc>");
	comment
}

fn do_cargo_derive_doc() -> Result<i32> {
	// When invoked as 'cargo derive-doc', we get: ["cargo-derive-doc", "derive-doc", ...]
	let args: Vec<String> = env::args().collect();

	// Skip the "derive-doc" that cargo passes
	let parse_result = if args.len() > 1 && args[1] == "derive-doc" {
		// Running as cargo subcommand
		DeriveDoc::from_args(&["cargo-derive-doc"], &args[2..].iter().map(|s| s.as_str()).collect::<Vec<_>>())
	} else {
		// Running directly
		DeriveDoc::from_args(&["cargo-derive-doc"], &args[1..].iter().map(|s| s.as_str()).collect::<Vec<_>>())
	};

	match parse_result {
		Ok(cmd) => run_derive_doc(cmd),
		Err(early_exit) => {
			println!("{}", early_exit.output);
			process::exit(match early_exit.status {
				Ok(()) => 0,
				Err(()) => 1,
			})
		}
	}
}

fn run_derive_doc(args: DeriveDoc) -> Result<i32> {
	// Set up environment for the wrapper
	let current_exe = env::current_exe()?;
	let original_wrapper = env::var_os("RUSTC_WRAPPER").unwrap_or_else(|| OsString::from("/"));

	if let Some(package) = &args.package {
		unsafe {
			env::set_var("CARGO_DERIVE_DOC_TARGET", package);
		}
	}

	if args.dry_run {
		unsafe {
			env::set_var("CARGO_DERIVE_DOC_DRY_RUN", "1");
		}
	}

	if let Some(dir_filter) = &args.dir_filter {
		unsafe {
			env::set_var("CARGO_DERIVE_DOC_DIR_FILTER", dir_filter);
		}
	}

	// Run cargo check with our wrapper
	let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")));
	cmd.arg("check");

	if let Some(package) = &args.package {
		cmd.arg("--package").arg(package);
	}

	if args.examples {
		cmd.arg("--examples");
	}

	if args.all_targets {
		cmd.arg("--all-targets");
	}

	cmd.env(CARGO_DERIVE_DOC_WRAPPER, original_wrapper);
	cmd.env("RUSTC_WRAPPER", current_exe);

	let status = cmd.status()?;
	Ok(status.code().unwrap_or(1))
}

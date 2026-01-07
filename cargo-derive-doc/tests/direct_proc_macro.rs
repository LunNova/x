// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Direct proc-macro expansion test.
//!
//! This test bypasses rust-analyzer's full workspace loading and instead:
//! 1. Spawns the proc-macro-srv process directly
//! 2. Loads the compiled proc-macro dylib
//! 3. Converts source tokens to RA's token tree format
//! 4. Calls expand() directly
//! 5. Parses the output to extract generated items

use ra_ap_paths::AbsPathBuf;
use ra_ap_proc_macro_api::{MacroDylib, ProcMacroClient, ProcMacroKind};
use ra_ap_span::{Edition, EditionedFileId, FileId, Span, SyntaxContext, TextRange, TextSize};
use ra_ap_syntax::{AstNode, SourceFile};
use ra_ap_syntax_bridge::{DocCommentDesugarMode, dummy_test_span_utils::DummyTestSpanMap};
use std::path::Path;
use std::process::Command;

/// Find the proc-macro-srv binary from the sysroot
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

/// Find the compiled proc-macro dylib for error_set
fn find_error_set_dylib() -> Option<AbsPathBuf> {
	let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/deps");

	// error_set's proc-macro is in error_set_impl crate
	let mut candidates: Vec<_> = std::fs::read_dir(&target_dir)
		.ok()?
		.filter_map(|e| e.ok())
		.filter_map(|e| {
			let path = e.path();
			let name = path.file_name()?.to_str()?;
			if name.starts_with("liberror_set_impl") && name.ends_with(".so") {
				let metadata = std::fs::metadata(&path).ok()?;
				Some((path, metadata.modified().ok()?))
			} else {
				None
			}
		})
		.collect();

	// Sort by modification time (newest first)
	candidates.sort_by(|a, b| b.1.cmp(&a.1));

	if let Some((path, _)) = candidates.first() {
		eprintln!("Found error_set_impl dylib: {}", path.display());
		return Some(AbsPathBuf::assert(path.to_str().unwrap().into()));
	}
	None
}

/// Create a dummy span for testing
fn dummy_span() -> Span {
	Span {
		range: TextRange::empty(TextSize::new(0)),
		anchor: ra_ap_span::SpanAnchor {
			file_id: EditionedFileId::new(FileId::from_raw(0xe4e4e), Edition::CURRENT),
			ast_id: ra_ap_span::ROOT_ERASED_FILE_AST_ID,
		},
		ctx: SyntaxContext::root(Edition::CURRENT),
	}
}

/// Extract items from expanded token tree text
fn extract_items_from_text(text: &str) -> Vec<String> {
	let mut items = Vec::new();

	let parse = syn::parse_file(text);
	match parse {
		Ok(file) => {
			for item in file.items {
				match item {
					syn::Item::Struct(s) => {
						items.push(format!("struct {}", s.ident));
					}
					syn::Item::Enum(e) => {
						items.push(format!("enum {}", e.ident));
					}
					syn::Item::Impl(i) => {
						if let Some((_, trait_path, _)) = &i.trait_ {
							let trait_name = trait_path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
							let self_ty = quote::ToTokens::to_token_stream(&i.self_ty).to_string();
							items.push(format!("impl {} for {}", trait_name, self_ty));
						} else {
							let self_ty = quote::ToTokens::to_token_stream(&i.self_ty).to_string();
							items.push(format!("impl {}", self_ty));
						}
					}
					syn::Item::Fn(f) => {
						items.push(format!("fn {}", f.sig.ident));
					}
					syn::Item::Type(t) => {
						items.push(format!("type {}", t.ident));
					}
					_ => {}
				}
			}
		}
		Err(e) => {
			eprintln!("Failed to parse expanded output: {}", e);
			eprintln!("Text was: {}", text);
		}
	}

	items
}

#[test]
fn test_direct_proc_macro_expansion() {
	let start = std::time::Instant::now();

	// Find proc-macro-srv
	let srv_path = match find_proc_macro_srv() {
		Some(p) => p,
		None => {
			eprintln!("Skipping test: proc-macro-srv not found");
			return;
		}
	};
	eprintln!("[{:?}] Found proc-macro-srv", start.elapsed());

	// Find error_set dylib
	let dylib_path = match find_error_set_dylib() {
		Some(p) => p,
		None => {
			eprintln!("Skipping test: error_set dylib not found. Run `cargo build` first.");
			return;
		}
	};
	eprintln!("[{:?}] Found error_set dylib", start.elapsed());

	// Spawn proc-macro client
	let env: Vec<(String, &Option<String>)> = vec![];
	let client = match ProcMacroClient::spawn(&srv_path, env, None) {
		Ok(c) => c,
		Err(e) => {
			eprintln!("Failed to spawn proc-macro client: {}", e);
			return;
		}
	};
	eprintln!("[{:?}] Spawned proc-macro client", start.elapsed());

	// Load the dylib
	let macros = match client.load_dylib(MacroDylib::new(dylib_path), None) {
		Ok(m) => m,
		Err(e) => {
			eprintln!("Failed to load dylib: {}", e);
			return;
		}
	};
	eprintln!("[{:?}] Loaded dylib, found {} macros", start.elapsed(), macros.len());

	for m in &macros {
		eprintln!("  - {} ({:?})", m.name(), m.kind());
	}

	// Find the error_set! macro (it's a Bang macro)
	let error_set_macro = macros.iter().find(|m| m.name() == "error_set" && m.kind() == ProcMacroKind::Bang);

	let error_set_macro = match error_set_macro {
		Some(m) => m,
		None => {
			eprintln!("error_set! macro not found in dylib");
			return;
		}
	};
	eprintln!("[{:?}] Found error_set! macro", start.elapsed());

	// Now let's expand a sample macro invocation
	// The error_set! macro uses := for set declarations

	let sample_input = r#"
        TestError := {
            IoError(std::io::Error),
            ParseError,
            NetworkError(String),
        }
    "#;

	// Parse the input with RA's parser
	let parse = SourceFile::parse(sample_input, Edition::CURRENT);
	let syntax = parse.tree().syntax().clone();

	// Convert to token tree
	let span = dummy_span();
	let tt_input = ra_ap_syntax_bridge::syntax_node_to_token_tree(&syntax, DummyTestSpanMap, span, DocCommentDesugarMode::ProcMacro);

	eprintln!("[{:?}] Converted input to token tree", start.elapsed());
	eprintln!("Input tokens: {}", tt_input);

	// Call the macro
	let expand_start = std::time::Instant::now();
	let result = error_set_macro.expand(
		tt_input.view(),
		None, // no attr for Bang macros
		vec![],
		span, // def_site
		span, // call_site
		span, // mixed_site
		env!("CARGO_MANIFEST_DIR").to_string(),
		None,
	);
	eprintln!("[{:?}] Expansion took {:?}", start.elapsed(), expand_start.elapsed());

	match result {
		Ok(Ok(expanded)) => {
			let expanded_text = expanded.to_string();
			eprintln!("\n=== Expanded output ({} chars) ===", expanded_text.len());
			eprintln!("{}", &expanded_text[..expanded_text.len().min(2000)]);
			if expanded_text.len() > 2000 {
				eprintln!("... (truncated)");
			}

			// Extract items
			let items = extract_items_from_text(&expanded_text);
			eprintln!("\n=== Generated items ===");
			for item in &items {
				eprintln!("  {}", item);
			}

			assert!(!items.is_empty(), "Should have generated some items");
			assert!(items.iter().any(|i| i.starts_with("enum")), "Should have generated an enum");
		}
		Ok(Err(e)) => {
			eprintln!("Macro expansion error: {}", e);
		}
		Err(e) => {
			eprintln!("Server error: {}", e);
		}
	}

	eprintln!("\n[{:?}] Total test time", start.elapsed());
}

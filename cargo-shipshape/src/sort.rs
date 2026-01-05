// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use anyhow::Result;
use ra_ap_syntax::ast::HasModuleItem;
use ra_ap_syntax::ast::HasName;
use ra_ap_syntax::{AstNode, Edition, SourceFile, SyntaxNode, ast};

struct Item<'a>(ItemSort<'a>, &'a str);

#[derive(PartialEq, PartialOrd, Eq, Ord)]
enum TypeDefKind<'a> {
	Struct,
	Enum,
	Union,
	Impl(Option<&'a str>), // trait name for trait impls
}

#[derive(PartialEq, PartialOrd, Eq, Ord)]
enum ItemSort<'a> {
	ExternCrate(&'a str),
	Mod(&'a str),
	Use,
	Const(&'a str),
	Static(&'a str),
	TypeAlias(&'a str),
	MacroRules(&'a str),
	MacroCall(&'a str),
	Trait(&'a str),
	// Sorts by type name first, keeping impls with their types
	TypeDef(&'a str, TypeDefKind<'a>),
	Fn(&'a str),
	BlockMod(&'a str),
}

fn classify<'a>(source: &'a str, item: &ast::Item) -> Result<ItemSort<'a>> {
	use ItemSort::*;
	Ok(match item {
		ast::Item::ExternCrate(i) => ExternCrate(name_of(source, i.name_ref())),
		ast::Item::Module(i) => {
			let name = name_of(source, i.name());
			if i.item_list().is_some() { BlockMod(name) } else { Mod(name) }
		}
		ast::Item::Use(_) => Use,
		ast::Item::Const(i) => Const(name_of(source, i.name())),
		ast::Item::Static(i) => Static(name_of(source, i.name())),
		ast::Item::TypeAlias(i) => TypeAlias(name_of(source, i.name())),
		ast::Item::MacroRules(i) => MacroRules(name_of(source, i.name())),
		ast::Item::MacroCall(i) => MacroCall(name_of(source, i.path())),
		ast::Item::Trait(i) => Trait(name_of(source, i.name())),
		ast::Item::Struct(i) => TypeDef(name_of(source, i.name()), TypeDefKind::Struct),
		ast::Item::Enum(i) => TypeDef(name_of(source, i.name()), TypeDefKind::Enum),
		ast::Item::Union(i) => TypeDef(name_of(source, i.name()), TypeDefKind::Union),
		ast::Item::Fn(i) => Fn(name_of(source, i.name())),
		ast::Item::Impl(i) => {
			let ty = i.self_ty().expect("impl always has self_ty");
			let ty = node_text(source, ty.syntax());
			let ty = ty.split('<').next().expect("split always has first element").trim();
			TypeDef(ty, TypeDefKind::Impl(i.trait_().map(|t| node_text(source, t.syntax()))))
		}
		ast::Item::ExternBlock(_) => ExternCrate("extern"),
		ast::Item::MacroDef(i) => MacroRules(name_of(source, i.name())),
		ast::Item::AsmExpr(_) => unreachable!("Unexpected AsmExpr item (rust-analyzer internal syntax)"),
	})
}

fn line_start(source: &str, pos: usize) -> usize {
	source[..pos].rfind('\n').map_or(0, |n| n + 1)
}

fn name_of<'a>(source: &'a str, node: Option<impl AstNode>) -> &'a str {
	node.map(|n| node_text(source, n.syntax())).expect("node has name")
}

fn node_text<'a>(source: &'a str, node: &SyntaxNode) -> &'a str {
	let range = node.text_range();
	&source[usize::from(range.start())..usize::from(range.end())]
}
/// Sort items in a Rust source file by type and name.
pub fn sort_items(source: &str) -> Result<String> {
	let parse = SourceFile::parse(source, Edition::Edition2024);
	let file = parse.tree();

	if !parse.errors().is_empty() {
		anyhow::bail!(
			"File has parse errors, skipping:\n{}",
			parse.errors().iter().map(|e| format!("  {e}")).collect::<Vec<_>>().join("\n")
		);
	}

	let Some(first) = file.items().next() else {
		return Ok(source.to_string());
	};
	let leading = &source[..line_start(source, first.syntax().text_range().start().into())];

	let all: Vec<_> = file.items().collect();
	let mut items: Vec<Item> = all
		.iter()
		.enumerate()
		.map(|(i, item)| {
			let syntax_start: usize = item.syntax().text_range().start().into();
			let syntax_end: usize = item.syntax().text_range().end().into();
			let line_start_pos = line_start(source, syntax_start);
			let prev_end: usize = if i > 0 { all[i - 1].syntax().text_range().end().into() } else { 0 };
			// Use line start if previous item ended on a different line, else syntax start
			let start = if prev_end <= line_start_pos { line_start_pos } else { syntax_start };
			// End at next item's line start, but not before this item ends
			let end = all
				.get(i + 1)
				.map(|next| line_start(source, next.syntax().text_range().start().into()))
				.unwrap_or(source.len())
				.max(syntax_end);
			Ok(Item(classify(source, item)?, &source[start..end]))
		})
		.collect::<Result<Vec<_>>>()?;

	items.sort_by(|a, b| a.0.cmp(&b.0));

	let mut result = leading.to_string();
	let mut prev: Option<(&ItemSort, &str)> = None;

	for Item(sort, text) in &items {
		if let Some((p, prev_text)) = prev {
			debug_assert!(
				result.ends_with('\n'),
				"result should always end with newline after processing an item"
			);
			let both_single_line = !prev_text.trim().contains('\n') && !text.trim().contains('\n');
			let needs_blank = match (p, sort) {
				(ItemSort::Use, ItemSort::Use) => false,
				(ItemSort::TypeDef(n1, _), ItemSort::TypeDef(n2, _)) if n1 == n2 && both_single_line => false,
				(ItemSort::Fn(_), ItemSort::Fn(_)) if both_single_line => false,
				_ => true,
			};
			if needs_blank && !result.ends_with("\n\n") {
				result.push('\n');
			}
		}
		prev = Some((sort, text));
		result.push_str(text);
		if !result.ends_with('\n') {
			result.push('\n');
		}
	}

	while result.ends_with("\n\n") {
		result.pop();
	}
	debug_assert!(result.ends_with('\n'), "result should end with exactly one newline");

	Ok(result)
}

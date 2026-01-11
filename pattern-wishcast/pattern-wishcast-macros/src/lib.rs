// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

mod codegen;

mod field_checking;

mod patterns;

use darling::ast::NestedMeta;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
	Generics, Ident, Result, Token, braced,
	parse::{Parse, ParseStream},
	parse_macro_input,
	punctuated::Punctuated,
};

use darling::FromMeta;

struct AdtCompose {
	uses: Vec<UseDeclaration>,
	items: Vec<AdtItem>,
}

impl Parse for AdtCompose {
	fn parse(input: ParseStream) -> Result<Self> {
		let mut uses = Vec::new();
		let mut items = Vec::new();

		// Parse use declarations first
		while input.peek(Token![use]) {
			uses.push(input.parse::<UseDeclaration>()?);
			if input.peek(Token![;]) {
				input.parse::<Token![;]>()?;
			}
		}

		// Parse items
		while !input.is_empty() {
			items.push(input.parse::<AdtItem>()?);
			if input.peek(Token![;]) {
				input.parse::<Token![;]>()?;
			}
		}

		Ok(AdtCompose { uses, items })
	}
}

enum AdtItem {
	EnumDeclaration(EnumDeclaration),
	PatternType(PatternTypeDeclaration),
	SubtypeImpl(SubtypeImplDeclaration),
	TypeAlias(TypeAlias),
}

impl Parse for AdtItem {
	fn parse(input: ParseStream) -> Result<Self> {
		if input.peek(Token![enum]) {
			Ok(AdtItem::EnumDeclaration(EnumDeclaration::parse_with_attrs(
				input,
				Vec::new(),
				Vec::new(),
			)?))
		} else if input.peek(Token![type]) {
			// Disambiguate between pattern types and simple type aliases
			let fork = input.fork();
			if fork.parse::<Token![type]>().is_ok()
				&& fork.parse::<Ident>().is_ok()
				&& fork.parse::<Token![=]>().is_ok()
				&& fork.parse::<Ident>().is_ok()
				&& fork.peek(syn::Ident)
			{
				// This looks like a pattern type (type X = Y is ...)
				Ok(AdtItem::PatternType(input.parse()?))
			} else {
				// This is a simple type alias (type X = Y<T>)
				Ok(AdtItem::TypeAlias(input.parse()?))
			}
		} else if input.peek(Token![impl]) {
			Ok(AdtItem::SubtypeImpl(input.parse()?))
		} else if input.peek(Token![#]) {
			// Parse outer attributes first
			let attrs = syn::Attribute::parse_outer(input)?;

			if input.peek(Token![impl]) {
				// Re-inject attributes for SubtypeImplDeclaration parsing
				// SubtypeImplDeclaration expects to parse its own attributes, so we need
				// to handle this differently - it already handles #[derive(SubtypingRelation(...))]
				// For now, extract derives and pass them, but SubtypeImpl has its own attr handling
				Ok(AdtItem::SubtypeImpl(SubtypeImplDeclaration::parse_with_attrs(input, attrs)?))
			} else if input.peek(Token![enum]) {
				let (derives, other_attrs) = extract_derives(attrs)?;
				Ok(AdtItem::EnumDeclaration(EnumDeclaration::parse_with_attrs(
					input,
					derives,
					other_attrs,
				)?))
			} else {
				Err(input.error("Expected 'enum' or 'impl' after attributes"))
			}
		} else {
			Err(input.error("Expected 'enum', 'type', or 'impl' declaration"))
		}
	}
}

enum CompositionPart {
	TypeRef(Ident, Option<syn::AngleBracketedGenericArguments>), // External enum like CoreAtoms or Container<T>
	BoxedTypeRef(Ident),                                         // Box<TypedTermComplex>
	InlineVariants { variants: Vec<Variant> },                   // { ... }
}

struct EnumBody(Vec<CompositionPart>);

impl EnumBody {
	fn parse_composition_parts(input: ParseStream, parts: &mut Vec<CompositionPart>) -> Result<()> {
		loop {
			if input.peek(syn::token::Brace) {
				// Inline variants: { ... }
				let variants_content;
				braced!(variants_content in input);
				let variants = variants_content.parse_terminated(Variant::parse, Token![,])?.into_iter().collect();
				parts.push(CompositionPart::InlineVariants { variants });
			} else if input.peek(Ident) && input.peek2(Token![<]) {
				// Generic type reference like Container<T> or Box<Type>
				let ident: Ident = input.parse()?;
				if ident == "Box" {
					input.parse::<Token![<]>()?;
					let type_name: Ident = input.parse()?;
					input.parse::<Token![>]>()?;
					parts.push(CompositionPart::BoxedTypeRef(type_name));
				} else {
					// Generic type reference - preserve the generics
					let generics: syn::AngleBracketedGenericArguments = input.parse()?;
					parts.push(CompositionPart::TypeRef(ident, Some(generics)));
				}
			} else if input.peek(Ident) {
				// Simple type reference
				let type_name: Ident = input.parse()?;
				parts.push(CompositionPart::TypeRef(type_name, None));
			} else {
				return Err(input.error("Expected type reference or inline variants"));
			}

			// Check for continuation with |
			if input.peek(Token![|]) {
				input.parse::<Token![|]>()?;
			} else {
				break;
			}
		}
		Ok(())
	}
}

impl Parse for EnumBody {
	fn parse(input: ParseStream) -> Result<Self> {
		// Handle both braced and direct union syntax
		if input.peek(syn::token::Brace) {
			// New syntax: = { ... }
			let content;
			braced!(content in input);

			if content.is_empty() {
				return Err(content.error("Empty enum body"));
			}

			// Inside braces, we only allow simple variants, not union syntax
			let mut variants = Vec::new();
			while !content.is_empty() {
				variants.push(content.parse::<Variant>()?);

				if content.peek(Token![,]) {
					content.parse::<Token![,]>()?;
				} else if content.peek(Token![|]) {
					return Err(content.error(
						"Union syntax (|) is not allowed inside braces. To compose types, use: enum MyEnum = TypeA | TypeB | { variants }",
					));
				} else if !content.is_empty() {
					return Err(content.error("Expected ',' between variants"));
				}
			}
			Ok(EnumBody(vec![CompositionPart::InlineVariants { variants }]))
		} else {
			// Direct syntax: = TypeRef | TypeRef | { ... }
			let mut parts = Vec::new();
			EnumBody::parse_composition_parts(input, &mut parts)?;
			Ok(EnumBody(parts))
		}
	}
}

struct EnumDeclaration {
	pub attrs: Vec<syn::Attribute>,
	pub derives: Vec<syn::Path>,
	pub name: Ident,
	pub generics: Option<Generics>,
	pub pattern_param: Option<(Ident, Ident)>, // (param_name, trait_name) for "is <P: PatternFields>"
	pub parts: EnumBody,
}

impl EnumDeclaration {
	/// Build the complete generics list combining regular generics with optional pattern parameter
	pub fn full_generics(&self) -> TokenStream2 {
		match (&self.generics, &self.pattern_param) {
			(Some(generics), Some((param_name, trait_name))) => {
				let params = &generics.params;
				quote! { <#params, #param_name: #trait_name> }
			}
			(Some(generics), None) => quote! { #generics },
			(None, Some((param_name, trait_name))) => quote! { <#param_name: #trait_name> },
			(None, None) => quote! {},
		}
	}

	/// Build the enum type with appropriate generic parameters
	pub fn enum_type(&self) -> TokenStream2 {
		let enum_name = &self.name;
		if let Some((param_name, _)) = &self.pattern_param {
			quote! { #enum_name<#param_name> }
		} else {
			let generics = &self.generics;
			quote! { #enum_name #generics }
		}
	}
}

impl EnumDeclaration {
	fn parse_with_attrs(input: ParseStream, derives: Vec<syn::Path>, attrs: Vec<syn::Attribute>) -> Result<Self> {
		// 'enum' keyword is now mandatory
		input.parse::<Token![enum]>()?;

		let name: Ident = input.parse()?;

		let generics = if input.peek(Token![<]) {
			Some(input.parse::<Generics>()?)
		} else {
			None
		};

		// Check for "is <P: Trait>" pattern parameter
		let pattern_param = if input.peek(syn::Ident) && input.peek2(Token![<]) {
			// Parse "is" keyword
			let is_kw: Ident = input.parse()?;
			if is_kw != "is" {
				return Err(syn::Error::new_spanned(is_kw, "Expected 'is' keyword"));
			}

			// Parse <P: Trait>
			input.parse::<Token![<]>()?;
			let param_name: Ident = input.parse()?;
			input.parse::<Token![:]>()?;
			let trait_name: Ident = input.parse()?;
			input.parse::<Token![>]>()?;

			Some((param_name, trait_name))
		} else {
			None
		};

		input.parse::<Token![=]>()?;

		// Parse composition - can be simple variants or union syntax
		let parts = input.parse::<EnumBody>()?;

		Ok(EnumDeclaration {
			attrs,
			derives,
			name,
			generics,
			pattern_param,
			parts,
		})
	}
}

impl Parse for EnumDeclaration {
	fn parse(input: ParseStream) -> Result<Self> {
		Self::parse_with_attrs(input, Vec::new(), Vec::new())
	}
}

#[derive(Clone, Default)]
struct FieldAttributes {
	pub attrs: Vec<syn::Attribute>,
	/// Safety-critical iteration expression for pattern checking
	pub unsafe_transmute_check_iter: Option<String>,
}

/// Cleaner pattern type declaration
struct PatternTypeDeclaration {
	pub name: Ident,
	pub base_type: Ident,
	pub pattern: VariantPattern,
}

impl syn::parse::Parse for PatternTypeDeclaration {
	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		input.parse::<Token![type]>()?;
		let name: Ident = input.parse()?;
		input.parse::<Token![=]>()?;
		let base_type: Ident = input.parse()?;

		let pattern = VariantPattern::parse_is_pattern(input)?;

		Ok(Self { name, base_type, pattern })
	}
}

#[derive(Debug, PartialEq)]
enum SubtypeAttribute {
	SubtypingRelation(SubtypingRelation),
}

struct SubtypeImplDeclaration {
	subtype: Ident,
	supertype: Ident,
	attributes: Vec<SubtypeAttribute>,
}

impl SubtypeImplDeclaration {
	fn parse_with_attrs(input: ParseStream, attrs: Vec<syn::Attribute>) -> Result<Self> {
		let mut attributes = Vec::new();

		for attr in attrs {
			if attr.path().is_ident("derive") {
				// Parse the meta list inside derive(...)
				let nested = attr.parse_args_with(|input: ParseStream| {
					let punctuated: Punctuated<NestedMeta, Token![,]> = Punctuated::parse_terminated(input)?;
					Ok(punctuated)
				})?;

				for meta in nested {
					if let NestedMeta::Meta(meta) = meta
						&& meta.path().is_ident("SubtypingRelation")
					{
						// Use darling to parse the SubtypingRelation
						let subtyping_rel = SubtypingRelation::from_meta(&meta).map_err(|e| syn::Error::new_spanned(&meta, e.to_string()))?;
						attributes.push(SubtypeAttribute::SubtypingRelation(subtyping_rel));
					}
				}
			}
		}

		input.parse::<Token![impl]>()?;
		let subtype: Ident = input.parse()?;
		input.parse::<Token![:]>()?;
		let supertype: Ident = input.parse()?;

		Ok(SubtypeImplDeclaration {
			subtype,
			supertype,
			attributes,
		})
	}
}

impl Parse for SubtypeImplDeclaration {
	fn parse(input: ParseStream) -> Result<Self> {
		let attrs = syn::Attribute::parse_outer(input)?;
		Self::parse_with_attrs(input, attrs)
	}
}

/// Parse #[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
// <generated by cargo-derive-doc>
// Macro expansions:
//   impl ::darling::FromMeta for SubtypingRelation
// </generated by cargo-derive-doc>
#[derive(Debug, FromMeta, PartialEq)]
struct SubtypingRelation {
	pub upcast: syn::Ident,
	pub downcast: syn::Ident,
}

struct TypeAlias {
	name: Ident,
	ty: syn::Type,
}

impl Parse for TypeAlias {
	fn parse(input: ParseStream) -> Result<Self> {
		input.parse::<Token![type]>()?;
		let name: Ident = input.parse()?;
		input.parse::<Token![=]>()?;
		let ty: syn::Type = input.parse()?;

		Ok(TypeAlias { name, ty })
	}
}

struct UseDeclaration {
	path: syn::Path,
}

impl Parse for UseDeclaration {
	fn parse(input: ParseStream) -> Result<Self> {
		input.parse::<Token![use]>()?;
		let path = input.parse::<syn::Path>()?;
		Ok(UseDeclaration { path })
	}
}

#[derive(Clone)]
struct Variant {
	pub attrs: Vec<syn::Attribute>,
	pub name: Ident,
	pub fields: Option<VariantFields>,
}

impl Parse for Variant {
	fn parse(input: ParseStream) -> Result<Self> {
		// Parse variant-level attributes (including doc comments)
		let attrs = syn::Attribute::parse_outer(input)?;

		let name: Ident = input.parse()?;

		let fields = if input.peek(syn::token::Brace) {
			let content;
			braced!(content in input);
			let mut named_fields = Vec::new();

			while !content.is_empty() {
				// Parse field attributes (including doc comments)
				let field_outer_attrs = syn::Attribute::parse_outer(&content)?;
				let mut field_attrs = FieldAttributes {
					attrs: field_outer_attrs.clone(),
					..Default::default()
				};

				for attr in &field_outer_attrs {
					if attr.path().is_ident("unsafe_transmute_check") {
						// Parse the attribute content
						attr.parse_nested_meta(|meta| {
							if meta.path.is_ident("iter") {
								meta.input.parse::<Token![=]>()?;
								let iter_expr: syn::LitStr = meta.input.parse()?;
								field_attrs.unsafe_transmute_check_iter = Some(iter_expr.value());
							}
							Ok(())
						})?;
					}
				}

				let field_name: Ident = content.parse()?;
				content.parse::<Token![:]>()?;
				let field_type: syn::Type = content.parse()?;
				named_fields.push((field_name, field_type, field_attrs));

				if content.peek(Token![,]) {
					content.parse::<Token![,]>()?;
				}
			}

			Some(VariantFields::Named(named_fields))
		} else if input.peek(syn::token::Paren) {
			let content;
			syn::parenthesized!(content in input);
			let types = content.parse_terminated(syn::Type::parse, Token![,])?;
			Some(VariantFields::Unnamed(types.into_iter().collect()))
		} else {
			None
		};

		Ok(Variant { attrs, name, fields })
	}
}

#[derive(Clone)]
enum VariantFields {
	Named(Vec<(Ident, syn::Type, FieldAttributes)>),
	Unnamed(Vec<syn::Type>),
}

/// Parse pattern types more cleanly
#[derive(Debug)]
enum VariantPattern {
	Wildcard,
	Variants(Vec<Ident>),
}

impl VariantPattern {
	fn parse_variant_with_pattern(input: syn::parse::ParseStream) -> syn::Result<Ident> {
		let variant: Ident = input.parse()?;

		// Handle pattern like (_) after variant name
		if input.peek(syn::token::Paren) {
			let parens;
			syn::parenthesized!(parens in input);
			// Only support wildcard patterns for now
			if parens.peek(Token![_]) {
				parens.parse::<Token![_]>()?;
			} else if !parens.is_empty() {
				return Err(parens.error("Complex patterns are not supported. Only wildcard patterns (_) are allowed. Complex patterns like ranges, guards, and nested patterns will require native pattern types support in Rust."));
			}
		}

		// Handle struct variant wildcard like { .. }
		if input.peek(syn::token::Brace) {
			let braces;
			syn::braced!(braces in input);

			// Check for .. wildcard
			if braces.peek(Token![..]) {
				braces.parse::<Token![..]>()?;
				if !braces.is_empty() {
					return Err(braces.error("Only wildcard pattern { .. } is supported for struct variants"));
				}
			} else {
				return Err(braces.error("Field patterns are not supported. Only wildcard pattern { .. } is allowed for struct variants. Field patterns will require native pattern types support in Rust."));
			}
		}

		// Check for guard patterns with 'if'
		if input.peek(syn::Ident) && input.peek2(syn::Ident) {
			let lookahead = input.lookahead1();
			if lookahead.peek(syn::Ident) {
				// Try to parse an identifier to see if it's "if"
				let fork = input.fork();
				if let Ok(ident) = fork.parse::<syn::Ident>()
					&& ident == "if"
				{
					return Err(
						input.error("Guard patterns with 'if' are not supported. Guards will require native pattern types support in Rust.")
					);
				}
			}
		}

		Ok(variant)
	}

	pub fn parse_is_pattern(input: syn::parse::ParseStream) -> syn::Result<Self> {
		// Look for "is"
		let is_ident: Ident = input.parse()?;
		if is_ident != "is" {
			return Err(input.error("Expected 'is' keyword"));
		}

		// Check for wildcard
		if input.peek(Token![_]) {
			input.parse::<Token![_]>()?;
			return Ok(VariantPattern::Wildcard);
		}

		// Parse variant list directly (no outer braces required)
		let mut variants = Vec::new();

		// Parse first variant
		let first_variant = Self::parse_variant_with_pattern(input)?;
		variants.push(first_variant);

		// Continue parsing additional variants separated by |
		while input.peek(Token![|]) {
			input.parse::<Token![|]>()?;
			let variant = Self::parse_variant_with_pattern(input)?;
			variants.push(variant);
		}

		Ok(VariantPattern::Variants(variants))
	}
}

fn expand_pattern_wishcast(input: &AdtCompose) -> TokenStream2 {
	let mut output = TokenStream2::new();

	// Generate use statements
	for use_decl in &input.uses {
		let path = &use_decl.path;
		output.extend(quote! {
			use #path;
		});
	}

	// Separate items by type for processing
	let mut enum_decls = Vec::new();
	let mut pattern_types = Vec::new();
	let mut subtype_impls = Vec::new();
	let mut type_aliases = Vec::new();

	for item in &input.items {
		match item {
			AdtItem::EnumDeclaration(e) => enum_decls.push(e),
			AdtItem::PatternType(p) => pattern_types.push(p),
			AdtItem::SubtypeImpl(s) => subtype_impls.push(s),
			AdtItem::TypeAlias(t) => type_aliases.push(t),
		}
	}

	// Create a map of enum names to their declarations for cross-referencing
	let enum_map: std::collections::HashMap<String, &EnumDeclaration> = enum_decls.iter().map(|decl| (decl.name.to_string(), *decl)).collect();

	// Check if any enum declares pattern support but has no pattern types
	if pattern_types.is_empty() {
		for enum_decl in &enum_decls {
			if enum_decl.pattern_param.is_some() {
				let enum_name = &enum_decl.name;
				return quote! {
					compile_error!(concat!(
						"Enum `", stringify!(#enum_name), "` declares pattern support with `is <P: ...>` but no pattern types are defined. ",
						"Either: 1) Add pattern type declarations like `type FlexValue = ", stringify!(#enum_name), " is _;`, or ",
						"2) Remove the `is <P: ...>` declaration if you don't need pattern-based strictness."
					));
				};
			}
		}
	}

	// Validate that pattern types only reference enums with pattern parameters
	for pattern_type in &pattern_types {
		let base_type_name = pattern_type.base_type.to_string();
		if let Some(enum_decl) = enum_map.get(&base_type_name) {
			if enum_decl.pattern_param.is_none() {
				return quote! {
					compile_error!(concat!(
						"Cannot create pattern type for enum `",
						stringify!(#base_type_name),
						"`. You must declare the enum with pattern support: `enum ",
						stringify!(#base_type_name),
						" is <P: PatternTrait> { ... }`"
					));
				};
			}
		} else {
			return quote! {
				compile_error!(concat!("Unknown base type: ", stringify!(#base_type_name)));
			};
		}
	}

	// Process each enum individually
	for enum_decl in &enum_decls {
		let enum_name = &enum_decl.name;

		// Find pattern types for this enum directly
		let enum_pattern_types: Vec<&PatternTypeDeclaration> = pattern_types.iter().filter(|pt| pt.base_type == *enum_name).copied().collect();

		// Build variants and analyze composition in one efficient pass
		let mut enum_variants = Vec::new();
		let mut variant_names = std::collections::HashSet::new();
		let mut has_type_composition = false;

		for part in &enum_decl.parts.0 {
			match part {
				CompositionPart::InlineVariants { variants } => {
					for variant in variants {
						variant_names.insert(variant.name.to_string());
						enum_variants.push(variant.clone()); // Still need owned for later modification
					}
				}
				CompositionPart::TypeRef(type_name, generics) => {
					has_type_composition = true;
					variant_names.insert(type_name.to_string());
					enum_variants.push(Variant {
						attrs: Vec::new(),
						name: type_name.clone(),
						fields: Some(VariantFields::Unnamed(vec![syn::parse_quote! { #type_name #generics }])),
					});
				}
				CompositionPart::BoxedTypeRef(type_name) => {
					has_type_composition = true;
					variant_names.insert(type_name.to_string());
					enum_variants.push(Variant {
						attrs: Vec::new(),
						name: type_name.clone(),
						fields: Some(VariantFields::Unnamed(vec![syn::parse_quote! { Box<#type_name> }])),
					});
				}
			}
		}

		let conditional_variants = patterns::identify_conditional_variants(&enum_pattern_types, &variant_names);
		let has_composition = !conditional_variants.is_empty() || has_type_composition;

		// Validate pattern enums that declare support but have no conditional variants
		if !enum_pattern_types.is_empty() && conditional_variants.is_empty() {
			// Generate appropriate error messages
			if enum_pattern_types.len() == 1 {
				let single_pattern = &enum_pattern_types[0];
				let pattern_name = &single_pattern.name;
				return quote! {
					compile_error!(concat!(
						"Enum `", stringify!(#enum_name), "` has only one pattern type `", stringify!(#pattern_name), "`. ",
						"Since there are no conditional variants, you don't need pattern support. ",
						"Remove `is <P: PatternFields>` from the enum declaration and use a simple type alias instead: ",
						"`type ", stringify!(#pattern_name), " = ", stringify!(#enum_name), ";`"
					));
				};
			} else {
				return quote! {
					compile_error!(concat!(
						"No conditional variants found for enum `", stringify!(#enum_name), "`. ",
						"All variants are included in all pattern types, making them identical. ",
						"Either: 1) Add variants that are excluded from some pattern types, ",
						"2) Use a single type alias instead of multiple identical ones, or ",
						"3) Remove `is <P: PatternFields>` if you don't need strictness patterns."
					));
				};
			}
		}

		// Generate enum with variant transformation based on pattern analysis
		let (variants, type_transformer): (Vec<_>, Box<dyn Fn(&syn::Type) -> TokenStream2>) = if !conditional_variants.is_empty() {
			// Pattern enum: apply pattern transformation to variants
			let mut modified_variants = Vec::new();
			let pattern_param_name = enum_decl.pattern_param.as_ref().map(|(param_name, _)| param_name).unwrap();

			for variant in &enum_variants {
				let variant_name = &variant.name;
				let variant_name_str = variant_name.to_string();

				// Check if this is an enum-as-variant (either unit variant or TypeRef/BoxedTypeRef)
				let is_enum_variant = variant.fields.is_none() && enum_map.contains_key(&variant_name_str);
				let is_type_ref_variant = matches!(
					&variant.fields,
					Some(VariantFields::Unnamed(types)) if types.len() == 1 && enum_map.contains_key(&variant_name_str)
				);

				if is_enum_variant || is_type_ref_variant {
					// Enum-as-variant (either unit or type reference)
					let referenced_enum_name = &variant_name;

					if conditional_variants.contains(&variant_name_str) {
						// Conditional enum-as-variant as tuple (value, trait_assoc_type)
						let never_field_name = syn::Ident::new(&format!("{variant_name_str}Allowed"), variant_name.span());

						// Preserve original field type for type references (including Box<T>)
						let original_field_type = if let Some(VariantFields::Unnamed(types)) = &variant.fields {
							types[0].clone()
						} else {
							syn::parse_quote! { #referenced_enum_name }
						};

						modified_variants.push(Variant {
							attrs: variant.attrs.clone(),
							name: variant_name.clone(),
							fields: Some(VariantFields::Unnamed(vec![
								original_field_type,
								syn::parse_quote! { #pattern_param_name::#never_field_name },
							])),
						});
					} else {
						// Regular enum-as-variant - preserve original field type
						let original_field_type = if let Some(VariantFields::Unnamed(types)) = &variant.fields {
							types[0].clone()
						} else {
							syn::parse_quote! { #referenced_enum_name }
						};

						modified_variants.push(Variant {
							attrs: variant.attrs.clone(),
							name: variant_name.clone(),
							fields: Some(VariantFields::Unnamed(vec![original_field_type])),
						});
					}
				} else if conditional_variants.contains(&variant_name_str) {
					// Conditional variant with _never field
					let never_field_name = syn::Ident::new(&format!("{variant_name_str}Allowed"), variant_name.span());
					let mut new_variant = variant.clone();

					match &mut new_variant.fields {
						Some(VariantFields::Named(fields)) => {
							fields.push((
								syn::Ident::new("_never", variant_name.span()),
								syn::parse_quote! { #pattern_param_name::#never_field_name },
								FieldAttributes::default(),
							));
						}
						Some(VariantFields::Unnamed(_)) => {
							// Skip tuple variants with conditionals for now
						}
						None => {
							new_variant.fields = Some(VariantFields::Named(vec![(
								syn::Ident::new("_never", variant_name.span()),
								syn::parse_quote! { #pattern_param_name::#never_field_name },
								FieldAttributes::default(),
							)]));
						}
					}
					modified_variants.push(new_variant);
				} else {
					// Regular variant
					modified_variants.push(variant.clone());
				}
			}

			let pattern_param_name_clone = pattern_param_name.clone();
			(
				modified_variants,
				Box::new(move |ty| codegen::fix_self_references(ty, enum_name, &pattern_param_name_clone)),
			)
		} else {
			// Simple enum: choose strategy based on composition
			if has_composition {
				(enum_variants.clone(), Box::new(|ty| quote! { #ty }))
			} else {
				(
					enum_variants.clone(),
					Box::new(|ty| codegen::fix_concrete_references(ty, &enum_map)),
				)
			}
		};

		// Transform variants using the appropriate strategy
		let expanded_variants: Vec<_> = variants
			.iter()
			.map(|v| codegen::expand_variant_with(v, |ty| type_transformer(ty)))
			.collect();

		let full_generics = enum_decl.full_generics();

		let derive_attr = if enum_decl.derives.is_empty() {
			quote! { #[derive(Debug, Clone)] }
		} else {
			let paths = &enum_decl.derives;
			quote! { #[derive(#(#paths),*)] }
		};

		let enum_attrs = &enum_decl.attrs;

		output.extend(quote! {
			#derive_attr
			#(#enum_attrs)*
			#[repr(C)]
			pub enum #enum_name #full_generics {
				#(#expanded_variants),*
			}
		});

		if has_composition {
			codegen::generate_from_traits(
				&mut output,
				enum_decl,
				if conditional_variants.is_empty() {
					None
				} else {
					Some(&conditional_variants)
				},
			);
		}

		// Only do pattern-specific generation if we have conditional variants
		if !conditional_variants.is_empty() {
			// pattern_param is guaranteed Some when conditional_variants is non-empty
			let (_, strictness_trait_name) = enum_decl
				.pattern_param
				.as_ref()
				.expect("conditional_variants requires pattern_param");

			// Generate strictness system
			output.extend(patterns::generate_strictness_system(
				enum_name,
				strictness_trait_name,
				&enum_pattern_types,
				&conditional_variants,
			));

			// Generate conversion methods
			generate_subtype_conversions(
				&mut output,
				enum_decl,
				&enum_variants,
				&conditional_variants,
				&subtype_impls,
				&enum_pattern_types,
			);

			// Generate automatic tests for subtyping relationships
			generate_subtyping_tests(&mut output, &enum_variants, &conditional_variants, &subtype_impls, &enum_map);
		}
	}

	// Generate simple type aliases
	for alias in &type_aliases {
		let name = &alias.name;
		let ty = &alias.ty;
		output.extend(quote! {
			pub type #name = #ty;
		});
	}

	output
}

/// Extract derive macro paths from attributes, returning (derives, other_attrs)
fn extract_derives(attrs: Vec<syn::Attribute>) -> Result<(Vec<syn::Path>, Vec<syn::Attribute>)> {
	let mut derives = Vec::new();
	let mut other_attrs = Vec::new();
	for attr in attrs {
		if attr.path().is_ident("derive") {
			attr.parse_nested_meta(|meta| {
				derives.push(meta.path);
				Ok(())
			})?;
		} else {
			other_attrs.push(attr);
		}
	}
	Ok((derives, other_attrs))
}

fn generate_subtype_conversions(
	output: &mut TokenStream2,
	enum_decl: &EnumDeclaration,
	enum_variants: &[Variant],
	conditional_variants: &std::collections::HashSet<String>,
	subtype_impls: &[&SubtypeImplDeclaration],
	pattern_types: &[&PatternTypeDeclaration],
) {
	let enum_name = &enum_decl.name;
	let pattern_allowed_variants: std::collections::HashMap<String, Option<std::collections::HashSet<String>>> = pattern_types
		.iter()
		.map(|pt| {
			let allowed = match &pt.pattern {
				VariantPattern::Wildcard => None, // All variants allowed
				VariantPattern::Variants(variants) => Some(variants.iter().map(|v| v.to_string()).collect()),
			};
			(pt.name.to_string(), allowed)
		})
		.collect();

	// Helper function to generate variant checks for a given check method
	// `allowed_variants` is the set of variants allowed in the target pattern (None = wildcard)
	let generate_variant_checks =
		|supertype: &Ident, check_ident: &Ident, allowed_variants: Option<&std::collections::HashSet<String>>| -> Vec<TokenStream2> {
			enum_variants
				.iter()
				.map(|variant| {
					let variant_name = &variant.name;
					let variant_name_str = variant_name.to_string();

					// A variant is rejected if it's conditional AND not in the target pattern's allowed list
					let is_rejected = conditional_variants.contains(&variant_name_str)
						&& allowed_variants.is_some_and(|allowed| !allowed.contains(&variant_name_str));

					if is_rejected {
						quote! {
							#supertype::#variant_name { .. } => Err(()),
						}
					} else {
						// Generate recursive checking for allowed variants
						// Note: conditional variants have a _never field added, so use { .. } pattern
						let is_conditional = conditional_variants.contains(&variant_name_str);
						match &variant.fields {
							None => {
								// Unit variant - but if conditional, it has _never field added
								if is_conditional {
									quote! {
										#supertype::#variant_name { .. } => Ok(()),
									}
								} else {
									quote! {
										#supertype::#variant_name => Ok(()),
									}
								}
							}
							Some(VariantFields::Named(fields)) => {
								let field_checks_with_names: Vec<_> = fields
									.iter()
									.filter_map(|(field_name, field_type, field_attrs)| {
										field_checking::generate_field_check(field_name, field_type, field_attrs, check_ident, enum_name)
											.map(|check| (field_name, check))
									})
									.collect();

								if field_checks_with_names.is_empty() {
									quote! {
										#supertype::#variant_name { .. } => Ok(()),
									}
								} else {
									let field_names: Vec<_> = field_checks_with_names.iter().map(|(name, _)| name).collect();
									let field_checks: Vec<_> = field_checks_with_names.iter().map(|(_, check)| check).collect();
									quote! {
										#supertype::#variant_name { #(#field_names),*, .. } => {
											#(#field_checks)*
											Ok(())
										},
									}
								}
							}
							Some(VariantFields::Unnamed(types)) => {
								let field_names: Vec<_> = (0..types.len())
									.map(|i| syn::Ident::new(&format!("field_{i}"), variant_name.span()))
									.collect();
								let field_checks: Vec<_> = types
									.iter()
									.enumerate()
									.filter_map(|(i, field_type)| {
										let field_name = &field_names[i];
										let default_attrs = FieldAttributes::default();
										field_checking::generate_field_check(field_name, field_type, &default_attrs, check_ident, enum_name)
									})
									.collect();

								if field_checks.is_empty() {
									// Use wildcard pattern when no field checks are needed
									quote! {
										#supertype::#variant_name(..) => Ok(()),
									}
								} else {
									quote! {
										#supertype::#variant_name(#(#field_names),*) => {
											#(#field_checks)*
											Ok(())
										},
									}
								}
							}
						}
					}
				})
				.collect()
		};

	// Generate conversion methods based on subtype implementations specified in the macro
	for subtype_impl in subtype_impls {
		for attr in &subtype_impl.attributes {
			let SubtypeAttribute::SubtypingRelation(rel) = attr;
			let subtype = &subtype_impl.subtype;
			let supertype = &subtype_impl.supertype;

			// Generate method names
			let upcast_ident = rel.upcast.clone();
			let upcast_ref_ident = syn::Ident::new(&format!("{}_ref", rel.upcast), subtype.span());
			// NOTE: We don't generate upcast_mut_ident because mutable upcasts are unsound

			let downcast_ident = rel.downcast.clone();
			let downcast_ref_ident = syn::Ident::new(&format!("{}_ref", rel.downcast), supertype.span());
			let downcast_mut_ident = syn::Ident::new(&format!("{}_mut", rel.downcast), supertype.span());
			let check_ident = syn::Ident::new(
				&format!("check_{}", rel.downcast.to_string().trim_start_matches("try_")),
				supertype.span(),
			);

			// Generate safe upcast conversions (subtype -> supertype)
			output.extend(quote! {
				impl #subtype {
					pub fn #upcast_ident(self) -> #supertype {
						unsafe { std::mem::transmute(self) }
					}

					pub fn #upcast_ref_ident(&self) -> &#supertype {
						unsafe { std::mem::transmute(self) }
					}

					// NOTE: We intentionally do NOT generate an upcast_mut method
					// Upcasting &mut SubType to &mut SuperType is unsound!
					// It would allow writing SuperType-only variants through the reference,
					// violating SubType's invariants.
				}
			});

			// Generate checked downcast conversions (supertype -> subtype)
			let subtype_allowed = pattern_allowed_variants.get(&subtype.to_string()).and_then(|opt| opt.as_ref());
			let variant_checks = generate_variant_checks(supertype, &check_ident, subtype_allowed);

			output.extend(quote! {
				impl #supertype {
					pub fn #check_ident(&self) -> Result<(), ()> {
						match self {
							#(#variant_checks)*
						}
					}

					pub fn #downcast_ident(self) -> Result<#subtype, Self> {
						match self.#check_ident() {
							Ok(()) => unsafe { Ok(std::mem::transmute(self)) },
							Err(()) => Err(self),
						}
					}

					pub fn #downcast_ref_ident(&self) -> Result<&#subtype, ()> {
						match self.#check_ident() {
							Ok(()) => unsafe { Ok(std::mem::transmute(self)) },
							Err(()) => Err(()),
						}
					}

					pub fn #downcast_mut_ident(&mut self) -> Result<&mut #subtype, ()> {
						match self.#check_ident() {
							Ok(()) => unsafe { Ok(std::mem::transmute(self)) },
							Err(()) => Err(()),
						}
					}
				}
			});
		}
	}
}

/// Generate automatic test code for subtyping relationships to verify transmute safety
fn generate_subtyping_tests(
	output: &mut TokenStream2,
	enum_variants: &[Variant],
	conditional_variants: &std::collections::HashSet<String>,
	subtype_impls: &[&SubtypeImplDeclaration],
	enum_map: &std::collections::HashMap<String, &EnumDeclaration>,
) {
	for subtype_impl in subtype_impls {
		for attr in &subtype_impl.attributes {
			let SubtypeAttribute::SubtypingRelation(rel) = attr;
			let subtype = &subtype_impl.subtype;
			let supertype = &subtype_impl.supertype;

			// Generate method names
			let upcast_ident = &rel.upcast;
			let upcast_ref_ident = syn::Ident::new(&format!("{}_ref", rel.upcast), subtype.span());
			let downcast_ident = &rel.downcast;

			// Generate test function name
			let test_fn_name = syn::Ident::new(
				&format!(
					"test_subtyping_{}_{}",
					subtype.to_string().to_lowercase(),
					supertype.to_string().to_lowercase()
				),
				subtype.span(),
			);

			// Find a non-conditional variant to use for testing
			'variant_loop: for variant in enum_variants.iter().filter(|v| !conditional_variants.contains(&v.name.to_string())) {
				let variant_name = &variant.name;

				// Generate test constructor based on variant fields
				let test_constructor = match &variant.fields {
					None => quote! { #subtype::#variant_name },
					Some(VariantFields::Named(fields)) => {
						let mut field_inits = Vec::new();
						for (name, ty, _attrs) in fields {
							match generate_test_value_for_type(ty, enum_map) {
								Ok(test_value) => {
									field_inits.push(quote! { #name: #test_value });
								}
								Err(_) => {
									// Skip generating tests for variants with unsupported field types
									continue 'variant_loop;
								}
							}
						}
						quote! { #subtype::#variant_name { #(#field_inits),* } }
					}
					Some(VariantFields::Unnamed(types)) => {
						// For tuple variants, we need to handle union composition vs inline variants differently
						if types.len() == 1 {
							// This is likely a union composition variant like CoreAtoms(CoreAtoms)
							let ty = &types[0];
							match generate_test_value_for_type(ty, enum_map) {
								Ok(test_value) => {
									quote! { #subtype::#variant_name(#test_value) }
								}
								Err(_) => {
									// Skip generating tests for variants with unsupported field types
									continue 'variant_loop;
								}
							}
						} else {
							// Multiple fields - generate test values for each
							let mut test_values = Vec::new();
							for ty in types {
								match generate_test_value_for_type(ty, enum_map) {
									Ok(test_value) => {
										test_values.push(test_value);
									}
									Err(_) => {
										// Skip generating tests for variants with unsupported field types
										continue 'variant_loop;
									}
								}
							}
							quote! { #subtype::#variant_name(#(#test_values),*) }
						}
					}
				};

				// Generate appropriate match pattern based on variant type
				let match_pattern = match &variant.fields {
					None => quote! { #supertype::#variant_name },
					Some(VariantFields::Named(_)) => quote! { #supertype::#variant_name { .. } },
					Some(VariantFields::Unnamed(_)) => quote! { #supertype::#variant_name(..) },
				};

				output.extend(quote! {
					#[cfg(test)]
					#[test]
					fn #test_fn_name() {
						use std::mem::discriminant;

						// Test discriminant preservation
						let strict = #test_constructor;
						let flex = strict.#upcast_ident();

						// Get discriminant values
						let strict_disc = discriminant(&#test_constructor);
						let flex_disc = discriminant(&flex);

						// Get raw discriminant values for comparison
						let strict_raw: usize = unsafe { *(&strict_disc as *const _ as *const usize) };
						let flex_raw: usize = unsafe { *(&flex_disc as *const _ as *const usize) };

						// Should have same raw value
						assert_eq!(strict_raw, flex_raw, "Raw discriminants should match between {} and {}", stringify!(#subtype), stringify!(#supertype));

						// Test reference conversions
						let mut strict_for_ref = #test_constructor;

						// Test immutable reference conversion
						let flex_ref: &#supertype = strict_for_ref.#upcast_ref_ident();
						assert!(matches!(flex_ref, #match_pattern), "Reference conversion failed");


						// Test pointer identity
						let strict_ptr = &strict_for_ref as *const _ as usize;
						let flex_ptr = strict_for_ref.#upcast_ref_ident() as *const _ as usize;
						assert_eq!(strict_ptr, flex_ptr, "Reference conversion changed pointer");

						// Test round-trip conversion for allowed variants
						let upcast_value = #test_constructor.#upcast_ident();
						match upcast_value.#downcast_ident() {
							Ok(downcast) => {
								// Should be able to round-trip successfully
								let downcast_disc = discriminant(&downcast);
								let original_disc = discriminant(&#test_constructor);
								assert_eq!(downcast_disc, original_disc, "Round-trip conversion corrupted discriminant");
							}
							Err(_) => panic!("Valid variant should round-trip successfully"),
						}
					}
				});

				// Successfully generated a test, break out of the variant loop
				break 'variant_loop;
			}
		}
	}
}

/// Generate a simple test value for a given type
fn generate_test_value_for_type(
	ty: &syn::Type,
	enum_map: &std::collections::HashMap<String, &EnumDeclaration>,
) -> std::result::Result<TokenStream2, String> {
	// Extract the base type name for simple pattern matching
	let type_str = quote! { #ty }.to_string();

	if type_str.contains("String") {
		Ok(quote! { "test".to_string() })
	} else if type_str.contains("Box<") {
		// For Box<T>, recursively generate the inner value
		if let syn::Type::Path(type_path) = ty
			&& let Some(segment) = type_path.path.segments.last()
			&& segment.ident == "Box"
			&& let syn::PathArguments::AngleBracketed(args) = &segment.arguments
			&& let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
		{
			let inner_value = generate_test_value_for_type(inner_ty, enum_map)?;
			return Ok(quote! { Box::new(#inner_value) });
		}
		Err(format!("Could not parse Box type: {type_str}"))
	} else if type_str.contains("Vec<") {
		Ok(quote! { vec![] })
	} else if type_str.contains("i32") || type_str.contains("i64") {
		Ok(quote! { 42 })
	} else if type_str.contains("usize") {
		Ok(quote! { 0 })
	} else if type_str.contains("bool") {
		Ok(quote! { true })
	} else if let syn::Type::Path(type_path) = ty {
		// Check if this is a known enum type
		if let Some(segment) = type_path.path.segments.last() {
			let type_name = segment.ident.to_string();
			if let Some(enum_decl) = enum_map.get(&type_name) {
				// Find the first unit variant or simplest variant to construct
				if let Some(simple_variant) = enum_decl.parts.0.iter().find_map(|part| match part {
					crate::CompositionPart::InlineVariants { variants } => variants.iter().find(|v| v.fields.is_none()),
					_ => None,
				}) {
					let variant_name = &simple_variant.name;
					let type_ident = syn::Ident::new(&type_name, variant_name.span());
					Ok(quote! { #type_ident::#variant_name })
				} else {
					Err(format!("No unit variant found in enum {type_name}"))
				}
			} else {
				Err(format!("Unknown type: {type_name}"))
			}
		} else {
			Err(format!("Complex path type not supported: {type_str}"))
		}
	} else {
		Err(format!("Unsupported type for test generation: {type_str}"))
	}
}

#[proc_macro]
pub fn pattern_wishcast(tokens: TokenStream) -> TokenStream {
	let input = parse_macro_input!(tokens as AdtCompose);
	let expanded = expand_pattern_wishcast(&input);
	TokenStream::from(expanded)
}

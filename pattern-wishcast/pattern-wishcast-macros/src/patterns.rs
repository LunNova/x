// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::collections::HashSet;

use crate::{EnumDeclaration, PatternTypeDeclaration, VariantPattern};

/// Generate strictness trait and types for pattern support
pub fn generate_strictness_system(
	enum_decl: &EnumDeclaration,
	pattern_types: &[&PatternTypeDeclaration],
	conditional_variants: &HashSet<String>,
) -> TokenStream2 {
	let mut output = TokenStream2::new();
	let enum_name = &enum_decl.name;

	// Generate strictness trait using user-specified name or default
	let strictness_trait_name = if let Some((_, trait_name)) = &enum_decl.pattern_param {
		trait_name.clone()
	} else {
		syn::Ident::new(&format!("{enum_name}Strictness"), enum_name.span())
	};

	let strictness_assoc_types: Vec<_> = conditional_variants
		.iter()
		.map(|cv| {
			let assoc_type_name = syn::Ident::new(&format!("{cv}Allowed"), enum_name.span());
			quote! { type #assoc_type_name; }
		})
		.collect();

	output.extend(quote! {
		pub trait #strictness_trait_name: Clone + Copy + std::fmt::Debug + PartialEq + Eq + std::hash::Hash {
			#(#strictness_assoc_types)*
		}
	});

	// Generate strictness types first so they're available for concrete enum references
	// Also generate the unrestricted type
	let unrestricted_type_name = syn::Ident::new(&format!("{enum_name}Type"), enum_name.span());
	output.extend(quote! {
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
		pub struct #unrestricted_type_name;
	});

	// Generate unrestricted trait impl (all variants allowed)
	let unrestricted_assoc_type_impls: Vec<_> = conditional_variants
		.iter()
		.map(|cv| {
			let assoc_type_name = syn::Ident::new(&format!("{cv}Allowed"), enum_name.span());
			quote! { type #assoc_type_name = (); }
		})
		.collect();

	output.extend(quote! {
		impl #strictness_trait_name for #unrestricted_type_name {
			#(#unrestricted_assoc_type_impls)*
		}
	});

	// Generate pattern-specific strictness types
	for pattern_type in pattern_types {
		let pattern_name = &pattern_type.name;
		let strictness_type_name = syn::Ident::new(&format!("{pattern_name}Type"), pattern_name.span());

		// Generate strictness type
		output.extend(quote! {
			#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
			pub struct #strictness_type_name;
		});

		// Generate strictness trait impl
		let mut assoc_type_impls = Vec::new();
		for conditional_variant in conditional_variants {
			let assoc_type_name = syn::Ident::new(&format!("{conditional_variant}Allowed"), enum_name.span());

			let allowed = match &pattern_type.pattern {
				VariantPattern::Wildcard => quote! { () },
				VariantPattern::Variants(variants) => {
					if variants.iter().any(|v| *v == *conditional_variant) {
						quote! { () }
					} else {
						quote! { ::pattern_wishcast::Never }
					}
				}
			};

			assoc_type_impls.push(quote! {
				type #assoc_type_name = #allowed;
			});
		}

		output.extend(quote! {
			impl #strictness_trait_name for #strictness_type_name {
				#(#assoc_type_impls)*
			}
		});
	}

	// Generate type aliases
	for pattern_type in pattern_types {
		let pattern_name = &pattern_type.name;
		let strictness_type_name = syn::Ident::new(&format!("{pattern_name}Type"), pattern_name.span());

		// Generate type alias
		output.extend(quote! {
			pub type #pattern_name = #enum_name<#strictness_type_name>;
		});
	}

	output
}

/// Identify which variants appear in patterns as conditional
pub fn identify_conditional_variants(pattern_types: &[&PatternTypeDeclaration], all_variant_names: &HashSet<String>) -> HashSet<String> {
	pattern_types
		.iter()
		.flat_map(|pt| match &pt.pattern {
			VariantPattern::Wildcard => Vec::new(),
			VariantPattern::Variants(variants) => {
				let pattern_variant_names: HashSet<String> = variants.iter().map(|v| v.to_string()).collect();
				all_variant_names.difference(&pattern_variant_names).cloned().collect()
			}
		})
		.collect()
}

// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::collections::{HashMap, HashSet};
use syn::Ident;

use crate::{CompositionPart, EnumDeclaration, Variant, VariantFields};

/// Generic variant expansion with customizable type transformation
pub fn expand_variant_with<F>(variant: &Variant, mut type_transformer: F) -> TokenStream2
where
	F: FnMut(&syn::Type) -> TokenStream2,
{
	let name = &variant.name;
	let variant_attrs = filter_internal_attrs(&variant.attrs);

	match &variant.fields {
		None => quote! { #(#variant_attrs)* #name },
		Some(VariantFields::Named(fields)) => {
			let field_tokens: Vec<_> = fields
				.iter()
				.map(|(fname, ftype, field_attrs)| {
					let transformed_type = type_transformer(ftype);
					let attrs = filter_internal_attrs(&field_attrs.attrs);
					quote! { #(#attrs)* #fname: #transformed_type }
				})
				.collect();
			quote! { #(#variant_attrs)* #name { #(#field_tokens),* } }
		}
		Some(VariantFields::Unnamed(types)) => {
			let transformed_types: Vec<_> = types.iter().map(type_transformer).collect();
			quote! { #(#variant_attrs)* #name(#(#transformed_types),*) }
		}
	}
}

/// Filter out macro-internal attributes that shouldn't be emitted to output
fn filter_internal_attrs(attrs: &[syn::Attribute]) -> Vec<&syn::Attribute> {
	attrs
		.iter()
		.filter(|attr| !attr.path().is_ident("unsafe_transmute_check"))
		.collect()
}

pub fn fix_concrete_references(ty: &syn::Type, enum_map: &HashMap<String, &EnumDeclaration>) -> TokenStream2 {
	fix_type_references(
		ty,
		|ident| {
			if let Some(enum_decl) = enum_map.get(&ident.to_string()) {
				if enum_decl.pattern_param.is_some() {
					// Only add unrestricted type parameter for enums with pattern support
					let unrestricted_type_name = syn::Ident::new(&format!("{ident}Type"), ident.span());
					Some(quote! { #ident<#unrestricted_type_name> })
				} else {
					// Simple enums should be referenced without generic parameters
					None
				}
			} else {
				None
			}
		},
		|inner_ty| fix_concrete_references(inner_ty, enum_map),
	)
}

pub fn fix_self_references(ty: &syn::Type, enum_name: &Ident, pattern_param_name: &Ident) -> TokenStream2 {
	fix_type_references(
		ty,
		|ident| {
			if ident == "Self" || ident == enum_name {
				Some(quote! { #enum_name<#pattern_param_name> })
			} else {
				None
			}
		},
		|inner_ty| fix_self_references(inner_ty, enum_name, pattern_param_name),
	)
}

/// Generic type reference fixer with customizable identifier handling
fn fix_type_references<F, R>(ty: &syn::Type, identifier_fixer: F, recursive_fixer: R) -> TokenStream2
where
	F: Fn(&Ident) -> Option<TokenStream2>,
	R: Fn(&syn::Type) -> TokenStream2,
{
	match ty {
		syn::Type::Path(type_path) => {
			// Handle paths like Vec<Value> or Box<Value>
			let mut new_path = type_path.clone();

			// Fix the path segments recursively
			for segment in &mut new_path.path.segments {
				if let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments {
					// Check each type argument
					for arg in &mut args.args {
						if let syn::GenericArgument::Type(inner_ty) = arg {
							// Recursively fix inner types
							let fixed = recursive_fixer(inner_ty);
							*inner_ty = syn::parse2(fixed).unwrap_or_else(|_| inner_ty.clone());
						}
					}
				}
			}

			// Check if this is a direct reference that needs fixing
			if let Some(segment) = new_path.path.segments.last()
				&& segment.arguments.is_empty()
				&& let Some(replacement) = identifier_fixer(&segment.ident)
			{
				return replacement;
			}

			quote! { #new_path }
		}
		_ => quote! { #ty },
	}
}

/// Generate From trait implementations for union composition
pub fn generate_from_traits(output: &mut TokenStream2, enum_decl: &EnumDeclaration, conditional_variants: Option<&HashSet<String>>) {
	for comp_part in &enum_decl.parts.0 {
		match comp_part {
			CompositionPart::TypeRef(type_name, generics) => {
				let type_name_str = type_name.to_string();
				// Skip if this variant is conditional (filtered out)
				if conditional_variants.is_none_or(|cv| !cv.contains(&type_name_str)) {
					let impl_generics = enum_decl.full_generics();
					let target_type = enum_decl.enum_type();
					let enum_name = &enum_decl.name;

					output.extend(quote! {
						impl #impl_generics From<#type_name #generics> for #target_type {
							fn from(value: #type_name #generics) -> Self {
								#enum_name::#type_name(value)
							}
						}
					});
				}
			}
			CompositionPart::BoxedTypeRef(type_name) => {
				let type_name_str = type_name.to_string();
				// Skip if this variant is conditional (filtered out)
				if conditional_variants.is_none_or(|cv| !cv.contains(&type_name_str)) {
					let impl_generics = enum_decl.full_generics();
					let target_type = enum_decl.enum_type();
					let enum_name = &enum_decl.name;

					output.extend(quote! {
						impl #impl_generics From<#type_name> for #target_type {
							fn from(value: #type_name) -> Self {
								#enum_name::#type_name(Box::new(value))
							}
						}
					});
				}
			}
			CompositionPart::InlineVariants { .. } => {
				// No From traits needed for inline variants
			}
		}
	}
}

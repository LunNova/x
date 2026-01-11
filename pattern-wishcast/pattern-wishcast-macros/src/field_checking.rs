// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// Check if a type contains Value types anywhere in its structure (recursively)
pub fn contains_value_type(ty: &syn::Type, enum_name: &Ident) -> bool {
	if is_value_type(ty, enum_name) {
		return true;
	}

	if let syn::Type::Path(type_path) = ty
		&& let Some(segment) = type_path.path.segments.last()
		&& let syn::PathArguments::AngleBracketed(args) = &segment.arguments
	{
		args.args
			.iter()
			.any(|arg| matches!(arg, syn::GenericArgument::Type(inner_type) if contains_value_type(inner_type, enum_name)))
	} else {
		false
	}
}

/// Recursively generate check code for a type, handling nested containers.
/// `var_expr` is the expression to access the value (e.g., `field_name`, `elem`, `(*box_val)`).
/// `depth` prevents infinite recursion.
fn generate_check_for_type(
	ty: &syn::Type,
	var_expr: TokenStream2,
	check_method: &Ident,
	enum_name: &Ident,
	depth: usize,
) -> Option<TokenStream2> {
	const MAX_DEPTH: usize = 10;
	if depth > MAX_DEPTH {
		return Some(quote! {
			compile_error!("Type nesting too deep for automatic field checking. Use #[unsafe_transmute_check(iter=\"...\")].");
		});
	}

	// Direct Value type check
	if is_value_type(ty, enum_name) {
		return Some(quote! {
			#var_expr.#check_method()?;
		});
	}

	if let syn::Type::Path(type_path) = ty
		&& let Some(segment) = type_path.path.segments.last()
	{
		if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
			&& let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
		{
			let inner_var = syn::Ident::new(&format!("__check_{depth}"), proc_macro2::Span::call_site());

			match segment.ident.to_string().as_str() {
				"Vec" => {
					// var_expr is already &Vec from pattern matching, so iterate directly
					if let Some(inner_check) = generate_check_for_type(inner_type, quote! { #inner_var }, check_method, enum_name, depth + 1) {
						return Some(quote! {
							for #inner_var in #var_expr {
								#inner_check
							}
						});
					}
				}
				"Box" => {
					// For Box<T>, rely on auto-deref for method calls
					// The inner_var will be &Box<T> or Box<T>, and auto-deref finds the method on T
					if let Some(inner_check) = generate_check_for_type(inner_type, quote! { #var_expr }, check_method, enum_name, depth + 1) {
						return Some(inner_check);
					}
				}
				"Option" => {
					if let Some(inner_check) = generate_check_for_type(inner_type, quote! { #inner_var }, check_method, enum_name, depth + 1) {
						return Some(quote! {
							if let Some(ref #inner_var) = *#var_expr {
								#inner_check
							}
						});
					}
				}
				_ => {
					// Unknown container - check if it contains Value types and error
					if contains_value_type(ty, enum_name) {
						let type_name = &segment.ident;
						return Some(quote! {
							compile_error!(concat!(
								"Unsupported field type: ",
								stringify!(#type_name),
								" containing Value types. Only Vec<T>, Box<T>, and Option<T> are supported for generic containers. ",
								"Use #[unsafe_transmute_check(iter=\"...\")] to provide custom checking."
							));
						});
					}
				}
			}
		}
	}
	None
}

/// Generate recursive field checking code for a field that may contain child Value references
pub fn generate_field_check(
	field_name: &Ident,
	field_type: &syn::Type,
	field_attrs: &crate::FieldAttributes,
	check_method: &Ident,
	enum_name: &Ident,
) -> Option<TokenStream2> {
	// Check if user provided a custom iterator expression
	if let Some(iter_expr) = &field_attrs.unsafe_transmute_check_iter {
		// Generate check using the user-provided iteration expression
		let iter_tokens: TokenStream2 = iter_expr.parse().unwrap_or_else(|_| {
			quote! {
				compile_error!(concat!(
					"Invalid iteration expression in #[unsafe_transmute_check]: ",
					stringify!(#iter_expr)
				))
			}
		});

		return Some(quote! {
			for elem in #field_name #iter_tokens {
				elem.#check_method()?;
			}
		});
	}

	// Use recursive helper to generate check code
	generate_check_for_type(field_type, quote! { #field_name }, check_method, enum_name, 0)
}

/// Check if a type is a Value type that needs strictness checking
pub fn is_value_type(ty: &syn::Type, enum_name: &Ident) -> bool {
	if let syn::Type::Path(type_path) = ty
		&& let Some(segment) = type_path.path.segments.last()
	{
		segment.ident == "Self" || segment.ident == *enum_name
	} else {
		false
	}
}

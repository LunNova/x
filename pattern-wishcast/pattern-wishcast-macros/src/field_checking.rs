// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

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
	if let syn::Type::Path(type_path) = field_type
		&& let Some(segment) = type_path.path.segments.last()
	{
		match segment.ident.to_string().as_str() {
			"Vec" => {
				// Vec<T> where T might be a Value type
				if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
					&& let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
					&& is_value_type(inner_type, enum_name)
				{
					return Some(quote! {
						for elem in #field_name {
							elem.#check_method()?;
						}
					});
				}
			}
			"Box" => {
				// Box<T> where T might be a Value type
				if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
					&& let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
					&& is_value_type(inner_type, enum_name)
				{
					return Some(quote! {
						#field_name.#check_method()?;
					});
				}
			}
			"Option" => {
				// Option<T> where T might be a Value type
				if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
					&& let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
					&& is_value_type(inner_type, enum_name)
				{
					return Some(quote! {
						if let Some(ref value) = #field_name {
							value.#check_method()?;
						}
					});
				}
			}
			_ => {
				// Direct Value type (like Self, Value<S>)
				if is_value_type(field_type, enum_name) {
					return Some(quote! {
						#field_name.#check_method()?;
					});
				}

				// Check for unknown generic types that contain Self/Value types
				if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
					for arg in &args.args {
						if let syn::GenericArgument::Type(inner_type) = arg
							&& contains_value_type(inner_type, enum_name)
						{
							// Error: unsupported generic type containing Self/Value
							let type_name = &segment.ident;
							return Some(quote! {
								compile_error!(concat!(
									"Unsupported field type: ",
									stringify!(#type_name),
									" containing Value types. Only Vec<T>, Box<T>, and Option<T> are supported for generic containers. ",
									"Field: ",
									stringify!(#field_name)
								));
							});
						}
					}
				}
			}
		}
	}
	None
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

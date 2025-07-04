// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test forward references between enums defined in the same macro block

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// This should work: TypedTerm can reference TypedTermComplex even though it's defined later
	enum TypedTerm = CoreAtoms | Box<TypedTermComplex> | {
		Literal { value: String },
		TupleCons { elements: Vec<TypedTerm> }, // Self-reference should work
	};

	// This should work: TypedTermComplex can reference TypedTerm even though it's defined earlier
	enum TypedTermComplex = {
		Lambda { body: TypedTerm },           // Forward reference should work
		Application { func: TypedTerm, arg: TypedTerm }, // Multiple forward references
	};

	// Base enum for composition
	enum CoreAtoms = {
		Variable { name: String },
		Level0,
	};
}

#[test]
fn test_forward_references() {
	// Test that we can create instances using forward references
	let var = CoreAtoms::Variable { name: "x".to_string() };
	let term: TypedTerm = var.into();

	// Test self-reference (Vec<TypedTerm>)
	let tuple = TypedTerm::TupleCons { elements: vec![term] };

	// Test forward reference (TypedTerm -> TypedTermComplex)
	let lambda = TypedTermComplex::Lambda { body: tuple };

	// Test boxing (Box<TypedTermComplex> in TypedTerm)
	let boxed_term = TypedTerm::TypedTermComplex(Box::new(lambda));

	match &boxed_term {
		TypedTerm::TypedTermComplex(complex) => match complex.as_ref() {
			TypedTermComplex::Lambda { body } => match body {
				TypedTerm::TupleCons { elements } => {
					assert_eq!(elements.len(), 1);
				}
				_ => panic!("Expected TupleCons"),
			},
			_ => panic!("Expected Lambda"),
		},
		_ => panic!("Expected TypedTermComplex"),
	}
}

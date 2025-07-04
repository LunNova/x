// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test union composition syntax with type references and inline variants

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Define some base enums first
	enum CoreAtoms = {
		BoundVariable { index: i64, debug: String },
		FreeVariable { id: usize },
		Level0,
		LevelType,
		StringLiteral { value: String },
	};

	enum TypeConstructors = {
		Pi { param_type: Box<String>, result_type: Box<String> },
		Star { level: i64 },
		TupleType { desc: Box<String> },
	};

	enum TypedTermComplex = {
		Lambda { param_name: String, body: String },
		Application { func: String, arg: String },
		Let { name: String, expr: String, body: String },
	};

	// Test union composition with the new RFC syntax
	enum InferrableTerm = CoreAtoms |
		TypeConstructors |
		{
			Annotated { term: Box<String>, type_ann: Box<String> },
			Application { func: Box<String>, arg: Box<String> },
			Let { name: String, expr: Box<String>, body: Box<String> },
		};

	// Test with boxed type references
	enum TypedTerm =
		CoreAtoms |
		TypeConstructors |
		Box<TypedTermComplex> |
		{
			Literal { value: Box<String> },
			TupleCons { elements: Vec<String> },
			EnumCons { constructor: String, arg: Box<String> },
		};

	// Test pure union (no inline variants)
	enum FlexValue = StrictValue | StuckValue;

	enum StrictValue = {
		HostValue { value: String },
		TupleValue { elements: Vec<String> },
	};

	enum StuckValue = {
		StuckVar { id: usize },
		Application { func: Box<String>, arg: Box<String> },
	};
}

#[test]
fn test_union_composition_syntax() {
	// Test CoreAtoms conversion
	let atom = CoreAtoms::Level0;
	let inferrable: InferrableTerm = atom.into();
	match &inferrable {
		InferrableTerm::CoreAtoms(CoreAtoms::Level0) => {}
		_ => panic!("Expected CoreAtoms::Level0, got {:?}", inferrable),
	}

	// Test TypeConstructors conversion
	let ty_cons = TypeConstructors::Star { level: 0 };
	let inferrable2: InferrableTerm = ty_cons.into();
	match &inferrable2 {
		InferrableTerm::TypeConstructors(TypeConstructors::Star { level: 0 }) => {}
		_ => panic!("Expected TypeConstructors::Star {{ level: 0 }}, got {:?}", inferrable2),
	}

	// Test inline variant creation
	let annotated = InferrableTerm::Annotated {
		term: Box::new("term".to_string()),
		type_ann: Box::new("type".to_string()),
	};
	match &annotated {
		InferrableTerm::Annotated { term, type_ann } => {
			assert_eq!(term.as_str(), "term");
			assert_eq!(type_ann.as_str(), "type");
		}
		_ => panic!("Expected Annotated variant, got {:?}", annotated),
	}

	// Test boxed type reference
	let complex = TypedTermComplex::Lambda {
		param_name: "x".to_string(),
		body: "body".to_string(),
	};
	let typed: TypedTerm = TypedTerm::TypedTermComplex(Box::new(complex));
	match &typed {
		TypedTerm::TypedTermComplex(boxed) => match boxed.as_ref() {
			TypedTermComplex::Lambda { param_name, body } => {
				assert_eq!(param_name, "x");
				assert_eq!(body, "body");
			}
			_ => panic!("Expected Lambda variant"),
		},
		_ => panic!("Expected TypedTermComplex variant, got {:?}", typed),
	}

	// Test pure union
	let strict = StrictValue::HostValue { value: "test".to_string() };
	let flex: FlexValue = strict.into();
	match &flex {
		FlexValue::StrictValue(StrictValue::HostValue { value }) => {
			assert_eq!(value, "test");
		}
		_ => panic!("Expected StrictValue::HostValue, got {:?}", flex),
	}
}

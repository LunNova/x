// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test ADT composition with union syntax

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Basic enum definition
	enum CoreAtoms = {
		BoundVariable { index: i64, debug: String },
		FreeVariable { id: usize },
		Level0,
		LevelType,
	};

	enum TypeConstructors = {
		Pi { param_type: Box<String>, result_type: Box<String> },
		Star { level: i64 },
	};

	// Union syntax with |
	enum InferrableTerm = CoreAtoms | TypeConstructors | {
		Annotated { term: Box<String>, type_ann: Box<String> },
		Application { func: Box<String>, arg: Box<String> },
	};

	// Test with Box<T>
	enum TypedTerm = CoreAtoms | Box<TypeConstructors> | {
		Literal { value: String },
	};
}

#[test]
fn test_union_syntax() {
	// Test union composition works correctly
	let atom = CoreAtoms::Level0;
	let inferrable: InferrableTerm = atom.into();
	match &inferrable {
		InferrableTerm::CoreAtoms(CoreAtoms::Level0) => {}
		_ => panic!("Expected CoreAtoms::Level0, got {:?}", inferrable),
	}

	let pi = TypeConstructors::Pi {
		param_type: Box::new("A".to_string()),
		result_type: Box::new("B".to_string()),
	};
	let inferrable2: InferrableTerm = pi.into();
	match &inferrable2 {
		InferrableTerm::TypeConstructors(TypeConstructors::Pi { param_type, result_type }) => {
			assert_eq!(param_type.as_str(), "A");
			assert_eq!(result_type.as_str(), "B");
		}
		_ => panic!("Expected TypeConstructors::Pi, got {:?}", inferrable2),
	}

	// Test inline variant
	let app = InferrableTerm::Application {
		func: Box::new("f".to_string()),
		arg: Box::new("x".to_string()),
	};
	match &app {
		InferrableTerm::Application { func, arg } => {
			assert_eq!(func.as_str(), "f");
			assert_eq!(arg.as_str(), "x");
		}
		_ => panic!("Expected Application variant, got {:?}", app),
	}

	// Test boxed type reference
	let star = TypeConstructors::Star { level: 0 };
	let typed: TypedTerm = TypedTerm::TypeConstructors(Box::new(star));
	match &typed {
		TypedTerm::TypeConstructors(boxed) => match boxed.as_ref() {
			TypeConstructors::Star { level: 0 } => {}
			_ => panic!("Expected Star {{ level: 0 }}, got {:?}", boxed),
		},
		_ => panic!("Expected TypeConstructors variant, got {:?}", typed),
	}
}

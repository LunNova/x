// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

pattern_wishcast::pattern_wishcast! {
	// Base expressions that can get stuck during evaluation
	enum StuckEvaluation = {
		Var { id: usize },
		// TODO: Is there a good way to express constraints on eventual value eg that arg is a tuple iff not stuck?
		Application { func: Box<PartialValue>, arg: Box<PartialValue> },
		UnboundVariable { name: String },
	};

	// Our main value type with pattern-based strictness
	enum Value is <P: PatternFields> = StuckEvaluation | {
		// Basic values that are always fully evaluated
		Number { value: i32 },
		Boolean { value: bool },
		Text { value: String },

		// Compound values - completeness depends on contents
		// Self -> Value<P>, recursively preserves applied pattern
		// including checking before conversion in generated downcast
		Tuple { elements: Vec<Self> },
		Function {
			param: String,
			body: Box<Self>,
			// Without unsafe_transmute_check line SubtypingRelation will error
			// as it doesn't know how to find the Self instances it can see in the generic
			// SAFETY: Must iterate over HashMap values to check pattern compliance
			#[unsafe_transmute_check(iter = ".values()")]
			captured_env: std::collections::HashMap<String, Self>
		},
	};

	// Complete values: guaranteed to be fully evaluated, no stuck states
	type CompleteValue = Value is Number { .. } | Boolean { .. } | Text { .. } | Tuple { .. } | Function { .. };

	// with real pattern types we wouldn't need to explicitly make an alias with a wildcard
	// load bearing here because we need to generate traits that make all the _never inhabited
	type PartialValue = Value is _;

	// No real subtyping but we can pretend by generating upcast and try downcast impls
	// With real pattern types in rustc no need to declare anything like this
	// CompleteValue would be a subtype of PartialValue just from specifying predicates that imply that relation
	#[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_complete))]
	impl CompleteValue : PartialValue;
}

#[test]
fn test_offsets_dont_vary_all_inhabited_variants() {
	// we only test variants that are always inhabited
	use std::collections::HashMap;

	fn assert_same_field_offset<T, S, U, V>(complete_field: &T, partial_field: &S, complete_value: &U, partial_value: &V, field_name: &str) {
		let complete_offset = (complete_field as *const _ as isize) - (complete_value as *const _ as isize);
		let partial_offset = (partial_field as *const _ as isize) - (partial_value as *const _ as isize);

		assert_eq!(
			complete_offset, partial_offset,
			"{} field offset differs between CompleteValue and PartialValue",
			field_name
		);
	}

	let complete_number = CompleteValue::Number { value: 42 };
	let partial_number = PartialValue::Number { value: 42 };

	match (&complete_number, &partial_number) {
		(CompleteValue::Number { value: c_val }, PartialValue::Number { value: p_val }) => {
			assert_same_field_offset(c_val, p_val, &complete_number, &partial_number, "Number::value");
		}
		_ => unreachable!(),
	}

	let complete_bool = CompleteValue::Boolean { value: true };
	let partial_bool = PartialValue::Boolean { value: true };

	match (&complete_bool, &partial_bool) {
		(CompleteValue::Boolean { value: c_val }, PartialValue::Boolean { value: p_val }) => {
			assert_same_field_offset(c_val, p_val, &complete_bool, &partial_bool, "Boolean::value");
		}
		_ => unreachable!(),
	}

	let complete_text = CompleteValue::Text {
		value: "hello".to_string(),
	};
	let partial_text = PartialValue::Text {
		value: "hello".to_string(),
	};

	match (&complete_text, &partial_text) {
		(CompleteValue::Text { value: c_val }, PartialValue::Text { value: p_val }) => {
			assert_same_field_offset(c_val, p_val, &complete_text, &partial_text, "Text::value");
		}
		_ => unreachable!(),
	}

	let complete_tuple = CompleteValue::Tuple {
		elements: vec![CompleteValue::Number { value: 1 }],
	};
	let partial_tuple = PartialValue::Tuple {
		elements: vec![PartialValue::Number { value: 1 }],
	};

	match (&complete_tuple, &partial_tuple) {
		(CompleteValue::Tuple { elements: c_elems }, PartialValue::Tuple { elements: p_elems }) => {
			assert_same_field_offset(c_elems, p_elems, &complete_tuple, &partial_tuple, "Tuple::elements");
		}
		_ => unreachable!(),
	}

	let mut complete_env = HashMap::new();
	complete_env.insert("x".to_string(), CompleteValue::Number { value: 10 });
	let mut partial_env = HashMap::new();
	partial_env.insert("x".to_string(), PartialValue::Number { value: 10 });

	let complete_function = CompleteValue::Function {
		param: "y".to_string(),
		body: Box::new(CompleteValue::Number { value: 42 }),
		captured_env: complete_env,
	};
	let partial_function = PartialValue::Function {
		param: "y".to_string(),
		body: Box::new(PartialValue::Number { value: 42 }),
		captured_env: partial_env,
	};

	match (&complete_function, &partial_function) {
		(
			CompleteValue::Function {
				param: c_param,
				body: c_body,
				captured_env: c_env,
			},
			PartialValue::Function {
				param: p_param,
				body: p_body,
				captured_env: p_env,
			},
		) => {
			assert_same_field_offset(c_param, p_param, &complete_function, &partial_function, "Function::param");
			assert_same_field_offset(c_body, p_body, &complete_function, &partial_function, "Function::body");
			assert_same_field_offset(c_env, p_env, &complete_function, &partial_function, "Function::captured_env");
		}
		_ => unreachable!(),
	}
}

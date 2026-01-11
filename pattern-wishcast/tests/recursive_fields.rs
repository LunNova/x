// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

//! Test recursive field types: Option<Box<Self>>, Vec<Box<Self>>, Box<Self>
//! These exercise field_checking.rs paths for container types holding Value types
//!
//! The Self type parameter means containers hold the same strictness level as parent.
//! So StrictValue's Box<Self> is Box<StrictValue>, and FlexValue's is Box<FlexValue>.

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		// Unit variant for simple construction
		Unit,
		// Test Option<Box<Self>> field checking
		MaybeValue { inner: Option<Box<Self>> },
		// Test Vec<Box<Self>> field checking
		ListOfValues { items: Vec<Box<Self>> },
		// Test Box<Self> field checking
		BoxedValue { boxed: Box<Self> },
		// Conditional variant (excluded from StrictValue)
		Stuck { reason: String },
	};

	type FlexValue = Value is _;
	type StrictValue = Value is Unit | MaybeValue { .. } | ListOfValues { .. } | BoxedValue { .. };

	#[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
	impl StrictValue : FlexValue;
}

#[test]
fn test_box_self_field() {
	// Test with boxed strict value
	let boxed = StrictValue::BoxedValue {
		boxed: Box::new(StrictValue::Unit),
	};
	let flex = boxed.to_flex();
	assert!(flex.try_to_strict().is_ok(), "Boxed strict value should convert");

	// Test with boxed stuck value (should fail)
	let stuck = FlexValue::Stuck {
		reason: "blocked".to_string(),
		_never: (),
	};
	let boxed_stuck = FlexValue::BoxedValue { boxed: Box::new(stuck) };
	assert!(
		boxed_stuck.try_to_strict().is_err(),
		"Box<Self> containing Stuck should fail conversion"
	);
}

#[test]
fn test_deeply_nested_structures() {
	// Deeply nested: BoxedValue -> MaybeValue -> ListOfValues -> Unit
	let deep = StrictValue::BoxedValue {
		boxed: Box::new(StrictValue::MaybeValue {
			inner: Some(Box::new(StrictValue::ListOfValues {
				items: vec![Box::new(StrictValue::Unit)],
			})),
		}),
	};

	let flex = deep.to_flex();
	assert!(flex.try_to_strict().is_ok(), "Deeply nested strict values should convert");

	// Same structure but with Stuck deeply nested - must construct as FlexValue
	let stuck = FlexValue::Stuck {
		reason: "deep".to_string(),
		_never: (),
	};
	let deep_stuck = FlexValue::BoxedValue {
		boxed: Box::new(FlexValue::MaybeValue {
			inner: Some(Box::new(FlexValue::ListOfValues {
				items: vec![Box::new(stuck)],
			})),
		}),
	};
	assert!(deep_stuck.try_to_strict().is_err(), "Deeply nested Stuck should fail conversion");
}

#[test]
fn test_option_box_self_field() {
	// Test with Some containing a strict value - construct entirely as StrictValue
	let with_some = StrictValue::MaybeValue {
		inner: Some(Box::new(StrictValue::Unit)),
	};
	let flex = with_some.to_flex();
	assert!(flex.try_to_strict().is_ok(), "Should convert back to strict");

	// Test with None
	let with_none = StrictValue::MaybeValue { inner: None };
	let flex_none = with_none.to_flex();
	assert!(flex_none.try_to_strict().is_ok(), "None should convert to strict");

	// Test with Some containing a stuck value (should fail)
	// Must construct as FlexValue since Stuck is not in StrictValue
	let stuck_inner = FlexValue::Stuck {
		reason: "blocked".to_string(),
		_never: (),
	};
	let with_stuck = FlexValue::MaybeValue {
		inner: Some(Box::new(stuck_inner)),
	};
	assert!(
		with_stuck.try_to_strict().is_err(),
		"Option<Box<Self>> containing Stuck should fail conversion"
	);
}

#[test]
fn test_vec_box_self_field() {
	// Test with empty vec
	let empty_list = StrictValue::ListOfValues { items: vec![] };
	let flex = empty_list.to_flex();
	assert!(flex.try_to_strict().is_ok(), "Empty vec should convert to strict");

	// Test with vec of strict values - all must be StrictValue
	let list = StrictValue::ListOfValues {
		items: vec![Box::new(StrictValue::Unit), Box::new(StrictValue::Unit)],
	};
	let flex = list.to_flex();
	assert!(flex.try_to_strict().is_ok(), "Vec of strict values should convert");

	// Test with vec containing a stuck value (should fail)
	let stuck = FlexValue::Stuck {
		reason: "blocked".to_string(),
		_never: (),
	};
	let list_with_stuck = FlexValue::ListOfValues {
		items: vec![Box::new(FlexValue::Unit), Box::new(stuck)],
	};
	assert!(
		list_with_stuck.try_to_strict().is_err(),
		"Vec<Box<Self>> containing Stuck should fail conversion"
	);
}

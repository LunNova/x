// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that basic supported pattern syntax works correctly

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		Number { value: i32 },
		Text { content: String },
		Flag,
		DebugInfo,
	};

	// These should all work fine:
	type BasicPatterns = Value is Number { .. } | Text { .. } | Flag;
	type TuplePatterns = Value is Number(_) | Text(_) | Flag;
	type WildcardPattern = Value is _;

	#[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
	impl BasicPatterns : WildcardPattern;
}

#[test]
fn test_basic_pattern_syntax() {
	// Test that all the pattern syntaxes compile and work
	let num = BasicPatterns::Number { value: 42 };
	let flex = num.to_flex();

	match flex.try_to_strict() {
		Ok(_) => {} // Expected - should convert back
		Err(_) => panic!("Should be able to convert back to strict"),
	}
}

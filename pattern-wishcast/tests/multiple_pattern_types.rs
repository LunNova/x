// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

//! Test multiple restrictive pattern types where conditional variants are
//! included in some patterns but not others.
//!
//! This exercises patterns.rs:84 - the branch where a conditional variant
//! IS explicitly included in a pattern's variant list.

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		// Always included in all patterns
		Core,
		// Included in PatternA but not PatternB
		OnlyA,
		// Included in PatternB but not PatternA
		OnlyB,
		// Included in neither (always conditional)
		Neither,
	};

	// PatternA includes Core and OnlyA, excludes OnlyB and Neither
	type PatternA = Value is Core | OnlyA;

	// PatternB includes Core and OnlyB, excludes OnlyA and Neither
	type PatternB = Value is Core | OnlyB;

	// FlexValue includes everything
	type FlexValue = Value is _;

	#[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_pattern_a))]
	impl PatternA : FlexValue;

	#[derive(SubtypingRelation(upcast=to_flex_b, downcast=try_to_pattern_b))]
	impl PatternB : FlexValue;
}

#[test]
fn test_core_works_in_both() {
	let core_a = PatternA::Core;
	let flex_a = core_a.to_flex();
	assert!(flex_a.clone().try_to_pattern_a().is_ok());
	assert!(flex_a.try_to_pattern_b().is_ok());

	let core_b = PatternB::Core;
	let flex_b = core_b.to_flex_b();
	assert!(flex_b.clone().try_to_pattern_a().is_ok());
	assert!(flex_b.try_to_pattern_b().is_ok());
}

#[test]
fn test_neither_fails_both() {
	let neither = FlexValue::Neither { _never: () };
	assert!(neither.clone().try_to_pattern_a().is_err());
	assert!(neither.try_to_pattern_b().is_err());
}

#[test]
fn test_pattern_a_allows_only_a() {
	// OnlyA is conditional (excluded from PatternB), so it has _never field
	let a = PatternA::OnlyA { _never: () };
	let flex = a.to_flex();
	assert!(flex.clone().try_to_pattern_a().is_ok());
	assert!(flex.try_to_pattern_b().is_err(), "OnlyA should not convert to PatternB");
}

#[test]
fn test_pattern_b_allows_only_b() {
	// OnlyB is conditional (excluded from PatternA), so it has _never field
	let b = PatternB::OnlyB { _never: () };
	let flex = b.to_flex_b();
	assert!(flex.clone().try_to_pattern_b().is_ok());
	assert!(flex.try_to_pattern_a().is_err(), "OnlyB should not convert to PatternA");
}

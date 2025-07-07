// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that we get a helpful error when no conditional variants exist

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		HostValue { value: String },
		TupleValue { elements: Vec<Self> },
	};

	type FlexValue = Value is _;
	type StrictValue = Value is HostValue(_) | TupleValue(_);

	#[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
	impl StrictValue : FlexValue;
}

fn main() {}

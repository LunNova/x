// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that unsupported generic fields containing Self produce helpful errors

use pattern_wishcast::pattern_wishcast;
use std::collections::HashMap;

pattern_wishcast! {
	enum StuckEvaluation = {
		Var { id: usize },
	};

	enum Value is <P: PatternFields> = StuckEvaluation | {
		HostValue { value: String },
		// This should produce a compile error:
		BadField { data: HashMap<String, Self> },
	};

	type FlexValue = Value is _;
	type StrictValue = Value is HostValue(_) | BadField(_);

	#[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
	impl StrictValue : FlexValue;
}

fn main() {}

// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that referencing a non-existent variant in a type alias pattern produces an error.

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		Number { value: i32 },
		Text { value: String },
	};

	// Bug: NonExistent doesn't exist but this doesn't error
	type BrokenValue = Value is Number { .. } | NonExistent { .. };

	type PartialValue = Value is _;

	#[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_broken))]
	impl BrokenValue : PartialValue;
}

fn main() {}

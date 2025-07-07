// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that range patterns produce helpful error messages

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		Number { value: i32 },
		Flag,
	};

	// Range patterns should produce helpful error messages:
	type SmallNumbers = Value is Number { value: 1..10 };
}

fn main() {}

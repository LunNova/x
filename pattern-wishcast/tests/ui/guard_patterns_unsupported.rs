// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that guard patterns produce helpful error messages

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		Number { value: i32 },
		Flag,
	};

	// Guard patterns should produce helpful error messages:
	type PositiveNumbers = Value is Number { value } if value > 0;
}

fn main() {}

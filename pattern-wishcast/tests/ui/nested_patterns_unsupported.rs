// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that nested patterns produce helpful error messages

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		Text { content: String },
		Flag,
	};

	// Nested patterns should produce helpful error messages:
	type SpecificText = Value is Text { content: "hello" | "world" };
}

fn main() {}

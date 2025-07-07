// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test error when enum declares pattern support but no pattern types are defined

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		HostValue { value: String },
		TupleValue { elements: Vec<Self> },
	};

	// Oops, forgot to define any pattern types!
}

fn main() {}

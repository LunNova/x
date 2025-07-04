// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test helpful error for single pattern type with no conditional variants

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum Value is <P: PatternFields> = {
		HostValue { value: String },
		TupleValue { elements: Vec<Self> },
	};

	// Only one pattern type, and it includes all variants
	type MyValue = Value is HostValue(_) | TupleValue(_);
}

fn main() {}

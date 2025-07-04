// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that creating pattern types for enums without pattern support produces helpful errors

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// This enum doesn't declare pattern support
	enum Value = {
		HostValue { value: String },
		TupleValue { elements: Vec<Self> },
	};

	// This should produce an error:
	type StrictValue = Value is HostValue(_);
}

fn main() {}

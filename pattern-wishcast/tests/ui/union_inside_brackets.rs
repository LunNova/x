// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that unsupported generic fields containing Self produce helpful errors

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	enum IncorrectlyInsideBracket = { None };
	enum StuckEvaluation = {
		IncorrectlyInsideBracket |
		{
			Var { id: usize },
		}
	};
}

fn main() {}

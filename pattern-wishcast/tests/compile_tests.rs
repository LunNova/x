// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

#[test]
#[cfg_attr(miri, ignore)]
fn ui() {
	let t = trybuild::TestCases::new();
	t.compile_fail("tests/ui/*.rs");
}

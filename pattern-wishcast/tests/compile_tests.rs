// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

#[test]
fn ui() {
	let t = trybuild::TestCases::new();
	t.compile_fail("tests/ui/*.rs");
}

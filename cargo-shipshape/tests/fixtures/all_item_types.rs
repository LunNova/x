// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

fn my_function() {}

struct MyStruct;

enum MyEnum { A, B }

trait MyTrait {}

const MY_CONST: i32 = 1;

static MY_STATIC: i32 = 2;

type MyAlias = i32;

macro_rules! my_macro {
    () => {};
}

macro my_macro_2 {}

use std::collections::HashMap;

mod external_mod;

extern crate std;

union MyUnion { a: i32, b: u32 }

impl MyStruct {}

mod inline_mod {
    fn inner() {}
}

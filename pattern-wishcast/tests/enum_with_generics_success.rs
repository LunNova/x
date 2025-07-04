// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that enums with regular generics work properly

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Enum with a regular generic parameter
	enum Result<T, E> = {
		Ok { value: T },
		Err { error: E },
	};

	// This should work - creating a concrete type
	type StringResult = Result<String, String>;
}

fn main() {
	// This should compile and work
	let ok_result = StringResult::Ok {
		value: "Success".to_string(),
	};
	let err_result = StringResult::Err {
		error: "Failed".to_string(),
	};

	for res in [ok_result, err_result] {
		match res {
			StringResult::Ok { value } => println!("Got: {}", value),
			StringResult::Err { error } => println!("Error: {}", error),
		}
	}
}

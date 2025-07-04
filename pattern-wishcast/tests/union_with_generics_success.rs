// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! Test ADT composition with generics

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Generic enum definition
	enum Container<T> = {
		Empty,
		Some { value: T },
		Many { values: Vec<T> },
	};

	// Union with generics
	enum MyResult<T, E> = Container<T> | {
		Error { error: E },
	};
}

#[test]
fn test_union_with_generics() {
	// Test generic enum
	let empty: Container<i32> = Container::Empty;
	match &empty {
		Container::Empty => {}
		_ => panic!("Expected Container::Empty, got {:?}", empty),
	}

	let some = Container::Some { value: 42 };
	match &some {
		Container::Some { value } => {
			assert_eq!(*value, 42);
		}
		_ => panic!("Expected Container::Some {{ value: 42 }}, got {:?}", some),
	}

	// Test union with generics - Container<T> becomes a single variant in MyResult
	let container_val = Container::Some {
		value: "hello".to_string(),
	};
	let ok: MyResult<String, &str> = container_val.into();
	match &ok {
		MyResult::Container(Container::Some { value }) => {
			assert_eq!(value, "hello");
		}
		_ => panic!("Expected MyResult::Container(Some {{ value: \"hello\" }}), got {:?}", ok),
	}

	let err: MyResult<String, &str> = MyResult::Error { error: "failed" };
	match &err {
		MyResult::Error { error } => {
			assert_eq!(*error, "failed");
		}
		_ => panic!("Expected MyResult::Error {{ error: \"failed\" }}, got {:?}", err),
	}

	// Test From trait with generics
	let container = Container::Many { values: vec![1, 2, 3] };
	let result: MyResult<i32, String> = container.into();
	match &result {
		MyResult::Container(Container::Many { values }) => {
			assert_eq!(values, &vec![1, 2, 3]);
		}
		_ => panic!("Expected MyResult::Container(Many {{ values: [1, 2, 3] }}), got {:?}", result),
	}
}

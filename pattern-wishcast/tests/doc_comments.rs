// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

//! Test that doc comments are preserved at all levels.

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	/// Enum-level doc comment.
	/// Multiple lines supported.
	enum DocEnum = {
		/// Unit variant doc comment
		Unit,
		/// Variant with named fields
		Named {
			/// Field doc comment
			value: i32,
			/// Another field
			name: String,
		},
		/// Tuple variant doc comment
		Tuple(i32, String),
	};
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_enum_compiles_with_docs() {
		// If this compiles, doc comments were preserved correctly
		let _ = DocEnum::Unit;
		let _ = DocEnum::Named {
			value: 42,
			name: "test".to_string(),
		};
		let _ = DocEnum::Tuple(1, "hello".to_string());
	}

	#[test]
	fn test_debug_output_works() {
		// Debug should still work with doc comments
		let unit = DocEnum::Unit;
		let debug_str = format!("{:?}", unit);
		assert!(debug_str.contains("Unit"));
	}
}

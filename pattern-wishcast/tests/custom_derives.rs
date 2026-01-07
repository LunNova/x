// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

use pattern_wishcast::pattern_wishcast;
use std::collections::HashSet;

pattern_wishcast! {
	// Custom derives: Debug, Clone, PartialEq, Eq, Hash
	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
	enum WithCustomDerives = {
		Unit,
		WithField { value: i32 },
	};

	// Default derives (Debug, Clone) - no explicit #[derive]
	enum WithDefaultDerives = {
		Unit,
		WithField { value: i32 },
	};
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_custom_derives_equality() {
		let a = WithCustomDerives::Unit;
		let b = WithCustomDerives::Unit;
		let c = WithCustomDerives::WithField { value: 42 };

		assert_eq!(a, b);
		assert_ne!(a, c);
	}

	#[test]
	fn test_custom_derives_hash() {
		let mut set = HashSet::new();
		set.insert(WithCustomDerives::Unit);
		set.insert(WithCustomDerives::WithField { value: 42 });
		set.insert(WithCustomDerives::Unit); // duplicate

		assert_eq!(set.len(), 2);
	}

	#[test]
	fn test_default_derives_debug_clone() {
		let a = WithDefaultDerives::WithField { value: 42 };
		let b = a.clone();

		// Debug works
		let _ = format!("{:?}", b);

		// Can't test PartialEq since it's not derived
	}
}

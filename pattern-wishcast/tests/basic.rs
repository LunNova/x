// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use pattern_wishcast::pattern_wishcast;

// First, define some base types
// <generated by cargo-derive-doc>
// Macro expansions:
//   impl  From <Literal> for StrictValue
// </generated by cargo-derive-doc>
#[derive(Debug, Clone)]
pub struct Literal {
	pub value: i32,
}

// <generated by cargo-derive-doc>
// Macro expansions:
//   impl  From <Variable> for StuckValue
// </generated by cargo-derive-doc>
#[derive(Debug, Clone)]
pub struct Variable {
	pub name: String,
}

// Now compose them into larger types
// <generated by cargo-derive-doc>
// Macro expansions:
//   pub enum StrictValue
//   impl  From <Literal> for StrictValue
//   pub enum StuckValue
//   impl  From <Variable> for StuckValue
//   pub enum FlexValue
//   impl  From <StrictValue> for FlexValue
//   impl  From <StuckValue> for FlexValue
//   pub enum Term
//   impl  From <FlexValue> for Term
//   pub fn main () -> ()
// </generated by cargo-derive-doc>
pattern_wishcast! {
	// StrictValue can be a literal or a lambda
	enum StrictValue = Literal | { Lambda { param: String, body: Box<Term> } };

	// StuckValue is when we're blocked on a variable
	enum StuckValue = Variable | { Apply { func: Box<StuckValue>, arg: Box<FlexValue> } };

	// FlexValue is either strict or stuck
	enum FlexValue = StrictValue | StuckValue;

	// A term can be any flex value
	enum Term = FlexValue;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_composition() {
		// Create a literal
		let lit = Literal { value: 42 };
		let strict: StrictValue = lit.into();
		let flex: FlexValue = strict.into();
		let term: Term = flex.into();

		// Create a variable (stuck value)
		let var = Variable { name: "x".to_string() };
		let stuck: StuckValue = var.into();
		let _flex2: FlexValue = stuck.into();

		// Pattern matching works as expected
		match term {
			Term::FlexValue(FlexValue::StrictValue(StrictValue::Literal(lit))) => {
				assert_eq!(lit.value, 42);
			}
			_ => panic!("Expected literal"),
		}
	}

	#[test]
	fn test_inline_variants() {
		let lambda = StrictValue::Lambda {
			param: "x".to_string(),
			body: Box::new(Term::FlexValue(FlexValue::StuckValue(StuckValue::Variable(Variable {
				name: "x".to_string(),
			})))),
		};

		match lambda {
			StrictValue::Lambda { param, body: _ } => {
				assert_eq!(param, "x");
			}
			_ => panic!("Expected lambda"),
		}
	}
}

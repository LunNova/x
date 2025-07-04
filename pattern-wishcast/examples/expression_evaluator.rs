// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! # Expression Evaluator Example
//!
//! This example demonstrates using pattern-wishcast to build a tiny evaluator.
//! Pattern-wishcast provides a general pattern types system - this evaluator is just
//! one example of what you can build with it.
//!
//! In this evaluator:
//! - Complete expressions are fully evaluated values
//! - Partial expressions might contain stuck evaluation states
//! - Pattern types ensure safe upcasts and checked downcasts

#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Base expressions that can get stuck during evaluation
	enum StuckEvaluation = {
		Var { id: usize },
		// TODO: Is there a good way to express constraints on eventual value eg that arg is a tuple iff not stuck?
		Application { func: Box<PartialValue>, arg: Box<PartialValue> },
		UnboundVariable { name: String },
	};

	// Our main value type with pattern-based strictness
	enum Value is <P: PatternFields> = StuckEvaluation | {
		// Basic values that are always fully evaluated
		Number { value: i32 },
		Boolean { value: bool },
		Text { value: String },

		// Compound values - completeness depends on contents
		// Self -> Value<P>, recursively preserves applied pattern
		// including checking before conversion in generated downcast
		Tuple { elements: Vec<Self> },
		Function {
			param: String,
			body: Box<Self>,
			// Without unsafe_transmute_check line SubtypingRelation will error
			// as it doesn't know how to find the Self instances it can see in the generic
			// SAFETY: Must iterate over HashMap values to check pattern compliance
			#[unsafe_transmute_check(iter = ".values()")]
			captured_env: std::collections::HashMap<String, Self>
		},
	};

	// Complete values: guaranteed to be fully evaluated, no stuck states
	type CompleteValue = Value is Number { .. } | Boolean { .. } | Text { .. } | Tuple { .. } | Function { .. };

	// with real pattern types we wouldn't need to explicitly make an alias with a wildcard
	// load bearing here because we need to generate traits that make all the _never inhabited
	type PartialValue = Value is _;

	// No real subtyping but we can pretend by generating upcast and try downcast impls
	// With real pattern types in rustc no need to declare anything like this
	// CompleteValue would be a subtype of PartialValue just from specifying predicates that imply that relation
	#[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_complete))]
	impl CompleteValue : PartialValue;
}

/// Evaluation context for variable lookup
#[derive(Debug, Clone)]
pub struct EvalContext {
	variables: std::collections::HashMap<String, PartialValue>,
}

impl EvalContext {
	pub fn new() -> Self {
		let mut ctx = Self {
			variables: std::collections::HashMap::new(),
		};

		// Add built-in functions as special function values
		ctx.add_builtin("add".to_string());
		ctx.add_builtin("sub".to_string());
		ctx.add_builtin("mul".to_string());

		ctx
	}

	fn add_builtin(&mut self, name: String) {
		// Create a builtin function marker
		let builtin = PartialValue::Function {
			param: format!("builtin_{}", name),
			body: Box::new(PartialValue::unbound_var("builtin_implementation".to_string())),
			captured_env: std::collections::HashMap::new(),
		};
		self.variables.insert(name, builtin);
	}

	pub fn bind(&mut self, name: String, value: PartialValue) {
		self.variables.insert(name, value);
	}

	pub fn lookup(&self, name: &str) -> Option<&PartialValue> {
		self.variables.get(name)
	}
}

impl std::fmt::Display for EvalContext {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut sorted_vars: Vec<_> = self.variables.iter().collect();
		sorted_vars.sort_by_key(|(name, _)| *name);

		writeln!(f, "{{")?;
		for (name, value) in sorted_vars {
			writeln!(f, "  {} = {}", name, value)?;
		}
		write!(f, "}}")
	}
}

impl<P: PatternFields> std::fmt::Display for Value<P> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Value::Number { value } => write!(f, "{}", value),
			Value::Boolean { value } => write!(f, "{}", if *value { "true" } else { "false" }),
			Value::Text { value } => write!(f, "\"{}\"", value),
			Value::Tuple { elements } => {
				write!(f, "(")?;
				for (i, elem) in elements.iter().enumerate() {
					if i > 0 {
						write!(f, ", ")?;
					}
					write!(f, "{}", elem)?;
				}
				write!(f, ")")
			}
			Value::Function { param, .. } => {
				if param.starts_with("builtin_") {
					let builtin_name = param.strip_prefix("builtin_").unwrap();
					write!(f, "⟨builtin: {}⟩", builtin_name)
				} else {
					write!(f, "λ{}.⟨function⟩", param)
				}
			}
			Value::StuckEvaluation(stuck, _) => match stuck {
				StuckEvaluation::Var { id } => write!(f, "?{}", id),
				StuckEvaluation::UnboundVariable { name } => write!(f, "⊥{}", name),
				StuckEvaluation::Application { func, arg } => {
					write!(f, "({}{})", func, arg)
				}
			},
		}
	}
}

impl CompleteValue {
	/// Create basic complete values
	pub fn number(n: i32) -> Self {
		CompleteValue::Number { value: n }
	}

	pub fn boolean(b: bool) -> Self {
		CompleteValue::Boolean { value: b }
	}

	pub fn text(s: String) -> Self {
		CompleteValue::Text { value: s }
	}

	pub fn tuple(elements: Vec<CompleteValue>) -> Self {
		// Convert each complete value to Self (which preserves completeness)
		let complete_elements: Vec<CompleteValue> = elements.into_iter().map(|elem| elem).collect();
		CompleteValue::Tuple {
			elements: complete_elements,
		}
	}

	/// Extract the numeric value if this is a number
	pub fn as_number(&self) -> Option<i32> {
		match self {
			CompleteValue::Number { value } => Some(*value),
			_ => None,
		}
	}

	/// Extract the boolean value if this is a boolean
	pub fn as_boolean(&self) -> Option<bool> {
		match self {
			CompleteValue::Boolean { value } => Some(*value),
			_ => None,
		}
	}
}

impl PartialValue {
	/// Create a stuck variable reference
	pub fn stuck_var(id: usize) -> Self {
		PartialValue::StuckEvaluation(StuckEvaluation::Var { id }, ())
	}

	/// Create an unbound variable error
	pub fn unbound_var(name: String) -> Self {
		PartialValue::StuckEvaluation(StuckEvaluation::UnboundVariable { name }, ())
	}

	/// Try to convert a list of arguments to complete values
	fn try_to_complete_args(args: Vec<PartialValue>) -> Result<Vec<CompleteValue>, Vec<PartialValue>> {
		// FIXME: pattern-wishcast should expose a better API for safely doing this
		let mut complete_args = Vec::new();
		let mut remaining_args = args.into_iter();

		for arg in remaining_args.by_ref() {
			match arg.try_to_complete() {
				Ok(complete) => complete_args.push(complete),
				Err(partial) => {
					// Convert completed args back to partial and combine with remaining
					let mut result: Vec<PartialValue> = complete_args.into_iter().map(|complete| complete.to_partial()).collect();
					result.push(partial);
					result.extend(remaining_args);
					return Err(result);
				}
			}
		}
		Ok(complete_args)
	}

	/// Apply a builtin function to arguments
	fn apply_builtin(name: &str, builtin_func: PartialValue, args: Vec<PartialValue>) -> PartialValue {
		match Self::try_to_complete_args(args) {
			Ok(complete_args) => match name {
				"add" if complete_args.len() == 2 => {
					if let (Some(x), Some(y)) = (complete_args[0].as_number(), complete_args[1].as_number()) {
						CompleteValue::number(x + y).to_partial()
					} else {
						PartialValue::unbound_var("type_error".to_string())
					}
				}
				"sub" if complete_args.len() == 2 => {
					if let (Some(x), Some(y)) = (complete_args[0].as_number(), complete_args[1].as_number()) {
						CompleteValue::number(x - y).to_partial()
					} else {
						PartialValue::unbound_var("type_error".to_string())
					}
				}
				"mul" if complete_args.len() == 2 => {
					if let (Some(x), Some(y)) = (complete_args[0].as_number(), complete_args[1].as_number()) {
						CompleteValue::number(x * y).to_partial()
					} else {
						PartialValue::unbound_var("type_error".to_string())
					}
				}
				_ => PartialValue::unbound_var("unknown_builtin".to_string()),
			},
			Err(original_args) => {
				// Arguments aren't all complete - create stuck application with resolved function
				PartialValue::StuckEvaluation(
					StuckEvaluation::Application {
						func: Box::new(builtin_func),
						arg: Box::new(PartialValue::Tuple { elements: original_args }),
					},
					(),
				)
			}
		}
	}

	/// Try to evaluate this value, potentially getting stuck
	pub fn eval(self, ctx: &EvalContext) -> PartialValue {
		match self {
			// Already evaluated values pass through
			PartialValue::Number { .. } | PartialValue::Boolean { .. } | PartialValue::Text { .. } => self,

			// Evaluate tuples recursively
			PartialValue::Tuple { elements } => {
				let eval_elements: Vec<PartialValue> = elements.into_iter().map(|elem| elem.eval(ctx)).collect();
				PartialValue::Tuple { elements: eval_elements }
			}

			// Handle stuck evaluations - some might be resolvable with context
			PartialValue::StuckEvaluation(stuck, _) => {
				match stuck {
					StuckEvaluation::UnboundVariable { name } => {
						// Try to resolve from context
						if let Some(value) = ctx.lookup(&name) {
							value.clone()
						} else {
							// Still unbound
							PartialValue::StuckEvaluation(StuckEvaluation::UnboundVariable { name }, ())
						}
					}
					StuckEvaluation::Application { func, arg } => {
						// Try to evaluate function and argument
						let eval_func = func.eval(ctx);
						let eval_arg = arg.eval(ctx);

						// Try to apply the function if both are resolved
						match (eval_func, eval_arg) {
							// Handle builtin function applications
							(PartialValue::Function { param, body, captured_env }, PartialValue::Tuple { elements })
								if param.starts_with("builtin_") =>
							{
								let builtin_name = param.clone();
								// FIXME: toy language has no user defined fns so only handles builtins
								let builtin_name = builtin_name.strip_prefix("builtin_").unwrap();
								Self::apply_builtin(builtin_name, PartialValue::Function { param, body, captured_env }, elements)
							}
							(eval_func, eval_arg) => PartialValue::StuckEvaluation(
								StuckEvaluation::Application {
									func: Box::new(eval_func),
									arg: Box::new(eval_arg),
								},
								(),
							),
						}
					}
					other => PartialValue::StuckEvaluation(other, ()),
				}
			}

			// Functions are values (don't evaluate body yet)
			PartialValue::Function { .. } => self,
		}
	}

	/// Check if this value is complete (fully evaluated, no stuck states)
	pub fn is_complete(&self) -> bool {
		match self.try_to_complete_ref() {
			Ok(_) => true,
			Err(_) => false,
		}
	}
}

/// Helper function to evaluate an expression and print the result
fn eval_and_print(expr: PartialValue, ctx: &EvalContext) -> PartialValue {
	print!("\n{}", expr);
	let result = expr.eval(ctx);
	println!("  = {}", result);
	println!("  is_complete() = {}", result.is_complete());
	result
}

/// Demo of compile-time subtyping relationships between enums with conditionally uninhabited variants,
/// hopefully probably maybe safe transmute-based conversions. miri seems happy :sweat_smile:,
/// runtime-checked downcasts
fn main() {
	println!(
		"pattern-wishcast toy expression evaluator
---"
	);

	// Create evaluation context with variables
	let mut ctx = EvalContext::new();
	ctx.bind("x".to_string(), CompleteValue::number(100).to_partial());
	ctx.bind("y".to_string(), CompleteValue::number(25).to_partial());

	println!("\nEvaluation Context: {}", ctx);

	// Create a stuck computation: add(x, 5) where x is unbound
	let unbound_x = PartialValue::unbound_var("x".to_string());
	let number_5 = CompleteValue::number(5).to_partial();
	let stuck_expr = PartialValue::StuckEvaluation(
		StuckEvaluation::Application {
			func: Box::new(PartialValue::unbound_var("add".to_string())),
			arg: Box::new(PartialValue::Tuple {
				elements: vec![unbound_x, number_5],
			}),
		},
		(),
	);

	let resolved_expr = eval_and_print(stuck_expr, &ctx);
	match resolved_expr.try_to_complete() {
		Ok(_) => {
			println!("  Downcasts to CompleteValue");
		}
		Err(_) => {
			panic!("add result was not fully evaluated")
		}
	}

	// Show another computation that should resolve completely
	let var_y = PartialValue::unbound_var("y".to_string());
	let mul_computation = PartialValue::StuckEvaluation(
		StuckEvaluation::Application {
			func: Box::new(PartialValue::unbound_var("mul".to_string())),
			arg: Box::new(PartialValue::Tuple {
				elements: vec![var_y, CompleteValue::number(2).to_partial()],
			}),
		},
		(),
	);

	let resolved_mul = eval_and_print(mul_computation, &ctx);
	match resolved_mul.try_to_complete() {
		Ok(_) => {
			println!("  Downcasts to CompleteValue");
		}
		Err(_) => {
			panic!("mul result was not fully evaluated")
		}
	}

	// Now try with an unbound var
	let var_z = PartialValue::unbound_var("z".to_string());
	let mul_unbound = PartialValue::StuckEvaluation(
		StuckEvaluation::Application {
			func: Box::new(PartialValue::unbound_var("mul".to_string())),
			arg: Box::new(PartialValue::Tuple {
				elements: vec![var_z, CompleteValue::number(2).to_partial()],
			}),
		},
		(),
	);

	let _resolved_unbound = eval_and_print(mul_unbound, &ctx);

	// Expected output:
	// Evaluation Context: {
	//   add = ⟨builtin: add⟩
	//   mul = ⟨builtin: mul⟩
	//   sub = ⟨builtin: sub⟩
	//   x = 100
	//   y = 25
	// }

	// (⊥add(⊥x, 5))  = 105
	//   is_complete() = true
	//   ✓ Successfully downcast to CompleteValue

	// (⊥mul(⊥y, 2))  = 50
	//   is_complete() = true
	//   ✓ Successfully downcast to CompleteValue

	// (⊥mul(⊥z, 2))  = (⟨builtin: mul⟩(⊥z, 2))
	//   is_complete() = false
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_complete_arithmetic() {
		let x = CompleteValue::number(10);
		let y = CompleteValue::number(5);
		let sum = x.add(y).unwrap();
		assert_eq!(sum.as_number().unwrap(), 15);
	}

	#[test]
	fn test_complete_upcast() {
		let complete = CompleteValue::number(42);
		let partial = complete.to_partial();
		assert!(partial.is_complete());
		assert_eq!(partial.try_to_complete().unwrap().as_number().unwrap(), 42);
	}

	#[test]
	fn test_stuck_downcast_fails() {
		let stuck = PartialValue::stuck_var(99);
		assert!(!stuck.is_complete());
		assert!(stuck.try_to_complete().is_err());
	}

	#[test]
	fn test_lazy_evaluation() {
		let ctx = EvalContext::new();
		let lazy = PartialValue::LazyComputation {
			thunk: Box::new(CompleteValue::boolean(true).to_partial()),
			_never: (),
		};
		let result = lazy.eval(&ctx);
		assert!(result.is_complete());
		assert_eq!(result.try_to_complete().unwrap().as_boolean().unwrap(), true);
	}
}

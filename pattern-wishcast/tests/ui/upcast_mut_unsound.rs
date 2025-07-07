// SPDX-License-Identifier: MPL-2.0
#![feature(never_type)]

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
	// Define a base value enum that can be None or Some
	enum Value is <P: PatternFields> = {
		None,
		Some(u32),
	};

	// CompleteValue excludes None - it can only be Some
	type CompleteValue = Value is Some { .. };

	// PartialValue allows both None and Some
	type PartialValue = Value is _;

	// This generates upcast/downcast methods including the problematic to_partial_mut
	#[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_complete))]
	impl CompleteValue : PartialValue;
}

// This test verifies that we DON'T generate to_partial_mut method because it would be unsound.
// Previous versions of pattern-wishcast incorrectly generated this method.
//
// If we could upcast &mut CompleteValue to &mut PartialValue, we could:
// 1. Get a &mut CompleteValue (which guarantees None variant is never present)
// 2. Upcast it to &mut PartialValue (which allows None variant)
// 3. Write PartialValue::None through the mutable reference
// 4. Now we have a CompleteValue that contains None, violating its invariant!
fn main() {
	let mut complete: CompleteValue = CompleteValue::Some(42);

	// This should NOT compile! to_partial_mut must not exist
	let partial_mut: &mut PartialValue = complete.to_partial_mut();

	// Now we can assign None, which CompleteValue specifically excludes!
	*partial_mut = PartialValue::None { _never: () };

	// complete now contains None, but its type says that's impossible!
	// we end up with a CompleteValue::None which should be uninhabited
	// due to its _never: ! field

	// If we try to use complete as CompleteValue, we have undefined behavior
	match complete {
		CompleteValue::Some(n) => {
			println!("Value: {}", n);
			return;
		} // This pattern is supposedly unreachable by the type system,
		  // but we just put a None in there!
	}

	unreachable!("Should not be possible to reach this");
}

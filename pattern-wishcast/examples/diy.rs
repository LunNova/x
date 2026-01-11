// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! A DIY implementation of what pattern_wishcast expands to
type AnyResult<O, E> = PatternableResult<AnyResultVariantPresence, O, E>;

type OkResult<O, E> = PatternableResult<OkResultVariantPresence, O, E>;

trait ResultVariantPresence {
	type Ok;
	type Err;
}

struct AnyResultVariantPresence;

impl ResultVariantPresence for AnyResultVariantPresence {
	type Ok = ();
	type Err = ();
}

// AnyResult instances can be either variant, OkResult instances can only be Ok
enum Never {}

struct OkResultVariantPresence;

impl ResultVariantPresence for OkResultVariantPresence {
	type Ok = ();
	type Err = Never;
}

enum PatternableResult<P: ResultVariantPresence, O, E> {
	// 2nd arg is either () or !.
	// If it's ! it's uninhabited so this variant can't be constructed and doesn't need to be matched
	Ok(O, P::Ok),
	_Err(E, P::Err),
}

fn main() {
	upcast();
	unwrap_safely(OkResult::Ok(1, ()));
}

#[test]
fn test_main() {
	main()
}

fn unwrap_safely(ok: OkResult<i64, ()>) -> i64 {
	match ok {
		OkResult::Ok(contains, _) => {
			// Matched on the only possible variant Ok of OkResult
			contains
		} // We don't need another match arm, rustc can tell Err is uninhabited
	}
}

fn upcast() {
	let any_res: AnyResult<i64, i64> = unsafe { std::mem::transmute(OkResult::<i64, i64>::Ok(1, ())) };
	assert!(matches!(&any_res, AnyResult::Ok(_, _)));
	let ok_res: OkResult<i64, i64> = unsafe { std::mem::transmute(any_res) };
	assert!(matches!(&ok_res, OkResult::Ok(_, _)));
}

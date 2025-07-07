<!--
SPDX-FileCopyrightText: 2025 LunNova
SPDX-License-Identifier: MIT
-->

**wishcasting /ˈwɪʃˌkɑːstɪŋ/**  
*n.* — The act of interpreting information or a situation in a way that casts it as favorable or desired, although there is no evidence for such a conclusion; a wishful forecast.

# pattern-wishcast

proc macro implementing some parts of pattern types, a proposed rust language feature which may be added soon™

here's to hoping that demonstrating the usefulness of predicate subtyping with this limited hack speeds the addition of real pattern types

## what

compile-time subtyping relationships between enums with conditionally uninhabited variants. hopefully probably maybe safe transmute-based conversions. miri seems happy.

```rust
#![feature(never_type)]
pattern_wishcast::pattern_wishcast! {
    enum StuckEvaluation = {
      BoundVar(String)
    };
    enum Value is <P: PatternFields> = StuckEvaluation | {
        Number { value: i32 },
        Boolean { value: bool },
        // ...
    };

    // Complete values: no stuck states
    type CompleteValue = Value is Number { .. } | Boolean { .. };

    // with real pattern types we wouldn't need explicit wildcards
    type PartialValue = Value is _;

    #[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_complete))]
    impl CompleteValue : PartialValue;
}
```

generates transmute-based upcasts and runtime-checked downcasts. auto-generated safety tests.

### safety note: no mutable reference upcasting

When you use `#[derive(SubtypingRelation(upcast=foo, downcast=bar))]`, the macro generates:
- `foo(self) -> SuperType` - upcast owned value
- `foo_ref(&self) -> &SuperType` - upcast immutable reference

Why no mutable reference upcasting? Because upcasting `&mut SubType` to `&mut SuperType` would allow:
1. Writing a `SuperType`-only variant through the upcast reference
2. Violating `SubType`'s invariant that certain variants are uninhabited
3. Undefined behavior when the value is used as `SubType` again

## limitations

- only patterns that make entire variants conditional work. can't restrict a field to a range like real rust patterns
- downcast gen has builtin support for only `Vec<T>`, `Box<T>`, `Option<T>` for generic containers containing Value types
  - requires `#[unsafe_transmute_check(iter = ".values()")]` for custom containers, don't mess up or you'll transmute never types into existence

## examples

see `examples/expression_evaluator.rs` for stuck evaluation -> resolved evaluation demo using `CompleteValue` and `PartialValue`

## status

works but hacky. would be much cleaner with native pattern types support in rustc.  
pre-release for now, expect the API to change. might be small changes, might get reworked. have only been iterating on this for a few days.

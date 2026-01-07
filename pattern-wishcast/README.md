<!--
SPDX-FileCopyrightText: 2025 LunNova
SPDX-License-Identifier: MIT
-->
**wishcasting /ˈwɪʃˌkɑːstɪŋ/**  
*n.* — The act of interpreting information or a situation in a way that casts it as favorable or desired, although there is no evidence for such a conclusion; a wishful forecast.

# pattern-wishcast

![Minimum Rust Version](https://img.shields.io/badge/rustc-1.85+-ab6000.svg)

proc macro implementing some parts of pattern types, a proposed rust language feature which may be added soon™

pattern types are a form of predicate subtyping - they create subtypes of existing types based on `match`-like predicates. examples from the [RFC](https://gist.github.com/joboet/0cecbce925ee2ad1ee3e5520cec81e30):

```rust,ignore
// Proposed future syntax
Option<i32> is Some(_)        // only the Some variant
i32 is 1..16                  // integers from 1 to 15
&[Ordering] is [Ordering::Less, ..]  // slices starting with Less
```

pattern types would replace types like `NonZero<u32>` with the more general `u32 is 1..`, enabling niche optimizations where `Option<u32 is 1..10>` is the same size as `u32`.

here's to hoping that demonstrating the usefulness of predicate subtyping for enum variants with this limited hack speeds the addition of real pattern types

rust has an [unstable implementation](https://github.com/rust-lang/rust/issues/123646) that works for numeric ranges but not enum variants or subtyping yet.

## what

compile-time subtyping relationships between enums with conditionally uninhabited variants. hopefully probably maybe safe transmute-based conversions.

```rust
pattern_wishcast::pattern_wishcast! {
    /// Evaluation states that block further progress
    #[derive(Debug, Clone, PartialEq)]
    enum StuckEvaluation = {
      BoundVar(String)
    };

    /// Main value type with pattern-based strictness
    #[derive(Debug, Clone, PartialEq)]
    enum Value is <P: PatternFields> = StuckEvaluation | {
        /// Numeric literal
        Number { value: i32 },
        Boolean { value: bool },
        // Vec<Self> applies the pattern recursively!
        // CompleteValue tuples contain only CompleteValue elements
        Tuple { elements: Vec<Self> },
    };

    // Complete values: no stuck states anywhere in the tree
    type CompleteValue = Value is Number { .. } | Boolean { .. } | Tuple { .. };

    // with real pattern types we wouldn't need explicit wildcards
    type PartialValue = Value is _;

    #[derive(SubtypingRelation(upcast=to_partial, downcast=try_to_complete))]
    impl CompleteValue : PartialValue;
}
```

generates transmute-based upcasts and runtime-checked downcasts. auto-generated safety tests.

## what this achieves

`pattern-wishcast` lets you pretend you have pattern types for enum variants in stable rust by:

- generating traits with associated types that are either `Never` or `()` to make variants conditionally uninhabited
- generating upcast and downcast methods with ref/mut ref variants where safe  
- **recursive patterns**: applying patterns recursively with `Self` - something the current pattern types proposal doesn't support. in the example above, a `CompleteValue::Tuple` guarantees ALL nested elements are also complete, not partially evaluated.

## safety

transmutes between types differing only in unused uninhabited variants seem to work under miri, but i'm not confident about soundness. if you find safety issues please report them.

### safety note: no mutable reference upcasting

when you use `#[derive(SubtypingRelation(upcast=foo, downcast=bar))]`, the macro generates:
- `foo(self) -> SuperType` - upcast owned value
- `foo_ref(&self) -> &SuperType` - upcast immutable reference
- `bar(self) -> Result<SubType, Self>` - checked downcast owned value
- `bar_ref(&self) -> Result<&SubType, ()>` - checked downcast immutable reference
- `bar_mut(&mut self) -> Result<&mut SubType, ()>` - checked downcast mutable reference

why no mutable reference upcasting? upcasting `&mut SubType` to `&mut SuperType` would allow:
1. writing a `SuperType`-only variant through the upcast reference
2. violating `SubType`'s invariant that certain variants are uninhabited
3. undefined behavior when the value is used as `SubType` again

## limitations

- only patterns that make entire variants conditional work. can't restrict a field to a range like real rust patterns
- downcast gen has builtin support for only `Vec<T>`, `Box<T>`, `Option<T>` for generic containers containing Value types
  - requires `#[unsafe_transmute_check(iter = ".values()")]` for custom containers, don't mess up or you'll transmute never types into existence

## examples

see `examples/expression_evaluator.rs` for stuck evaluation -> resolved evaluation demo using `CompleteValue` and `PartialValue`

## status

works but hacky. would be much cleaner with native pattern types support in rustc.  
pre-release for now, expect the API to change. might be small changes, might get reworked. have only been iterating on this for a few days.

**rust version compatibility**: works on stable rust! uses an internal `Never` type instead of requiring nightly. for nightly users, the `never_type` feature enables the real `!` type.

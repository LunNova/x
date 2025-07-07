// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT
#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "never_type", feature(never_type))]

pub use pattern_wishcast_macros::pattern_wishcast;

/// An uninhabited type for use in pattern-wishcast generated code.
///
/// When the `never_type` feature is enabled, this is an alias to the unstable never type (`!`).
/// Otherwise, this is a stable equivalent enum that works on stable Rust.
#[cfg(feature = "never_type")]
pub type Never = !;

/// An uninhabited type for use in pattern-wishcast generated code.
///
/// When the `never_type` feature is enabled, this is an alias to the unstable never type (`!`).
/// Otherwise, this is a stable equivalent enum that works on stable Rust.
#[cfg(not(feature = "never_type"))]
#[derive(Debug, Clone, Copy)]
pub enum Never {}

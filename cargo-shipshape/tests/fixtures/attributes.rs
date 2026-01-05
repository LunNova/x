// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

#[derive(Debug)]
struct Zebra;

#[derive(Clone, Copy)]
#[repr(C)]
struct Apple;

#[inline]
fn zebra_fn() {}

#[cfg(test)]
fn apple_fn() {}

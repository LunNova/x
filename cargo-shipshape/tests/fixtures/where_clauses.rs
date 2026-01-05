// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

impl<T> MyTrait for Container<T>
where
    T: Clone + Send,
{
    fn method(&self) {}
}

impl<T> Container<T>
where
    T: Default,
{
    fn new() -> Self { todo!() }
}

trait MyTrait {}

struct Container<T>(T);

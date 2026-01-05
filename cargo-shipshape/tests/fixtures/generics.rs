// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

impl<T> Clone for Vec<T> {}

impl Vec<i32> {}

impl<T: Debug> Debug for Vec<T> {}

struct Vec<T>(T);

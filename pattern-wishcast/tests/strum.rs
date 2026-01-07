// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use pattern_wishcast::pattern_wishcast;

pattern_wishcast! {
    /// Stuck evaluation states
    #[derive(Debug, Clone, PartialEq)]
    enum StuckValue = {
        /// Unresolved variable
        Free { index: usize },
    };

    /// Main value type with strum discriminants
    #[derive(Debug, Clone, PartialEq, strum::EnumDiscriminants)]
    #[strum_discriminants(derive(strum::IntoStaticStr))]
    enum Value is <P: PatternFields> = StuckValue | {
        /// Fully evaluated number
        Number { value: i32 },
    };

    type StrictValue = Value is Number { .. };
    type FlexValue = Value is _;

    #[derive(SubtypingRelation(upcast=to_flex, downcast=try_to_strict))]
    impl StrictValue : FlexValue;
}

impl FlexValue {
    pub fn kind_name(&self) -> &'static str {
        ValueDiscriminants::from(self).into()
    }
}

#[test]
fn test_strum_discriminants() {
    let v = FlexValue::Number { value: 42 };
    assert_eq!(v.kind_name(), "Number");

    let stuck = FlexValue::StuckValue(StuckValue::Free { index: 0 }, ());
    assert_eq!(stuck.kind_name(), "StuckValue");
}

// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A macro to define bitflags in a structured way.
#[macro_export]
#[doc(hidden)]
macro_rules! khora_bitflags {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident: $ty:ty {
            $(
                $(#[$flag_attr:meta])*
                const $flag_name:ident = $flag_value:expr;
            )*
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        $vis struct $name {
            pub(crate) bits: $ty,
        }

        impl $name {
            /// An empty set of flags.
            pub const EMPTY: Self = Self { bits: 0 };

            /// Creates a new bitflag set from the given raw bits.
            /// Bits not corresponding to any defined flag are kept.
            pub const fn from_bits_truncate(bits: $ty) -> Self {
                Self { bits }
            }

            /// Returns the raw value of the bitflag set.
            pub const fn bits(&self) -> $ty {
                self.bits
            }

            /// Returns `true` if all flags in `other` are contained within `self`.
            pub const fn contains(&self, other: Self) -> bool {
                (self.bits & other.bits) == other.bits
            }

            /// Returns `true` if any flag in `other` is contained within `self`.
            pub const fn intersects(&self, other: Self) -> bool {
                (self.bits & other.bits) != 0
            }

            /// Inserts the flags in `other` into `self`.
            pub fn insert(&mut self, other: Self) {
                self.bits |= other.bits;
            }

            /// Removes the flags in `other` from `self`.
            pub fn remove(&mut self, other: Self) {
                self.bits &= !other.bits;
            }

            /// Toggles the flags in `other` in `self`.
            pub fn toggle(&mut self, other: Self) {
                self.bits ^= other.bits;
            }

            /// Returns a new `Self` with `other` flags inserted.
            #[must_use]
            pub const fn with(mut self, other: Self) -> Self {
                self.bits |= other.bits;
                self
            }

            /// Returns a new `Self` with `other` flags removed.
            #[must_use]
            pub const fn without(mut self, other: Self) -> Self {
                self.bits &= !other.bits;
                self
            }

            // Define the individual flag constants
            $(
                $(#[$flag_attr])*
                pub const $flag_name: Self = Self { bits: $flag_value };
            )*
        }

        // Implement bitwise operators
        impl core::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, other: Self) -> Self {
                Self { bits: self.bits | other.bits }
            }
        }

        impl core::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, other: Self) -> Self {
                Self { bits: self.bits & other.bits }
            }
        }

        impl core::ops::BitXor for $name {
            type Output = Self;
            fn bitxor(self, other: Self) -> Self {
                Self { bits: self.bits ^ other.bits }
            }
        }

        impl core::ops::Not for $name {
            type Output = Self;
            fn not(self) -> Self {
                Self { bits: !self.bits }
            }
        }

        impl core::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, other: Self) {
                self.bits |= other.bits;
            }
        }

        impl core::ops::BitAndAssign for $name {
            fn bitand_assign(&mut self, other: Self) {
                self.bits &= other.bits;
            }
        }

        impl core::ops::BitXorAssign for $name {
            fn bitxor_assign(&mut self, other: Self) {
                self.bits ^= other.bits;
            }
        }

        // Optimized Debug implementation (no runtime allocations)
        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let mut bits = self.bits;
                let mut first_flag = true;

                write!(f, "{} {{ ", stringify!($name))?;

                $(
                    // Only process flags that are non-zero.
                    // Check if the flag's bits are actually present in the current_bits.
                    if ($flag_value != 0) && (bits & $flag_value) == $flag_value {
                        if !first_flag {
                            write!(f, " | ")?;
                        }
                        write!(f, "{}", stringify!($flag_name))?;
                        bits &= !$flag_value; // Clear these bits from the remaining set
                        first_flag = false;
                    }
                )*

                // Handle any remaining unknown bits
                if bits != 0 {
                    if !first_flag {
                        write!(f, " | ")?;
                    }
                    write!(f, "UNKNOWN({:#x})", bits)?;
                    first_flag = false;
                }

                // If after checking all flags (and unknown bits), the original value was 0,
                // and no named flags were printed (meaning `first_flag` is still true),
                // then explicitly print "EMPTY".
                if self.bits == 0 && first_flag {
                    write!(f, "EMPTY")?;
                }

                write!(f, " }}")
            }
        }
    };
}

#[cfg(test)]
mod tests {
    // Import the macro for testing within this module
    use crate::khora_bitflags;

    // Define a test bitflag type using the macro
    khora_bitflags! {
        /// TestFlags for macro verification
        pub struct TestFlags: u32 {
            const FLAG_A = 1 << 0;
            const FLAG_B = 1 << 1;
            const FLAG_C = 1 << 2;
            const FLAG_D = 1 << 3;
            const COMBINED_AC = Self::FLAG_A.bits() | Self::FLAG_C.bits();
            const CUSTOM_HIGH_BIT = 1 << 20;
            const NONE_FLAG = 0; // A flag with value 0, should behave like EMPTY
        }
    }

    #[test]
    fn test_empty_flags() {
        let flags = TestFlags::EMPTY;
        assert_eq!(flags.bits(), 0);
        assert!(flags.contains(TestFlags::EMPTY));
        assert!(!flags.contains(TestFlags::FLAG_A));
        assert_eq!(TestFlags::default().bits(), 0, "Default should be empty");
        assert_eq!(format!("{:?}", flags), "TestFlags { EMPTY }");
    }

    #[test]
    fn test_single_flag() {
        let flags = TestFlags::FLAG_A;
        assert_eq!(flags.bits(), 1);
        assert!(flags.contains(TestFlags::FLAG_A));
        assert!(!flags.contains(TestFlags::FLAG_B));
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A }");
    }

    #[test]
    fn test_multiple_flags() {
        let flags = TestFlags::FLAG_A | TestFlags::FLAG_C;
        assert_eq!(flags.bits(), 0b101); // 1 | 4 = 5
        assert!(flags.contains(TestFlags::FLAG_A));
        assert!(!flags.contains(TestFlags::FLAG_B));
        assert!(flags.contains(TestFlags::FLAG_C));
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_C }");
    }

    #[test]
    fn test_combined_constant() {
        let flags = TestFlags::COMBINED_AC;
        assert_eq!(
            flags.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_C.bits()
        );
        assert!(flags.contains(TestFlags::FLAG_A));
        assert!(flags.contains(TestFlags::FLAG_C));
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_C }");
    }

    #[test]
    fn test_from_bits_truncate_and_bits() {
        let flags = TestFlags::from_bits_truncate(5);
        assert_eq!(flags.bits(), 5);
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_C }");

        // Test with unknown bits
        let unknown_bits = TestFlags::from_bits_truncate(0b10000); // 1 << 4 = 16, not a defined flag
        assert_eq!(unknown_bits.bits(), 16);
        assert_eq!(format!("{:?}", unknown_bits), "TestFlags { UNKNOWN(0x10) }");
    }

    #[test]
    fn test_contains() {
        let all_defined =
            TestFlags::FLAG_A | TestFlags::FLAG_B | TestFlags::FLAG_C | TestFlags::FLAG_D;
        assert!(all_defined.contains(TestFlags::FLAG_A));
        assert!(all_defined.contains(TestFlags::FLAG_A | TestFlags::FLAG_C));
        assert!(!all_defined.contains(TestFlags::CUSTOM_HIGH_BIT));
        assert!(!all_defined.contains(TestFlags::FLAG_A | TestFlags::CUSTOM_HIGH_BIT));
        assert!(all_defined.contains(TestFlags::EMPTY));
    }

    #[test]
    fn test_intersects() {
        let flags1 = TestFlags::FLAG_A | TestFlags::FLAG_B; // 0b0011
        let flags2 = TestFlags::FLAG_B | TestFlags::FLAG_C; // 0b0110
        let flags3 = TestFlags::FLAG_C | TestFlags::FLAG_D; // 0b1100

        assert!(flags1.intersects(flags2)); // Common FLAG_B
        assert!(!flags1.intersects(flags3)); // No common flags
        assert!(flags1.intersects(TestFlags::FLAG_A));
        assert!(!flags1.intersects(TestFlags::EMPTY)); // Empty does not intersect anything (by definition)
    }

    #[test]
    fn test_mutable_operations_insert() {
        let mut flags = TestFlags::FLAG_A;
        flags.insert(TestFlags::FLAG_B);
        assert_eq!(
            flags.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_B.bits()
        );
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_B }");
    }

    #[test]
    fn test_mutable_operations_remove() {
        let mut flags = TestFlags::FLAG_A | TestFlags::FLAG_B;
        flags.remove(TestFlags::FLAG_A);
        assert_eq!(flags.bits(), TestFlags::FLAG_B.bits());
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_B }");

        flags.remove(TestFlags::FLAG_B | TestFlags::FLAG_D); // Remove B, D not present
        assert_eq!(flags.bits(), TestFlags::EMPTY.bits());
        assert_eq!(format!("{:?}", flags), "TestFlags { EMPTY }");
    }

    #[test]
    fn test_mutable_operations_toggle() {
        let mut flags = TestFlags::FLAG_A;
        flags.toggle(TestFlags::FLAG_C); // Add C
        assert_eq!(
            flags.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_C.bits()
        );
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_C }");

        flags.toggle(TestFlags::FLAG_A); // Remove A
        assert_eq!(flags.bits(), TestFlags::FLAG_C.bits());
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_C }");
    }

    #[test]
    fn test_immutable_operations_with() {
        let initial = TestFlags::FLAG_A;
        let with_b = initial.with(TestFlags::FLAG_B);
        assert_eq!(
            with_b.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_B.bits()
        );
        assert_eq!(format!("{:?}", with_b), "TestFlags { FLAG_A | FLAG_B }");
        assert_eq!(
            initial.bits(),
            TestFlags::FLAG_A.bits(),
            "Original should be unchanged"
        );
    }

    #[test]
    fn test_immutable_operations_without() {
        let initial = TestFlags::FLAG_A | TestFlags::FLAG_B;
        let without_a = initial.without(TestFlags::FLAG_A);
        assert_eq!(without_a.bits(), TestFlags::FLAG_B.bits());
        assert_eq!(format!("{:?}", without_a), "TestFlags { FLAG_B }");
        assert_eq!(
            initial.bits(),
            (TestFlags::FLAG_A | TestFlags::FLAG_B).bits(),
            "Original should be unchanged"
        );
    }

    #[test]
    fn test_bitwise_or_operator() {
        let f1 = TestFlags::FLAG_A | TestFlags::FLAG_B;
        let f2 = TestFlags::FLAG_B | TestFlags::FLAG_C;
        let result = f1 | f2;
        assert_eq!(
            result.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_B.bits() | TestFlags::FLAG_C.bits()
        );
        assert_eq!(
            format!("{:?}", result),
            "TestFlags { FLAG_A | FLAG_B | FLAG_C }"
        );
    }

    #[test]
    fn test_bitwise_and_operator() {
        let f1 = TestFlags::FLAG_A | TestFlags::FLAG_B;
        let f2 = TestFlags::FLAG_B | TestFlags::FLAG_C;
        let result = f1 & f2;
        assert_eq!(result.bits(), TestFlags::FLAG_B.bits());
        assert_eq!(format!("{:?}", result), "TestFlags { FLAG_B }");
    }

    #[test]
    fn test_bitwise_xor_operator() {
        let f1 = TestFlags::FLAG_A | TestFlags::FLAG_B;
        let f2 = TestFlags::FLAG_B | TestFlags::FLAG_C;
        let result = f1 ^ f2;
        assert_eq!(
            result.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_C.bits()
        );
        assert_eq!(format!("{:?}", result), "TestFlags { FLAG_A | FLAG_C }");
    }

    #[test]
    fn test_bitwise_not_operator() {
        let flags = TestFlags::FLAG_A;
        let result = !flags;
        assert_eq!(result.bits(), !TestFlags::FLAG_A.bits()); // Value depends on the underlying integer type
        // Debug output for NOT might be complex due to UNKNOWN bits
        // For u32, !1 is 0xFFFFFFFE, which is a lot of unknown bits.
        // We'll just assert the raw bits for now.
    }

    #[test]
    fn test_assign_or_operator() {
        let mut flags = TestFlags::FLAG_A;
        flags |= TestFlags::FLAG_B;
        assert_eq!(
            flags.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_B.bits()
        );
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_B }");
    }

    #[test]
    fn test_assign_and_operator() {
        let mut flags = TestFlags::FLAG_A | TestFlags::FLAG_B | TestFlags::FLAG_C; // 0b0111 = 7
        // AND with (FLAG_B | FLAG_D) = 0b0010 | 0b1000 = 0b1010 = 10
        // Expected result: 0b0111 & 0b1010 = 0b0010 (FLAG_B) = 2
        flags &= TestFlags::FLAG_B | TestFlags::FLAG_D;
        assert_eq!(flags.bits(), TestFlags::FLAG_B.bits());
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_B }");
    }

    #[test]
    fn test_assign_xor_operator() {
        let mut flags = TestFlags::FLAG_A | TestFlags::FLAG_B | TestFlags::FLAG_C;
        flags ^= TestFlags::FLAG_B; // Toggle B (remove it)
        assert_eq!(
            flags.bits(),
            TestFlags::FLAG_A.bits() | TestFlags::FLAG_C.bits()
        );
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A | FLAG_C }");
    }

    #[test]
    fn test_debug_formatting_high_bit() {
        let flags = TestFlags::CUSTOM_HIGH_BIT;
        assert_eq!(format!("{:?}", flags), "TestFlags { CUSTOM_HIGH_BIT }");
    }

    #[test]
    fn test_debug_formatting_mixed_known_and_unknown() {
        // Test a combination of known and unknown bits
        let flags = TestFlags::FLAG_A | TestFlags::from_bits_truncate(1 << 8); // FLAG_A (0x1) | 0x100
        assert_eq!(
            format!("{:?}", flags),
            "TestFlags { FLAG_A | UNKNOWN(0x100) }"
        );

        let flags_more_unknown =
            TestFlags::FLAG_B | TestFlags::from_bits_truncate((1 << 8) | (1 << 9)); // FLAG_B (0x2) | 0x100 | 0x200
        assert_eq!(
            format!("{:?}", flags_more_unknown),
            "TestFlags { FLAG_B | UNKNOWN(0x300) }"
        );
    }

    #[test]
    fn test_debug_formatting_only_unknown() {
        // This test case needs to ensure NO known flags are set.
        // FLAG_A = 1<<0, FLAG_B = 1<<1, FLAG_C = 1<<2, FLAG_D = 1<<3
        // So, 0xFF (0b11111111) is actually *not* only unknown; it contains A, B, C, D.
        // A truly "only unknown" value would be one where bits 0-3 are zero,
        // and 1<<20 is zero, but other bits are set.
        // Example: (1 << 4) | (1 << 5) = 0b110000 = 48 (0x30)
        let flags = TestFlags::from_bits_truncate((1 << 4) | (1 << 5));
        assert_eq!(format!("{:?}", flags), "TestFlags { UNKNOWN(0x30) }");

        // Another example: a single unknown bit that isn't one of the defined ones
        let flags_single_unknown = TestFlags::from_bits_truncate(1 << 4); // 0b10000 = 16 (0x10)
        assert_eq!(
            format!("{:?}", flags_single_unknown),
            "TestFlags { UNKNOWN(0x10) }"
        );
    }

    #[test]
    fn test_none_flag_value() {
        // A flag with value 0 (NONE_FLAG) should not affect operations or appear in debug output
        let flags = TestFlags::FLAG_A | TestFlags::NONE_FLAG;
        assert_eq!(flags.bits(), TestFlags::FLAG_A.bits());
        assert_eq!(format!("{:?}", flags), "TestFlags { FLAG_A }");

        let flags_empty = TestFlags::NONE_FLAG;
        assert_eq!(flags_empty.bits(), 0);
        assert_eq!(format!("{:?}", flags_empty), "TestFlags { EMPTY }");
    }
}

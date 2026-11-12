//! User-configurable rounding of computed amounts (percentages, interest).
//!
//! Banks differ: some round to the satang, some up to the whole baht. This is a setting.

use std::fmt;

/// Which way to round a value that falls between two steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoundingMode {
    /// Toward positive infinity (customer pays the extra).
    Up,
    /// Toward negative infinity.
    Down,
    /// To the nearest step, halves away from zero.
    #[default]
    HalfUp,
}

/// Round to a multiple of `step` minor units (1 = satang, 100 = whole baht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rounding {
    step: i64,
    mode: RoundingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStep(pub i64);

impl fmt::Display for InvalidStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rounding step must be at least 1 minor unit, got {}",
            self.0
        )
    }
}

impl std::error::Error for InvalidStep {}

impl Default for Rounding {
    /// Nearest minor unit (satang), halves up.
    fn default() -> Self {
        Self {
            step: 1,
            mode: RoundingMode::HalfUp,
        }
    }
}

impl Rounding {
    pub fn new(step: i64, mode: RoundingMode) -> Result<Self, InvalidStep> {
        if step < 1 {
            Err(InvalidStep(step))
        } else {
            Ok(Self { step, mode })
        }
    }

    pub const fn step(&self) -> i64 {
        self.step
    }

    pub const fn mode(&self) -> RoundingMode {
        self.mode
    }

    /// Round `numerator / denominator` (in minor units) to this rule.
    /// `denominator` must be positive. Returns `None` on overflow.
    pub fn apply(&self, numerator: i128, denominator: i128) -> Option<i64> {
        debug_assert!(denominator > 0);
        let unit = denominator.checked_mul(i128::from(self.step))?;
        let q = numerator.div_euclid(unit);
        let r = numerator.rem_euclid(unit);
        let steps = match self.mode {
            RoundingMode::Down => q,
            RoundingMode::Up => q + i128::from(r != 0),
            RoundingMode::HalfUp => {
                // Halves away from zero: compare against the half, flipped for negatives.
                let twice = r * 2;
                if twice > unit || (twice == unit && numerator >= 0) {
                    q + 1
                } else {
                    q
                }
            }
        };
        i64::try_from(steps.checked_mul(i128::from(self.step))?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(step: i64, mode: RoundingMode) -> Rounding {
        Rounding::new(step, mode).unwrap()
    }

    #[test]
    fn satang_half_up() {
        let x = Rounding::default();
        assert_eq!(x.apply(1005, 10), Some(101)); // 100.5 → 101
        assert_eq!(x.apply(1004, 10), Some(100));
        assert_eq!(x.apply(-1005, 10), Some(-101)); // halves away from zero
        assert_eq!(x.apply(-1004, 10), Some(-100));
    }

    #[test]
    fn whole_baht_up_and_down() {
        // ฿12.01 → up ฿13, down ฿12
        assert_eq!(r(100, RoundingMode::Up).apply(1201, 1), Some(1300));
        assert_eq!(r(100, RoundingMode::Down).apply(1201, 1), Some(1200));
        assert_eq!(r(100, RoundingMode::Up).apply(1200, 1), Some(1200));
        assert_eq!(r(100, RoundingMode::HalfUp).apply(1250, 1), Some(1300));
        assert_eq!(r(100, RoundingMode::HalfUp).apply(1249, 1), Some(1200));
    }

    #[test]
    fn rejects_zero_step() {
        assert_eq!(Rounding::new(0, RoundingMode::Up), Err(InvalidStep(0)));
    }

    #[test]
    fn overflow_is_none() {
        assert_eq!(Rounding::default().apply(i128::from(i64::MAX) * 4, 1), None);
    }
}

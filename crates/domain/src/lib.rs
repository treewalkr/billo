//! Billo domain: pure, deterministic financial calculations.
//!
//! No I/O, no clock, no randomness. Every input (including "today") is passed in.

pub mod money;

pub use money::{Currency, Money, MoneyError};

pub mod calendar;

pub use calendar::{BusinessCalendar, EveryDay, WeekendCalendar};

pub mod card;
pub mod daycount;

pub mod rounding;

pub use daycount::DayCount;
pub use rounding::{Rounding, RoundingMode};

//! Thai financial institution holidays (วันหยุดของสถาบันการเงิน), nationwide.
//!
//! Copied from the Bank of Thailand's notifications; every date below was checked
//! against the notification it cites. Do not add a year without an official source.
//!
//! - 2026 (B.E. 2569): Notification of the Bank of Thailand No. 31/2568, announced
//!   5 August 2025, 19 days.
//!   <https://www.bot.or.th/content/dam/bot/fipcs/documents/FPG/2568/EngPDF/25680162.pdf>
//! - 2027 (B.E. 2570): Notification of the Bank of Thailand No. 37/2569, announced
//!   17 August 2026, 18 days.
//!   <https://www.bot.or.th/content/dam/bot/fipcs/documents/FPG/2569/EngPDF/25690175.pdf>
//!
//! Not included, because they don't apply to every branch:
//! - Friday 16 October 2026, a special holiday in Bangkok only (No. 26/2569,
//!   <https://www.bot.or.th/content/dam/bot/fipcs/documents/FPG/2569/EngPDF/25690116.pdf>).
//! - Eid al-Fitr and Eid al-Adha (Islamic Bank of Thailand, and branches in Narathiwat,
//!   Pattani, Yala, Satun and Songkhla) and Chinese New Year (those five provinces):
//!   their dates are announced separately.
//!
//! Users can add such days themselves as extra holidays.
//!
//! Years after 2027 are not covered yet: add them when the Bank of Thailand publishes
//! the notification (usually in August of the year before).

use time::Date;
use time::macros::date;

/// The years this table covers; outside them only weekends (and user holidays) apply.
pub const YEARS: std::ops::RangeInclusive<i32> = 2026..=2027;

/// `(date, English name, Thai name)`, in date order.
pub const HOLIDAYS: &[(Date, &str, &str)] = &[
    // 2026: BoT No. 31/2568.
    (date!(2026 - 01 - 01), "New Year's Day", "วันขึ้นปีใหม่"),
    (
        date!(2026 - 01 - 02),
        "Additional special holiday",
        "วันหยุดพิเศษเพิ่มเติม",
    ),
    (date!(2026 - 03 - 03), "Makha Bucha Day", "วันมาฆบูชา"),
    (date!(2026 - 04 - 06), "Chakri Memorial Day", "วันจักรี"),
    (date!(2026 - 04 - 13), "Songkran Festival", "วันสงกรานต์"),
    (date!(2026 - 04 - 14), "Songkran Festival", "วันสงกรานต์"),
    (date!(2026 - 04 - 15), "Songkran Festival", "วันสงกรานต์"),
    (
        date!(2026 - 05 - 01),
        "National Labour Day",
        "วันแรงงานแห่งชาติ",
    ),
    (date!(2026 - 05 - 04), "Coronation Day", "วันฉัตรมงคล"),
    (
        date!(2026 - 06 - 01),
        "Substitution for Visakha Bucha Day",
        "ชดเชยวันวิสาขบูชา",
    ),
    (
        date!(2026 - 06 - 03),
        "H.M. Queen Suthida's Birthday",
        "วันเฉลิมพระชนมพรรษาสมเด็จพระราชินี",
    ),
    (
        date!(2026 - 07 - 28),
        "H.M. the King's Birthday",
        "วันเฉลิมพระชนมพรรษาพระบาทสมเด็จพระเจ้าอยู่หัว",
    ),
    (date!(2026 - 07 - 29), "Asarnha Bucha Day", "วันอาสาฬหบูชา"),
    (
        date!(2026 - 08 - 12),
        "The Queen Mother's Birthday and Mother's Day",
        "วันเฉลิมพระชนมพรรษาพระพันปีหลวง และวันแม่แห่งชาติ",
    ),
    (
        date!(2026 - 10 - 13),
        "King Bhumibol the Great Memorial Day",
        "วันนวมินทรมหาราช",
    ),
    (
        date!(2026 - 10 - 23),
        "King Chulalongkorn the Great Memorial Day",
        "วันปิยมหาราช",
    ),
    (
        date!(2026 - 12 - 07),
        "Substitution for Father's Day",
        "ชดเชยวันพ่อแห่งชาติ",
    ),
    (date!(2026 - 12 - 10), "Constitution Day", "วันรัฐธรรมนูญ"),
    (date!(2026 - 12 - 31), "New Year's Eve", "วันสิ้นปี"),
    // 2027: BoT No. 37/2569.
    (date!(2027 - 01 - 01), "New Year's Day", "วันขึ้นปีใหม่"),
    (
        date!(2027 - 02 - 22),
        "Substitution for Makha Bucha Day",
        "ชดเชยวันมาฆบูชา",
    ),
    (date!(2027 - 04 - 06), "Chakri Memorial Day", "วันจักรี"),
    (date!(2027 - 04 - 13), "Songkran Festival", "วันสงกรานต์"),
    (date!(2027 - 04 - 14), "Songkran Festival", "วันสงกรานต์"),
    (date!(2027 - 04 - 15), "Songkran Festival", "วันสงกรานต์"),
    (
        date!(2027 - 05 - 03),
        "Substitution for National Labour Day",
        "ชดเชยวันแรงงานแห่งชาติ",
    ),
    (date!(2027 - 05 - 04), "Coronation Day", "วันฉัตรมงคล"),
    (date!(2027 - 05 - 20), "Visakha Bucha Day", "วันวิสาขบูชา"),
    (
        date!(2027 - 06 - 03),
        "H.M. Queen Suthida's Birthday",
        "วันเฉลิมพระชนมพรรษาสมเด็จพระราชินี",
    ),
    (
        date!(2027 - 07 - 19),
        "Substitution for Asarnha Bucha Day",
        "ชดเชยวันอาสาฬหบูชา",
    ),
    (
        date!(2027 - 07 - 28),
        "H.M. the King's Birthday",
        "วันเฉลิมพระชนมพรรษาพระบาทสมเด็จพระเจ้าอยู่หัว",
    ),
    (
        date!(2027 - 08 - 12),
        "The Queen Mother's Birthday and Mother's Day",
        "วันเฉลิมพระชนมพรรษาพระพันปีหลวง และวันแม่แห่งชาติ",
    ),
    (
        date!(2027 - 10 - 13),
        "King Bhumibol the Great Memorial Day",
        "วันนวมินทรมหาราช",
    ),
    (
        date!(2027 - 10 - 25),
        "Substitution for King Chulalongkorn the Great Memorial Day",
        "ชดเชยวันปิยมหาราช",
    ),
    (
        date!(2027 - 12 - 06),
        "Substitution for Father's Day",
        "ชดเชยวันพ่อแห่งชาติ",
    ),
    (date!(2027 - 12 - 10), "Constitution Day", "วันรัฐธรรมนูญ"),
    (date!(2027 - 12 - 31), "New Year's Eve", "วันสิ้นปี"),
];

/// The holiday dates only.
pub fn dates() -> impl Iterator<Item = Date> {
    HOLIDAYS.iter().map(|(d, _, _)| *d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Weekday;

    #[test]
    fn counts_match_the_notifications() {
        let count = |y| dates().filter(|d| d.year() == y).count();
        assert_eq!(count(2026), 19);
        assert_eq!(count(2027), 18);
        assert!(dates().all(|d| YEARS.contains(&d.year())));
    }

    #[test]
    fn sorted_unique_and_on_weekdays() {
        let all: Vec<Date> = dates().collect();
        assert!(all.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
        // The notifications move weekend holidays to a substitute weekday.
        assert!(
            all.iter()
                .all(|d| !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday))
        );
    }
}

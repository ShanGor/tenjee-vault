//! 传统节日与二十四节气（design D4）。
//!
//! 传统节日由农历日期推导；二十四节气由内置查表（lunar_data.rs）按公历日匹配；
//! 清明既是节气也是节气类传统节日，展示层由两者合成。

use chrono::{Datelike, NaiveDate};

use super::lunar::MAX_YEAR;
use super::lunar::{month_days, LunarDate};
use super::lunar_data::SOLAR_TERM_DAYS;

/// 二十四节气名称（索引 0..23，与 SOLAR_TERM_DAYS 对齐）。
pub const TERM_NAMES: [&str; 24] = [
    "小寒", "大寒", "立春", "雨水", "惊蛰", "春分", "清明", "谷雨", "立夏", "小满", "芒种", "夏至",
    "小暑", "大暑", "立秋", "处暑", "白露", "秋分", "寒露", "霜降", "立冬", "小雪", "大雪", "冬至",
];

/// 节气序号对应的公历月（0→1 月小寒…22/23→12 月大雪/冬至）。
pub const fn term_month(idx: u8) -> u8 {
    idx / 2 + 1
}

/// 公历日命中某节气则返回其序号（0..23），否则 None。超出查表年份返回 None。
pub fn solar_term(date: NaiveDate) -> Option<u8> {
    let year = date.year();
    if !(super::lunar::MIN_YEAR..=MAX_YEAR).contains(&year) {
        return None;
    }
    let row = &SOLAR_TERM_DAYS[(year - super::lunar::MIN_YEAR) as usize];
    let idx0 = ((date.month() - 1) * 2) as usize;
    let day = date.day() as u8;
    if row[idx0] == day {
        return Some(idx0 as u8);
    }
    if row[idx0 + 1] == day {
        return Some(idx0 as u8 + 1);
    }
    None
}

/// 农历日对应的内建传统节日（仅平月有节；闰月不重复过节）。除夕 = 腊月最后一日。
pub fn traditional_festival(lunar: &LunarDate) -> Option<&'static str> {
    if lunar.is_leap {
        return None;
    }
    match (lunar.month, lunar.day) {
        (1, 1) => Some("春节"),
        (1, 15) => Some("元宵节"),
        (5, 5) => Some("端午节"),
        (7, 7) => Some("七夕节"),
        (7, 15) => Some("中元节"),
        (8, 15) => Some("中秋节"),
        (9, 9) => Some("重阳节"),
        (12, 8) => Some("腊八节"),
        (12, d) if d == month_days(lunar.year, 12) => Some("除夕"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::lunar::solar_to_lunar;
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn qingming_follows_solar_term_across_years() {
        // 清明严格按节气实际日期（4 月 4/5 日浮动）
        let cases: &[(i32, u32)] = &[
            (2020, 4),
            (2021, 4),
            (2022, 5),
            (2023, 5),
            (2024, 4),
            (2025, 4),
            (2026, 5),
        ];
        for &(year, day) in cases {
            let date = d(year, 4, day);
            assert_eq!(solar_term(date), Some(6), "{date} 应为清明（节气序号 6）");
            assert_eq!(TERM_NAMES[6], "清明");
        }
    }

    #[test]
    fn dongzhi_anchors() {
        let cases: &[(i32, u32)] = &[
            (2020, 21),
            (2021, 21),
            (2022, 22),
            (2023, 22),
            (2024, 21),
            (2025, 21),
        ];
        for &(year, day) in cases {
            assert_eq!(
                solar_term(d(year, 12, day)),
                Some(23),
                "{year}-12-{day} 应为冬至"
            );
        }
    }

    #[test]
    fn term_month_mapping() {
        assert_eq!(term_month(0), 1); // 小寒
        assert_eq!(term_month(1), 1); // 大寒
        assert_eq!(term_month(2), 2); // 立春
        assert_eq!(term_month(22), 12); // 大雪
        assert_eq!(term_month(23), 12); // 冬至
        assert_eq!(solar_term(d(2026, 3, 5)), Some(4), "2026-03-05 应为惊蛰");
        assert_eq!(solar_term(d(2026, 8, 7)), Some(14), "2026-08-07 应为立秋");
        assert_eq!(solar_term(d(2026, 6, 21)), Some(11), "2026-06-21 应为夏至");
    }

    #[test]
    fn traditional_festivals_derive_from_lunar() {
        // 2026 年：春节 2-17、元宵 3-03、端午 6-19、七夕 8-19、中秋 9-25、重阳 10-18
        let new_year = traditional_festival(&solar_to_lunar(d(2026, 2, 17)).unwrap()).unwrap();
        assert_eq!(new_year, "春节");
        assert_eq!(
            traditional_festival(&solar_to_lunar(d(2026, 3, 3)).unwrap()).unwrap(),
            "元宵节"
        );
        assert_eq!(
            traditional_festival(&solar_to_lunar(d(2026, 6, 19)).unwrap()).unwrap(),
            "端午节"
        );
        assert_eq!(
            traditional_festival(&solar_to_lunar(d(2026, 8, 19)).unwrap()).unwrap(),
            "七夕节"
        );
        assert_eq!(
            traditional_festival(&solar_to_lunar(d(2026, 9, 25)).unwrap()).unwrap(),
            "中秋节"
        );
        assert_eq!(
            traditional_festival(&solar_to_lunar(d(2026, 10, 18)).unwrap()).unwrap(),
            "重阳节"
        );
        // 腊八：2025-01-07 = 甲辰年腊月初八（跨公历年，lunar.year 为 2024）
        let laba = solar_to_lunar(d(2025, 1, 7)).unwrap();
        assert_eq!((laba.year, laba.month, laba.day), (2024, 12, 8));
        assert_eq!(traditional_festival(&laba).unwrap(), "腊八节");
        // 除夕：2026-02-16 = 乙巳年腊月廿九（2025 年腊月小月 29 天）
        let chuxi = solar_to_lunar(d(2026, 2, 16)).unwrap();
        assert_eq!((chuxi.year, chuxi.month, chuxi.day), (2025, 12, 29));
        assert_eq!(traditional_festival(&chuxi).unwrap(), "除夕");
        // 平月无节日
        let plain = solar_to_lunar(d(2026, 4, 1)).unwrap();
        assert!(traditional_festival(&plain).is_none());
    }

    #[test]
    fn leap_month_has_no_festival() {
        // 2025 闰六月初一不重复过任何节
        let leap = solar_to_lunar(NaiveDate::from_ymd_opt(2025, 7, 25).unwrap()).unwrap();
        assert!(leap.is_leap && leap.month == 6);
        assert!(traditional_festival(&leap).is_none());
    }

    #[test]
    fn out_of_range_years_have_no_terms() {
        assert!(solar_term(d(1899, 1, 5)).is_none());
        assert!(solar_term(d(2101, 1, 5)).is_none());
    }
}

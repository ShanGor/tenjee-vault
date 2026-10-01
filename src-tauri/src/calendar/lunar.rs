//! 农历换算（design D4）：基于内置 1900–2100 位掩码查表的纯本地历法换算。
//!
//! 算法为经典查表法：以 1900-01-31（农历 1900 年正月初一）为基准做日差行走。
//! 范围接口按顺序增量推进，避免逐日重复行走（替代逐日 memoize，见 design D4）。

use chrono::NaiveDate;

use super::lunar_data::LUNAR_INFO;

/// 查表覆盖的农历年份范围。
pub const MIN_YEAR: i32 = 1900;
pub const MAX_YEAR: i32 = 2100;
/// 公历可换算起点：1900-01-31 = 农历 1900-01-01。
pub const MIN_SOLAR: NaiveDate = match NaiveDate::from_ymd_opt(1900, 1, 31) {
    Some(d) => d,
    None => panic!(),
};

/// 农历日期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LunarDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub is_leap: bool,
}

/// 某农历年的闰月月份（0 = 无闰月）。
pub fn leap_month(year: i32) -> u8 {
    debug_assert!((MIN_YEAR..=MAX_YEAR).contains(&year));
    (LUNAR_INFO[(year - MIN_YEAR) as usize] & 0xf) as u8
}

/// 某农历年闰月的天数（无闰月返回 0）。
pub fn leap_days(year: i32) -> u8 {
    if leap_month(year) == 0 {
        0
    } else if LUNAR_INFO[(year - MIN_YEAR) as usize] & 0x10000 != 0 {
        30
    } else {
        29
    }
}

/// 某农历年某平月的天数（29/30）。
pub fn month_days(year: i32, month: u8) -> u8 {
    debug_assert!((MIN_YEAR..=MAX_YEAR).contains(&year));
    debug_assert!((1..=12).contains(&month));
    if LUNAR_INFO[(year - MIN_YEAR) as usize] & (0x10000 >> month) != 0 {
        30
    } else {
        29
    }
}

/// 某农历年全年天数（含闰月）。
pub fn year_days(year: i32) -> u16 {
    let mut sum = leap_days(year) as u16;
    for m in 1..=12 {
        sum += month_days(year, m) as u16;
    }
    sum
}

const MONTH_NAMES: [&str; 12] = [
    "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "腊月",
];

/// 农历月名称（正月/二月/…/冬月/腊月）。
pub fn month_name(month: u8) -> &'static str {
    MONTH_NAMES[(month - 1) as usize]
}

const DAY_NAMES: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一", "十二",
    "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十", "廿一", "廿二", "廿三", "廿四",
    "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];

/// 农历日名称（初一/十五/廿三/三十…）。
pub fn day_name(day: u8) -> &'static str {
    DAY_NAMES[(day - 1) as usize]
}

/// 公历 → 农历。超出查表范围（早于 1900-01-31，或晚于农历 2100 年末）返回 None。
pub fn solar_to_lunar(date: NaiveDate) -> Option<LunarDate> {
    let mut offset = (date - MIN_SOLAR).num_days();
    if offset < 0 {
        return None;
    }
    // 走年
    let mut year = MIN_YEAR;
    while year <= MAX_YEAR {
        let yd = year_days(year) as i64;
        if offset < yd {
            break;
        }
        offset -= yd;
        year += 1;
    }
    if year > MAX_YEAR {
        return None;
    }
    // 走月：平月与（如有）其后的闰月按序交替
    let leap = leap_month(year);
    let mut month = 1u8;
    let mut is_leap = false;
    loop {
        let md = if is_leap {
            leap_days(year)
        } else {
            month_days(year, month)
        } as i64;
        if offset < md {
            break;
        }
        offset -= md;
        if is_leap {
            is_leap = false;
            month += 1;
        } else if month == leap {
            is_leap = true;
        } else {
            month += 1;
        }
    }
    Some(LunarDate {
        year,
        month,
        day: offset as u8 + 1,
        is_leap,
    })
}

/// 农历 → 公历。非法输入（越界年份/月份/日、无此闰月）返回 None。
/// 日超出当月天数时钳制到当月最后一日（如腊月三十遇小月归到廿九，供除夕等场景使用）。
pub fn lunar_to_solar(year: i32, month: u8, day: u8, is_leap: bool) -> Option<NaiveDate> {
    if !(MIN_YEAR..=MAX_YEAR).contains(&year) || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    if is_leap && leap_month(year) != month {
        return None;
    }
    let max_day = if is_leap {
        leap_days(year)
    } else {
        month_days(year, month)
    };
    let day = day.min(max_day);
    let mut offset: i64 = 0;
    for y in MIN_YEAR..year {
        offset += year_days(y) as i64;
    }
    let leap = leap_month(year);
    let mut m = 1u8;
    while m < month {
        offset += month_days(year, m) as i64;
        if m == leap {
            offset += leap_days(year) as i64;
        }
        m += 1;
    }
    if is_leap {
        offset += month_days(year, month) as i64;
    }
    offset += day as i64 - 1;
    Some(MIN_SOLAR + chrono::Duration::days(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn table_starts_with_canonical_1900_value() {
        // 经典公开表 1900 年编码 0x04bd8（闰八月，29 天）
        assert_eq!(LUNAR_INFO[0], 0x04bd8);
        assert_eq!(leap_month(1900), 8);
        assert_eq!(leap_days(1900), 29);
        assert_eq!(leap_days(2025), 29, "2025 闰六月为小月");
    }

    #[test]
    fn spring_festival_anchors() {
        // 权威日期对照（多源公认数据）
        let cases: &[(i32, u32, u32, i32)] = &[
            (1900, 1, 31, 1900),
            (1950, 2, 17, 1950),
            (1978, 2, 7, 1978),
            (2000, 2, 5, 2000),
            (2010, 2, 14, 2010),
            (2020, 1, 25, 2020),
            (2023, 1, 22, 2023),
            (2024, 2, 10, 2024),
            (2025, 1, 29, 2025),
            (2026, 2, 17, 2026),
            (2030, 2, 3, 2030),
            (2050, 1, 23, 2050),
            (2100, 2, 9, 2100),
        ];
        for &(y, m, day, lunar_year) in cases {
            let lunar =
                solar_to_lunar(d(y, m, day)).unwrap_or_else(|| panic!("{y}-{m}-{day} 应可换算"));
            assert_eq!(
                (lunar.year, lunar.month, lunar.day, lunar.is_leap),
                (lunar_year, 1, 1, false),
                "{y}-{m}-{day} 应为农历 {lunar_year} 正月初一"
            );
        }
    }

    #[test]
    fn mid_autumn_anchors() {
        // 农历八月十五
        let cases: &[(i32, u32, u32)] = &[
            (2020, 10, 1),
            (2021, 9, 21),
            (2022, 9, 10),
            (2023, 9, 29),
            (2024, 9, 17),
            (2025, 10, 6),
        ];
        for &(y, m, day) in cases {
            let lunar = solar_to_lunar(d(y, m, day)).unwrap();
            assert_eq!(
                (lunar.month, lunar.day, lunar.is_leap),
                (8, 15, false),
                "{y}-{m}-{day} 应为中秋"
            );
        }
    }

    #[test]
    fn leap_month_years() {
        // 2025 闰六月、2023 闰二月、2020 闰四月
        assert_eq!(leap_month(2025), 6);
        assert_eq!(leap_month(2023), 2);
        assert_eq!(leap_month(2020), 4);
        assert_eq!(leap_month(2026), 0);
        // 2025 闰六月初一 = 2025-07-25
        let solar = lunar_to_solar(2025, 6, 1, true).unwrap();
        assert_eq!(solar, d(2025, 7, 25));
        let back = solar_to_lunar(solar).unwrap();
        assert_eq!(
            (back.year, back.month, back.day, back.is_leap),
            (2025, 6, 1, true)
        );
    }

    #[test]
    fn lunar_to_solar_round_trip_and_clamp() {
        // 2026 正月初一
        assert_eq!(lunar_to_solar(2026, 1, 1, false).unwrap(), d(2026, 2, 17));
        // 腊月三十遇小月钳制到廿九（2025 年腊月为小月 29 天）
        assert_eq!(month_days(2025, 12), 29);
        assert_eq!(
            lunar_to_solar(2025, 12, 30, false).unwrap(),
            lunar_to_solar(2025, 12, 29, false).unwrap()
        );
        // 非法：2026 无闰月却指定闰月
        assert!(lunar_to_solar(2026, 6, 1, true).is_none());
        assert!(lunar_to_solar(1899, 1, 1, false).is_none());
        assert!(lunar_to_solar(2026, 13, 1, false).is_none());
        assert!(lunar_to_solar(2026, 1, 0, false).is_none());
    }

    #[test]
    fn exhaustive_round_trip_all_days() {
        // 全范围逐日往返：solar → lunar → solar 必须恒等（覆盖 1900-01-31 起约 7 万余日）
        let mut date = MIN_SOLAR;
        let end = d(2100, 12, 31);
        while date <= end {
            let lunar = solar_to_lunar(date).unwrap_or_else(|| panic!("{date} 应可换算"));
            let back = lunar_to_solar(lunar.year, lunar.month, lunar.day, lunar.is_leap)
                .unwrap_or_else(|| panic!("{date} 对应农历应可反解"));
            assert_eq!(back, date, "{date} 往返不一致");
            date += chrono::Duration::days(1);
        }
        // 2101 年起超出范围
        assert!(solar_to_lunar(d(2101, 6, 1)).is_none());
        assert!(solar_to_lunar(d(1900, 1, 30)).is_none());
    }

    #[test]
    fn names() {
        assert_eq!(month_name(1), "正月");
        assert_eq!(month_name(11), "冬月");
        assert_eq!(month_name(12), "腊月");
        assert_eq!(day_name(1), "初一");
        assert_eq!(day_name(15), "十五");
        assert_eq!(day_name(23), "廿三");
        assert_eq!(day_name(30), "三十");
    }
}

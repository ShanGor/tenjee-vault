//! 农历重复事件规则（spec §农历事件与重复；design D4）。
//!
//! `events.lunar_recurrence` 列的 JSON 语义：`{month, day, leap_month}`。
//! leap_month 策略：默认忽略闰月（闰月年份按平月庆祝）；`only` 指定仅在闰月庆祝。

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use super::lunar::{leap_month, lunar_to_solar, MAX_YEAR, MIN_YEAR};
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeapMonthPolicy {
    /// 默认：忽略闰月，闰月年份按平月日期庆祝。
    Ignore,
    /// 仅在闰月庆祝（平年不出现）。
    Only,
}

impl Default for LeapMonthPolicy {
    fn default() -> Self {
        Self::Ignore
    }
}

/// 农历年重复规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LunarRecurrence {
    /// 农历月（1..=12）。
    pub month: u8,
    /// 农历日（1..=30）。
    pub day: u8,
    #[serde(default)]
    pub leap_month: LeapMonthPolicy,
}

impl LunarRecurrence {
    pub fn parse(json: &str) -> VaultResult<Self> {
        let rule: LunarRecurrence = serde_json::from_str(json)
            .map_err(|e| VaultError::Validation(format!("农历重复规则解析失败: {e}")))?;
        rule.validate()?;
        Ok(rule)
    }

    pub fn to_json(&self) -> VaultResult<String> {
        self.validate()?;
        serde_json::to_string(self)
            .map_err(|e| VaultError::Validation(format!("农历重复规则序列化失败: {e}")))
    }

    fn validate(&self) -> VaultResult<()> {
        if !(1..=12).contains(&self.month) {
            return Err(VaultError::Validation(format!(
                "农历月 {} 非法（应为 1-12）",
                self.month
            )));
        }
        if !(1..=30).contains(&self.day) {
            return Err(VaultError::Validation(format!(
                "农历日 {} 非法（应为 1-30）",
                self.day
            )));
        }
        Ok(())
    }

    /// 在给定公历范围内展开为公历日期序列（升序）。
    /// 日超出当月天数时钳制到当月最后一日（沿用 lunar_to_solar 的约定）。
    pub fn expand(&self, range_start: NaiveDate, range_end: NaiveDate) -> Vec<NaiveDate> {
        if range_end < range_start {
            return Vec::new();
        }
        let mut out = Vec::new();
        // 范围内公历年映射到农历年（左右各扩一年覆盖跨春节边界）
        let y_start = (range_start.year() - 1).max(MIN_YEAR);
        let y_end = (range_end.year() + 1).min(MAX_YEAR);
        for year in y_start..=y_end {
            let solar = match self.leap_month {
                LeapMonthPolicy::Ignore => lunar_to_solar(year, self.month, self.day, false),
                LeapMonthPolicy::Only => {
                    if leap_month(year) == self.month {
                        lunar_to_solar(year, self.month, self.day, true)
                    } else {
                        None
                    }
                }
            };
            if let Some(date) = solar {
                if date >= range_start && date <= range_end {
                    out.push(date);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn lunar_birthday_expands_across_years() {
        let rule = LunarRecurrence {
            month: 8,
            day: 10,
            leap_month: LeapMonthPolicy::Ignore,
        };
        let dates = rule.expand(d(2024, 1, 1), d(2026, 12, 31));
        assert_eq!(
            dates,
            vec![d(2024, 9, 12), d(2025, 10, 1), d(2026, 9, 20)],
            "农历八月初十跨三年"
        );
    }

    #[test]
    fn leap_month_ignore_uses_regular_month() {
        // 2020 闰四月：ignore 策略走平四月初一（2020-04-23），闰四月初一（05-23）不出现
        let rule = LunarRecurrence {
            month: 4,
            day: 1,
            leap_month: LeapMonthPolicy::Ignore,
        };
        let dates = rule.expand(d(2020, 1, 1), d(2020, 12, 31));
        assert_eq!(dates, vec![d(2020, 4, 23)]);
    }

    #[test]
    fn leap_month_only_appears_in_leap_years() {
        // only 策略：2020 闰四月初一（05-23）出现；2021 无闰四月则不出现
        let rule = LunarRecurrence {
            month: 4,
            day: 1,
            leap_month: LeapMonthPolicy::Only,
        };
        let dates = rule.expand(d(2019, 1, 1), d(2021, 12, 31));
        assert_eq!(dates, vec![d(2020, 5, 23)]);
        // 闰六月（2025）同理
        let rule6 = LunarRecurrence {
            month: 6,
            day: 1,
            leap_month: LeapMonthPolicy::Only,
        };
        assert_eq!(
            rule6.expand(d(2025, 1, 1), d(2025, 12, 31)),
            vec![d(2025, 7, 25)]
        );
    }

    #[test]
    fn json_round_trip_and_validation() {
        let rule = LunarRecurrence::parse(r#"{"month": 1, "day": 1}"#).unwrap();
        assert_eq!(
            rule.leap_month,
            LeapMonthPolicy::Ignore,
            "缺省策略为忽略闰月"
        );
        let json = rule.to_json().unwrap();
        let back = LunarRecurrence::parse(&json).unwrap();
        assert_eq!(back, rule);

        let only =
            LunarRecurrence::parse(r#"{"month": 4, "day": 1, "leap_month": "only"}"#).unwrap();
        assert_eq!(only.leap_month, LeapMonthPolicy::Only);

        assert!(LunarRecurrence::parse(r#"{"month": 13, "day": 1}"#).is_err());
        assert!(LunarRecurrence::parse(r#"{"month": 1, "day": 31}"#).is_err());
        assert!(LunarRecurrence::parse("not json").is_err());
    }

    #[test]
    fn empty_range_and_out_of_table_bounds() {
        let rule = LunarRecurrence {
            month: 1,
            day: 1,
            leap_month: LeapMonthPolicy::Ignore,
        };
        assert!(rule.expand(d(2026, 5, 1), d(2026, 4, 1)).is_empty());
        // 范围超出表年份：返回空而不 panic
        assert!(rule.expand(d(1890, 1, 1), d(1895, 1, 1)).is_empty());
    }
}

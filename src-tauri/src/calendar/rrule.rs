//! RFC 5545 RRULE 有界子集（design D2）：解析与范围内展开。
//!
//! 支持：FREQ=DAILY/WEEKLY/MONTHLY/YEARLY、INTERVAL、COUNT、UNTIL（含当天）、
//! BYDAY（周）、BYMONTHDAY（月/年）。其余部件（WKST/BYSETPOS 等）明确报校验错误，
//! 不静默忽略；数据库保留原始字符串以便 M4 .ics 往返。

use chrono::{Datelike, NaiveDate, NaiveDateTime, Weekday};
use serde::{Deserialize, Serialize};

use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// 有界子集重复规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RRule {
    pub freq: Freq,
    #[serde(default = "default_interval")]
    pub interval: u32,
    pub count: Option<u32>,
    /// 结束日期（含当天，无论 DTSTART 时刻）。
    pub until: Option<NaiveDate>,
    /// 每周的星期（周重复；缺省为 DTSTART 当天星期）。
    #[serde(default)]
    pub by_day: Vec<Weekday>,
    /// 每月的日（月/年重复；缺省为 DTSTART 当天日）。不存在的日期跳过（RFC 语义）。
    #[serde(default)]
    pub by_month_day: Vec<i8>,
}

fn default_interval() -> u32 {
    1
}

impl RRule {
    /// 解析 RRULE 字符串（如 `FREQ=WEEKLY;INTERVAL=2;COUNT=4`）。
    pub fn parse(s: &str) -> VaultResult<RRule> {
        if s.trim().is_empty() {
            return Err(VaultError::Validation("重复规则为空".into()));
        }
        let mut freq: Option<Freq> = None;
        let mut interval = 1u32;
        let mut count = None;
        let mut until = None;
        let mut by_day = Vec::new();
        let mut by_month_day = Vec::new();
        for part in s.split(';') {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| VaultError::Validation(format!("重复规则部件非法: {part}")))?;
            let (key, value) = (key.trim().to_ascii_uppercase(), value.trim());
            match key.as_str() {
                "FREQ" => {
                    freq = Some(match value {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        other => {
                            return Err(VaultError::Validation(format!(
                                "不支持的 FREQ={other}（支持 DAILY/WEEKLY/MONTHLY/YEARLY）"
                            )))
                        }
                    });
                }
                "INTERVAL" => {
                    interval = value
                        .parse::<u32>()
                        .map_err(|_| VaultError::Validation(format!("INTERVAL={value} 非法")))?;
                    if interval == 0 {
                        return Err(VaultError::Validation("INTERVAL 必须 >= 1".into()));
                    }
                }
                "COUNT" => {
                    let c = value
                        .parse::<u32>()
                        .map_err(|_| VaultError::Validation(format!("COUNT={value} 非法")))?;
                    if c == 0 {
                        return Err(VaultError::Validation("COUNT 必须 >= 1".into()));
                    }
                    count = Some(c);
                }
                "UNTIL" => {
                    until = Some(parse_until(value)?);
                }
                "BYDAY" => {
                    by_day = value
                        .split(',')
                        .map(|v| match v {
                            "MO" => Ok(Weekday::Mon),
                            "TU" => Ok(Weekday::Tue),
                            "WE" => Ok(Weekday::Wed),
                            "TH" => Ok(Weekday::Thu),
                            "FR" => Ok(Weekday::Fri),
                            "SA" => Ok(Weekday::Sat),
                            "SU" => Ok(Weekday::Sun),
                            other => Err(VaultError::Validation(format!("BYDAY 值 {other} 非法"))),
                        })
                        .collect::<VaultResult<Vec<_>>>()?;
                }
                "BYMONTHDAY" => {
                    by_month_day = value
                        .split(',')
                        .map(|v| {
                            v.parse::<i8>().map_err(|_| {
                                VaultError::Validation(format!("BYMONTHDAY 值 {v} 非法"))
                            })
                        })
                        .collect::<VaultResult<Vec<_>>>()?;
                    if by_month_day.iter().any(|d| !(1..=31).contains(d)) {
                        return Err(VaultError::Validation("BYMONTHDAY 应在 1-31".into()));
                    }
                }
                other => {
                    return Err(VaultError::Validation(format!(
                        "不支持的重复规则部件 {other}（有界子集：FREQ/INTERVAL/COUNT/UNTIL/BYDAY/BYMONTHDAY）"
                    )));
                }
            }
        }
        let freq = freq.ok_or_else(|| VaultError::Validation("重复规则缺少 FREQ".into()))?;
        if count.is_some() && until.is_some() {
            return Err(VaultError::Validation("COUNT 与 UNTIL 不可同时出现".into()));
        }
        Ok(RRule {
            freq,
            interval,
            count,
            until,
            by_day,
            by_month_day,
        })
    }

    /// 序列化为 RRULE 字符串（M4 .ics 往返保留原语义）。
    pub fn to_rrule_string(&self) -> String {
        let mut parts = vec![format!(
            "FREQ={}",
            match self.freq {
                Freq::Daily => "DAILY",
                Freq::Weekly => "WEEKLY",
                Freq::Monthly => "MONTHLY",
                Freq::Yearly => "YEARLY",
            }
        )];
        if self.interval != 1 {
            parts.push(format!("INTERVAL={}", self.interval));
        }
        if let Some(c) = self.count {
            parts.push(format!("COUNT={c}"));
        }
        if let Some(u) = self.until {
            parts.push(format!("UNTIL={}", u.format("%Y%m%d")));
        }
        if !self.by_day.is_empty() {
            let names = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
            parts.push(format!(
                "BYDAY={}",
                self.by_day
                    .iter()
                    .map(|d| names[d.num_days_from_monday() as usize])
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.by_month_day.is_empty() {
            parts.push(format!(
                "BYMONTHDAY={}",
                self.by_month_day
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        parts.join(";")
    }

    /// 展开 [range_start, range_end] 内的实例（升序）。
    /// dtstart 为首次出现的日期时间；实例身份由 (dtstart, 规则) 唯一确定。
    pub fn expand(
        &self,
        dtstart: NaiveDateTime,
        range_start: NaiveDateTime,
        range_end: NaiveDateTime,
    ) -> Vec<NaiveDateTime> {
        let mut out = Vec::new();
        if range_end < range_start {
            return out;
        }
        let until_end = self
            .until
            .map(|d| d.and_hms_opt(23, 59, 59).expect("合法时刻"));
        if let Some(u) = until_end {
            if dtstart > u {
                return out;
            }
        }
        let mut emitted: u32 = 0; // 已遍历实例数（含范围外，COUNT 语义）
        let mut cycle: u64 = 0;
        loop {
            let batch = self.cycle_occurrences(dtstart, cycle);
            // 终止判定：本周期最早实例已超出范围（或本周期无合法实例且上界已超出）→ 后续周期更晚
            let done = match batch.first() {
                Some(earliest) => *earliest > range_end,
                None => self.cycle_bound(dtstart, cycle) > range_end,
            };
            if done {
                break;
            }
            for occ in batch {
                if let Some(u) = until_end {
                    if occ > u {
                        return out;
                    }
                }
                emitted += 1;
                if let Some(c) = self.count {
                    if emitted > c {
                        return out;
                    }
                }
                if occ >= range_start && occ <= range_end {
                    out.push(occ);
                }
            }
            cycle += 1;
            // 防御性上限：单查询不可能需要 10^6 个周期
            if cycle > 1_000_000 {
                break;
            }
        }
        out
    }

    /// 第 cycle 周期的结束上界（含时刻），保证每周期严格时间前进。
    fn cycle_bound(&self, dtstart: NaiveDateTime, cycle: u64) -> NaiveDateTime {
        let t = dtstart.time();
        match self.freq {
            Freq::Daily => dtstart + chrono::Duration::days(cycle as i64 * self.interval as i64),
            Freq::Weekly => {
                let monday = dtstart.date()
                    - chrono::Duration::days(dtstart.weekday().num_days_from_monday() as i64)
                    + chrono::Duration::weeks(cycle as i64 * self.interval as i64);
                (monday + chrono::Duration::days(6)).and_time(t)
            }
            Freq::Monthly => {
                let (y, m) = add_months(
                    dtstart.date().year(),
                    dtstart.date().month(),
                    cycle * self.interval as u64,
                );
                // 下月第一日 - 1 天 = 本月末日
                let (ny, nm) = add_months(y, m, 1);
                (NaiveDate::from_ymd_opt(ny, nm, 1).expect("1 日恒合法")
                    - chrono::Duration::days(1))
                .and_time(t)
            }
            Freq::Yearly => NaiveDate::from_ymd_opt(
                dtstart.date().year() + (cycle * self.interval as u64) as i32,
                12,
                31,
            )
            .expect("12-31 恒合法")
            .and_time(t),
        }
    }

    /// 第 cycle 个周期内的全部实例（升序，可能含早于 dtstart 的项由调用方过滤）。
    fn cycle_occurrences(&self, dtstart: NaiveDateTime, cycle: u64) -> Vec<NaiveDateTime> {
        match self.freq {
            Freq::Daily => {
                vec![dtstart + chrono::Duration::days(cycle as i64 * self.interval as i64)]
            }
            Freq::Weekly => {
                let days: Vec<Weekday> = if self.by_day.is_empty() {
                    vec![dtstart.weekday()]
                } else {
                    let mut d = self.by_day.clone();
                    d.sort_by_key(|w| w.num_days_from_monday());
                    d
                };
                // 周锚点：dtstart 所在周的周一（WKST=MO），每个 interval 周一步进
                let monday = dtstart.date()
                    - chrono::Duration::days(dtstart.weekday().num_days_from_monday() as i64);
                let week = monday + chrono::Duration::weeks(cycle as i64 * self.interval as i64);
                days.iter()
                    .map(|w| {
                        week.and_time(dtstart.time())
                            + chrono::Duration::days(w.num_days_from_monday() as i64)
                    })
                    .filter(|&occ| occ >= dtstart)
                    .collect()
            }
            Freq::Monthly => {
                let (year, month) = add_months(
                    dtstart.date().year(),
                    dtstart.date().month(),
                    cycle * self.interval as u64,
                );
                let doms: &[i8] = if self.by_month_day.is_empty() {
                    &[dtstart.date().day() as i8]
                } else {
                    &self.by_month_day
                };
                let mut v: Vec<NaiveDateTime> = doms
                    .iter()
                    .filter_map(|&dom| {
                        NaiveDate::from_ymd_opt(year, month, dom as u32)
                            .map(|d| d.and_time(dtstart.time()))
                    })
                    .filter(|&occ| occ >= dtstart)
                    .collect();
                v.sort();
                v
            }
            Freq::Yearly => {
                let year = dtstart.date().year() + (cycle * self.interval as u64) as i32;
                let month = dtstart.date().month();
                let doms: &[i8] = if self.by_month_day.is_empty() {
                    &[dtstart.date().day() as i8]
                } else {
                    &self.by_month_day
                };
                let mut v: Vec<NaiveDateTime> = doms
                    .iter()
                    .filter_map(|&dom| {
                        NaiveDate::from_ymd_opt(year, month, dom as u32)
                            .map(|d| d.and_time(dtstart.time()))
                    })
                    .filter(|&occ| occ >= dtstart)
                    .collect();
                v.sort();
                v
            }
        }
    }

    /// 供「完成重复任务重生」使用：从某日期起推进到下一个实例日期。
    pub fn next_after(&self, from: NaiveDate) -> Option<NaiveDate> {
        let dtstart = from.and_hms_opt(0, 0, 0).expect("合法时刻");
        let next = dtstart + chrono::Duration::days(1);
        self.expand(dtstart, next, next + chrono::Duration::days(366 * 10))
            .first()
            .map(|dt| dt.date())
    }
}

/// (year, month) 前进 n 个月。
fn add_months(year: i32, month: u32, n: u64) -> (i32, u32) {
    let total = year as i64 * 12 + (month as i64 - 1) + n as i64;
    (
        (total.div_euclid(12)) as i32,
        (total.rem_euclid(12) + 1) as u32,
    )
}

/// UNTIL：YYYYMMDD 或 YYYYMMDDTHHMMSS（取日期部分）。
fn parse_until(value: &str) -> VaultResult<NaiveDate> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return Err(VaultError::Validation(format!("UNTIL={value} 非法")));
    }
    let y: i32 = digits[..4]
        .parse()
        .map_err(|_| VaultError::Validation(format!("UNTIL={value} 非法")))?;
    let m: u32 = digits[4..6]
        .parse()
        .map_err(|_| VaultError::Validation(format!("UNTIL={value} 非法")))?;
    let d: u32 = digits[6..8]
        .parse()
        .map_err(|_| VaultError::Validation(format!("UNTIL={value} 非法")))?;
    NaiveDate::from_ymd_opt(y, m, d)
        .ok_or_else(|| VaultError::Validation(format!("UNTIL={value} 非法")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn daily_with_interval_and_count() {
        let rule = RRule::parse("FREQ=DAILY;INTERVAL=2;COUNT=3").unwrap();
        let occ = rule.expand(
            dt(2026, 3, 1, 9, 0),
            dt(2026, 3, 1, 0, 0),
            dt(2026, 3, 31, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2026, 3, 1, 9, 0),
                dt(2026, 3, 3, 9, 0),
                dt(2026, 3, 5, 9, 0)
            ]
        );
        // 范围裁剪：从 3-2 起
        let clipped = rule.expand(
            dt(2026, 3, 1, 9, 0),
            dt(2026, 3, 2, 0, 0),
            dt(2026, 3, 31, 23, 59),
        );
        assert_eq!(clipped, vec![dt(2026, 3, 3, 9, 0), dt(2026, 3, 5, 9, 0)]);
    }

    #[test]
    fn weekly_byday_multi() {
        let rule = RRule::parse("FREQ=WEEKLY;BYDAY=MO,WE").unwrap();
        let occ = rule.expand(
            dt(2026, 3, 2, 9, 0),
            dt(2026, 3, 1, 0, 0),
            dt(2026, 3, 15, 23, 59),
        );
        // 2026-03-02 周一、03-04 周三、03-09、03-11
        assert_eq!(
            occ,
            vec![
                dt(2026, 3, 2, 9, 0),
                dt(2026, 3, 4, 9, 0),
                dt(2026, 3, 9, 9, 0),
                dt(2026, 3, 11, 9, 0),
            ]
        );
    }

    #[test]
    fn weekly_default_uses_dtstart_weekday() {
        let rule = RRule::parse("FREQ=WEEKLY;INTERVAL=2").unwrap();
        // DTSTART 周四
        let occ = rule.expand(
            dt(2026, 3, 5, 14, 0),
            dt(2026, 3, 1, 0, 0),
            dt(2026, 4, 30, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2026, 3, 5, 14, 0),
                dt(2026, 3, 19, 14, 0),
                dt(2026, 4, 2, 14, 0),
                dt(2026, 4, 16, 14, 0),
                dt(2026, 4, 30, 14, 0),
            ]
        );
    }

    #[test]
    fn monthly_skips_short_months() {
        // 1 月 31 日起的月重复：2 月（28 天）与 4 月（30 天）跳过
        let rule = RRule::parse("FREQ=MONTHLY").unwrap();
        let occ = rule.expand(
            dt(2026, 1, 31, 8, 0),
            dt(2026, 1, 1, 0, 0),
            dt(2026, 12, 31, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2026, 1, 31, 8, 0),
                dt(2026, 3, 31, 8, 0),
                dt(2026, 5, 31, 8, 0),
                dt(2026, 7, 31, 8, 0),
                dt(2026, 8, 31, 8, 0),
                dt(2026, 10, 31, 8, 0),
                dt(2026, 12, 31, 8, 0),
            ]
        );
    }

    #[test]
    fn monthly_by_month_day() {
        let rule = RRule::parse("FREQ=MONTHLY;BYMONTHDAY=15,25").unwrap();
        let occ = rule.expand(
            dt(2026, 1, 10, 0, 0),
            dt(2026, 1, 1, 0, 0),
            dt(2026, 2, 28, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2026, 1, 15, 0, 0),
                dt(2026, 1, 25, 0, 0),
                dt(2026, 2, 15, 0, 0),
                dt(2026, 2, 25, 0, 0)
            ]
        );
    }

    #[test]
    fn yearly_feb29_only_leap_years() {
        let rule = RRule::parse("FREQ=YEARLY").unwrap();
        let occ = rule.expand(
            dt(2024, 2, 29, 10, 0),
            dt(2024, 1, 1, 0, 0),
            dt(2040, 12, 31, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2024, 2, 29, 10, 0),
                dt(2028, 2, 29, 10, 0),
                dt(2032, 2, 29, 10, 0),
                dt(2036, 2, 29, 10, 0),
                dt(2040, 2, 29, 10, 0),
            ]
        );
    }

    #[test]
    fn until_is_inclusive_of_last_day() {
        let rule = RRule::parse("FREQ=DAILY;UNTIL=20260310").unwrap();
        let occ = rule.expand(
            dt(2026, 3, 8, 22, 30),
            dt(2026, 3, 1, 0, 0),
            dt(2026, 12, 31, 23, 59),
        );
        assert_eq!(
            occ,
            vec![
                dt(2026, 3, 8, 22, 30),
                dt(2026, 3, 9, 22, 30),
                dt(2026, 3, 10, 22, 30)
            ]
        );
        // UNTIL 早于 DTSTART → 空
        let before = RRule::parse("FREQ=DAILY;UNTIL=20260301").unwrap();
        assert!(before
            .expand(
                dt(2026, 3, 8, 0, 0),
                dt(2026, 3, 1, 0, 0),
                dt(2026, 3, 31, 23, 59)
            )
            .is_empty());
    }

    #[test]
    fn invalid_rules_rejected() {
        assert!(matches!(RRule::parse(""), Err(VaultError::Validation(_))));
        assert!(matches!(
            RRule::parse("INTERVAL=2"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=HOURLY"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;WKST=SU"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;COUNT=0"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;INTERVAL=0"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;UNTIL=20261301"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;BYDAY=XX"),
            Err(VaultError::Validation(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;COUNT=2;UNTIL=20260301"),
            Err(VaultError::Validation(_))
        ));
    }

    #[test]
    fn rrule_string_round_trip() {
        for s in [
            "FREQ=DAILY",
            "FREQ=WEEKLY;INTERVAL=2;COUNT=4",
            "FREQ=WEEKLY;BYDAY=MO,WE,FR",
            "FREQ=MONTHLY;BYMONTHDAY=1,15;UNTIL=20300101",
        ] {
            let rule = RRule::parse(s).unwrap();
            // 语义往返：序列化后再解析得到等价规则，且二次序列化稳定
            let re = RRule::parse(&rule.to_rrule_string()).unwrap();
            assert_eq!(re, rule, "往返应保留语义");
            assert_eq!(re.to_rrule_string(), rule.to_rrule_string(), "序列化应稳定");
        }
    }

    #[test]
    fn next_after_for_task_recurrence() {
        // 每周重复：2026-03-02 完成后下一实例 03-09
        let rule = RRule::parse("FREQ=WEEKLY").unwrap();
        assert_eq!(
            rule.next_after(NaiveDate::from_ymd_opt(2026, 3, 2).unwrap()),
            Some(NaiveDate::from_ymd_opt(2026, 3, 9).unwrap())
        );
        // 每月 31 日（从 1-31）：2 月跳过 → 3-31
        let monthly = RRule::parse("FREQ=MONTHLY").unwrap();
        assert_eq!(
            monthly.next_after(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
            Some(NaiveDate::from_ymd_opt(2026, 3, 31).unwrap())
        );
    }

    #[test]
    fn time_component_preserved() {
        let rule = RRule::parse("FREQ=DAILY;COUNT=2").unwrap();
        let occ = rule.expand(
            dt(2026, 3, 1, 7, 45),
            dt(2026, 3, 1, 0, 0),
            dt(2026, 3, 31, 23, 59),
        );
        assert_eq!(occ[0].time(), NaiveTime::from_hms_opt(7, 45, 0).unwrap());
        assert_eq!(occ[1].time(), NaiveTime::from_hms_opt(7, 45, 0).unwrap());
    }
}

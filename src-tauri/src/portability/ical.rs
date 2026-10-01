//! Conservative iCalendar boundary adapter.  It deliberately accepts the RFC 5545 subset the
//! calendar domain can represent and turns all unsupported properties into visible warnings.

use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;

use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IcalEventPlan {
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: String,
    pub all_day: bool,
    pub timezone: Option<String>,
    pub recurrence_rule: Option<String>,
    pub recurrence_id: Option<String>,
    pub exdates: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct IcalImportPlan {
    pub events: Vec<IcalEventPlan>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcalException {
    pub original_start_at: String,
    pub new_start_at: Option<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcalExportEvent {
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: String,
    pub all_day: bool,
    pub timezone: Option<String>,
    pub recurrence_rule: Option<String>,
    pub exceptions: Vec<IcalException>,
    /// A stable grouping id for materialized series. RFC 5545 clients which do not know the
    /// Tenjee extension still see ordinary, independent VEVENTs.
    pub related_to: Option<String>,
    pub lunar_series: Option<String>,
}

/// Render the Gregorian calendar subset as a UTF-8 VCALENDAR.  Each exception becomes an
/// override VEVENT, so both cancellations and reschedules remain explicit on re-import.
pub fn render(events: &[IcalExportEvent]) -> VaultResult<String> {
    let mut properties = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Tenjee Vault//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
    ];
    for event in events {
        properties.push("BEGIN:VEVENT".to_string());
        properties.push(format!("UID:{}", escape(&event.uid)));
        properties.push(format!("SUMMARY:{}", escape(&event.title)));
        if let Some(value) = &event.description {
            properties.push(format!("DESCRIPTION:{}", escape(value)));
        }
        if let Some(value) = &event.location {
            properties.push(format!("LOCATION:{}", escape(value)));
        }
        properties.push(format_datetime(
            "DTSTART",
            &event.start_at,
            event.all_day,
            event.timezone.as_deref(),
        )?);
        properties.push(format_datetime(
            "DTEND",
            &event.end_at,
            event.all_day,
            event.timezone.as_deref(),
        )?);
        if let Some(rule) = &event.recurrence_rule {
            crate::calendar::rrule::RRule::parse(rule)?;
            properties.push(format!("RRULE:{rule}"));
        }
        if let Some(related_to) = &event.related_to {
            properties.push(format!("RELATED-TO:{}", escape(related_to)));
        }
        if let Some(series) = &event.lunar_series {
            properties.push(format!("X-TENJEE-LUNAR-SERIES:{}", escape(series)));
        }
        properties.push("END:VEVENT".to_string());
        for exception in &event.exceptions {
            properties.push("BEGIN:VEVENT".to_string());
            properties.push(format!("UID:{}", escape(&event.uid)));
            properties.push(format_datetime(
                "RECURRENCE-ID",
                &exception.original_start_at,
                event.all_day,
                event.timezone.as_deref(),
            )?);
            if exception.cancelled {
                properties.push("STATUS:CANCELLED".to_string());
                properties.push(format_datetime(
                    "DTSTART",
                    &exception.original_start_at,
                    event.all_day,
                    event.timezone.as_deref(),
                )?);
            } else if let Some(new_start) = &exception.new_start_at {
                properties.push(format_datetime(
                    "DTSTART",
                    new_start,
                    event.all_day,
                    event.timezone.as_deref(),
                )?);
                properties.push(format_datetime(
                    "DTEND",
                    &shift_end(&event.start_at, &event.end_at, new_start, event.all_day)?,
                    event.all_day,
                    event.timezone.as_deref(),
                )?);
            } else {
                return Err(VaultError::Validation("日程例外缺少改期时间".into()));
            }
            properties.push("END:VEVENT".to_string());
        }
    }
    properties.push("END:VCALENDAR".to_string());
    Ok(properties
        .into_iter()
        .flat_map(|line| fold_line(&line))
        .collect())
}

fn format_datetime(
    name: &str,
    value: &str,
    all_day: bool,
    timezone: Option<&str>,
) -> VaultResult<String> {
    if all_day {
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation(format!("日程日期非法: {value}")))?;
        let value = if name == "DTEND" {
            date + chrono::Duration::days(1)
        } else {
            date
        };
        return Ok(format!("{name};VALUE=DATE:{}", value.format("%Y%m%d")));
    }
    let value = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| VaultError::Validation(format!("日程时间非法: {value}")))?;
    let param = timezone
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!(";TZID={value}"))
        .unwrap_or_default();
    Ok(format!("{name}{param}:{}", value.format("%Y%m%dT%H%M%S")))
}

fn shift_end(
    original_start: &str,
    original_end: &str,
    new_start: &str,
    all_day: bool,
) -> VaultResult<String> {
    if all_day {
        let start = NaiveDate::parse_from_str(original_start, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation("原全天事件日期非法".into()))?;
        let end = NaiveDate::parse_from_str(original_end, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation("原全天事件日期非法".into()))?;
        let new = NaiveDate::parse_from_str(new_start, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation("改期全天事件日期非法".into()))?;
        return Ok((new + (end - start)).format("%Y-%m-%d").to_string());
    }
    let start = NaiveDateTime::parse_from_str(original_start, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| VaultError::Validation("原事件时间非法".into()))?;
    let end = NaiveDateTime::parse_from_str(original_end, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| VaultError::Validation("原事件时间非法".into()))?;
    let new = NaiveDateTime::parse_from_str(new_start, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| VaultError::Validation("改期事件时间非法".into()))?;
    Ok((new + (end - start))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string())
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

fn fold_line(line: &str) -> Vec<String> {
    const LIMIT: usize = 75;
    let mut out = Vec::new();
    let mut current = String::new();
    for character in line.chars() {
        let limit = if out.is_empty() { LIMIT } else { LIMIT - 1 };
        if current.len() + character.len_utf8() > limit {
            out.push(format!("{current}\r\n"));
            current = " ".to_string();
        }
        current.push(character);
    }
    out.push(format!("{current}\r\n"));
    out
}

/// Parse a UTF-8 VCALENDAR into a write-free import plan. Folded lines and escaped text are
/// handled before any database operation. An invalid VEVENT is skipped with a warning, so a
/// mixed external file can still import its independent valid events without partial writes.
pub fn parse(input: &str) -> VaultResult<IcalImportPlan> {
    let lines = unfold(input);
    let mut plan = IcalImportPlan::default();
    let mut current: Vec<(String, Vec<(String, String)>, String)> = Vec::new();
    let mut in_event = false;
    for line in lines {
        match line.as_str() {
            "BEGIN:VEVENT" => {
                if in_event {
                    return Err(VaultError::Validation("iCalendar VEVENT 嵌套非法".into()));
                }
                in_event = true;
                current.clear();
            }
            "END:VEVENT" => {
                if !in_event {
                    return Err(VaultError::Validation(
                        "iCalendar VEVENT 结束位置非法".into(),
                    ));
                }
                match parse_event(&current, &mut plan.warnings) {
                    Ok(event) => plan.events.push(event),
                    Err(error) => plan.warnings.push(format!("已跳过无效 VEVENT: {error}")),
                }
                in_event = false;
            }
            _ if in_event => match property(&line) {
                Ok(property) => current.push(property),
                Err(error) => plan
                    .warnings
                    .push(format!("已跳过无效 VEVENT 属性: {error}")),
            },
            _ => {}
        }
    }
    if in_event {
        return Err(VaultError::Validation("iCalendar 缺少 END:VEVENT".into()));
    }
    if plan.events.is_empty() {
        return Err(VaultError::Validation("iCalendar 不含 VEVENT".into()));
    }
    Ok(plan)
}

fn unfold(input: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in input.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if let Some(previous) = lines
            .last_mut()
            .filter(|_| raw.starts_with(' ') || raw.starts_with('\t'))
        {
            previous.push_str(raw.trim_start());
        } else {
            lines.push(raw.to_owned());
        }
    }
    lines
}

fn property(line: &str) -> VaultResult<(String, Vec<(String, String)>, String)> {
    let (head, value) = line
        .split_once(':')
        .ok_or_else(|| VaultError::Validation(format!("iCalendar 属性缺少冒号: {line}")))?;
    let mut pieces = head.split(';');
    let name = pieces.next().unwrap_or_default().to_ascii_uppercase();
    if name.is_empty() {
        return Err(VaultError::Validation("iCalendar 属性名为空".into()));
    }
    let params = pieces
        .filter_map(|part| {
            part.split_once('=')
                .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        })
        .collect();
    Ok((name, params, value.to_owned()))
}

fn parse_event(
    properties: &[(String, Vec<(String, String)>, String)],
    warnings: &mut Vec<String>,
) -> VaultResult<IcalEventPlan> {
    let get = |name: &str| properties.iter().find(|(key, _, _)| key == name);
    let uid = get("UID")
        .map(|(_, _, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| VaultError::Validation("VEVENT 缺少 UID".into()))?;
    let start = get("DTSTART")
        .ok_or_else(|| VaultError::Validation(format!("VEVENT {uid} 缺少 DTSTART")))?;
    let (start_at, all_day) = parse_datetime(&start.2, &start.1)?;
    let end_at = match get("DTEND") {
        Some((_, params, value)) => {
            let (value, end_all_day) = parse_datetime(value, params)?;
            if end_all_day != all_day {
                return Err(VaultError::Validation(format!(
                    "VEVENT {uid} DTSTART/DTEND 类型不一致"
                )));
            }
            if all_day {
                let end = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
                    .map_err(|_| VaultError::Validation("全天事件 DTEND 非法".into()))?;
                (end - chrono::Duration::days(1))
                    .format("%Y-%m-%d")
                    .to_string()
            } else {
                value
            }
        }
        None => start_at.clone(),
    };
    let timezone = start
        .1
        .iter()
        .find(|(key, _)| key == "TZID")
        .map(|(_, value)| value.clone());
    let recurrence_rule = get("RRULE")
        .map(|(_, _, value)| value.clone())
        .map(|rule| crate::calendar::rrule::RRule::parse(&rule).map(|_| rule))
        .transpose()?;
    let recurrence_id = get("RECURRENCE-ID")
        .map(|(_, params, value)| parse_datetime(value, params).map(|(value, _)| value))
        .transpose()?;
    let exdates = properties
        .iter()
        .filter(|(key, _, _)| key == "EXDATE")
        .flat_map(|(_, params, value)| {
            value
                .split(',')
                .map(move |value| parse_datetime(value, params).map(|(value, _)| value))
        })
        .collect::<VaultResult<Vec<_>>>()?;
    for (name, _, _) in properties {
        if !matches!(
            name.as_str(),
            "UID"
                | "SUMMARY"
                | "DESCRIPTION"
                | "LOCATION"
                | "DTSTART"
                | "DTEND"
                | "RRULE"
                | "RECURRENCE-ID"
                | "EXDATE"
                | "STATUS"
                | "DTSTAMP"
                | "CREATED"
                | "LAST-MODIFIED"
                | "SEQUENCE"
        ) {
            warnings.push(format!("VEVENT {uid}: 已忽略不支持的属性 {name}"));
        }
    }
    Ok(IcalEventPlan {
        uid,
        title: get("SUMMARY")
            .map(|(_, _, value)| unescape(value))
            .unwrap_or_else(|| "(无标题事件)".into()),
        description: get("DESCRIPTION").map(|(_, _, value)| unescape(value)),
        location: get("LOCATION").map(|(_, _, value)| unescape(value)),
        start_at,
        end_at,
        all_day,
        timezone,
        recurrence_rule,
        recurrence_id,
        exdates,
        cancelled: get("STATUS")
            .is_some_and(|(_, _, value)| value.eq_ignore_ascii_case("CANCELLED")),
    })
}

fn parse_datetime(value: &str, params: &[(String, String)]) -> VaultResult<(String, bool)> {
    let all_day = params
        .iter()
        .any(|(key, value)| key == "VALUE" && value.eq_ignore_ascii_case("DATE"))
        || (value.len() == 8 && value.chars().all(|ch| ch.is_ascii_digit()));
    if all_day {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d")
            .map_err(|_| VaultError::Validation(format!("iCalendar 日期非法: {value}")))?;
        return Ok((date.format("%Y-%m-%d").to_string(), true));
    }
    let value = value.strip_suffix('Z').unwrap_or(value);
    let datetime = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
        .map_err(|_| VaultError::Validation(format!("iCalendar 时间非法: {value}")))?;
    Ok((datetime.format("%Y-%m-%dT%H:%M:%S").to_string(), false))
}

fn unescape(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_folded_recurrence_and_warns_for_unknown_properties() {
        let plan = parse("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:demo\r\nSUMMARY:Team\\, sync\r\nDTSTART;TZID=Asia/Hong_Kong:20260302T090000\r\nDTEND;TZID=Asia/Hong_Kong:20260302T093000\r\nRRULE:FREQ=WEEKLY;COUNT=2\r\nEXDATE;TZID=Asia/Hong_Kong:20260309T090000\r\nX-CLIENT:ignored\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").unwrap();
        assert_eq!(plan.events[0].title, "Team, sync");
        assert_eq!(plan.events[0].timezone.as_deref(), Some("Asia/Hong_Kong"));
        assert_eq!(
            plan.events[0].recurrence_rule.as_deref(),
            Some("FREQ=WEEKLY;COUNT=2")
        );
        assert_eq!(plan.events[0].exdates, ["2026-03-09T09:00:00"]);
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn keeps_valid_events_when_a_mixed_file_contains_an_invalid_vevent() {
        let plan = parse("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:good\nSUMMARY:Good\nDTSTART:20260302T090000\nDTEND:20260302T100000\nEND:VEVENT\nBEGIN:VEVENT\nUID:bad\nSUMMARY:Bad\nDTSTART:not-a-date\nEND:VEVENT\nEND:VCALENDAR").unwrap();
        assert_eq!(plan.events.len(), 1);
        assert_eq!(plan.events[0].uid, "good");
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("已跳过无效 VEVENT")));
    }
}

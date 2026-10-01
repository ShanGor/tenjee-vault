//! 事件管理（spec: 事件管理 / 事件重复规则 / 事件提醒 / 拖拽创建与调整 /
//! 事件关联笔记与任务 / 日程搜索）。
//!
//! 重复展开（design D2/D3）：RRULE 与农历重复（lunar_rule）统一展开为实例，
//! 例外表（取消剔除、改期替换）在查询时应用；实例身份 = event_id + original_start_at。
//! 全天事件 start_at/end_at 存日期（'YYYY-MM-DD'），定时事件存 'YYYY-MM-DDTHH:MM:SS'；
//! 例外键与实例键同格式（全天为日期串）。

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::lunar_rule::LunarRecurrence;
use super::rrule::RRule;
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: String,
    pub all_day: bool,
    pub timezone: Option<String>,
    pub recurrence_rule: Option<String>,
    pub lunar_recurrence: Option<String>,
    pub color: Option<String>,
    pub linked_page_ref: Option<String>,
    pub linked_task_id: Option<String>,
    pub external_uid: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 展开后的事件实例（视图条目）。
#[derive(Debug, Clone, Serialize)]
pub struct EventInstance {
    #[serde(flatten)]
    pub event: Event,
    /// 规则原始出现时刻（例外键）；非重复事件等于 start_at。
    pub original_start_at: String,
    /// 应用例外后的实际起止。
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventReminder {
    pub id: String,
    pub event_id: String,
    pub minutes_before: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinkResolution {
    pub reference: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EventLinkState {
    pub page: Option<LinkResolution>,
    pub task: Option<LinkResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcalConflictPolicy {
    Update,
    Copy,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct IcalImportResult {
    pub created: usize,
    pub updated: usize,
    pub copied: usize,
}

/// 可编辑字段补丁（None = 不修改）。
#[derive(Debug, Clone, Default)]
pub struct EventPatch {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub all_day: Option<bool>,
    pub timezone: Option<Option<String>>,
    pub recurrence_rule: Option<Option<String>>,
    pub lunar_recurrence: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub linked_page_ref: Option<Option<String>>,
    pub linked_task_id: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayInfo {
    pub date: String,
    /// 农历日显示：初一为月名（正月/闰六月），其余为日名（初二/十五/廿三…）。
    pub lunar_day: String,
    pub lunar_month: Option<u8>,
    pub lunar_date: Option<u8>,
    pub lunar_leap: bool,
    /// 节气名（如清明/冬至）。
    pub term: Option<String>,
    /// 传统节日名（如春节/中秋）。
    pub festival: Option<String>,
}

fn row_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?,
        title: r.get(1)?,
        description: r.get(2)?,
        location: r.get(3)?,
        start_at: r.get(4)?,
        end_at: r.get(5)?,
        all_day: r.get::<_, i64>(6)? != 0,
        timezone: r.get(7)?,
        recurrence_rule: r.get(8)?,
        lunar_recurrence: r.get(9)?,
        color: r.get(10)?,
        linked_page_ref: r.get(11)?,
        linked_task_id: r.get(12)?,
        external_uid: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

const SELECT_EVENT: &str = "SELECT id, title, description, location, start_at, end_at, all_day, timezone, recurrence_rule, lunar_recurrence, color, linked_page_ref, linked_task_id, external_uid, created_at, updated_at FROM events";

pub fn get_event(conn: &Connection, id: &str) -> VaultResult<Event> {
    conn.query_row(
        &format!("{SELECT_EVENT} WHERE id = ?1"),
        params![id],
        row_event,
    )
    .optional()?
    .ok_or_else(|| VaultError::NotFound(format!("事件 {id}")))
}

/// Export selected (or all) events. Lunar rules have no RFC 5545 equivalent, so they are
/// materialized into Gregorian VEVENTs only when the caller supplies a finite date range.
/// Each materialized event carries the same `RELATED-TO` and private series marker; imports
/// intentionally treat them as independent Gregorian events rather than guessing a lunar rule.
pub fn export_ical(
    conn: &Connection,
    ids: Option<&[String]>,
    range: Option<(NaiveDate, NaiveDate)>,
) -> VaultResult<String> {
    if let Some((start, end)) = range {
        if end < start {
            return Err(VaultError::Validation(
                "导出范围结束日期不得早于开始日期".into(),
            ));
        }
    }
    let mut sql = SELECT_EVENT.to_string();
    if let Some(ids) = ids {
        if ids.is_empty() {
            return Ok(crate::portability::ical::render(&[])?);
        }
        sql.push_str(&format!(
            " WHERE id IN ({})",
            ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
        ));
    }
    sql.push_str(" ORDER BY start_at, id");
    let events = match ids {
        Some(ids) => conn
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(ids), row_event)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        None => conn
            .prepare(&sql)?
            .query_map([], row_event)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    if events.iter().any(|event| event.lunar_recurrence.is_some()) && range.is_none() {
        return Err(VaultError::Validation(
            "农历重复事件导出必须指定有限开始和结束日期".into(),
        ));
    }

    // A date-range export is a bounded interchange snapshot. Materializing all recurring
    // occurrences prevents a recipient from seeing instances outside the user-selected range;
    // an unbounded export below retains the native Gregorian RRULE representation instead.
    if let Some((range_start, range_end)) = range {
        let selected_ids: std::collections::HashSet<&str> =
            events.iter().map(|event| event.id.as_str()).collect();
        let mut materialized = Vec::new();
        for instance in instances_in_range(conn, range_start, range_end)? {
            if !selected_ids.contains(instance.event.id.as_str()) {
                continue;
            }
            let repeating = instance.event.recurrence_rule.is_some()
                || instance.event.lunar_recurrence.is_some();
            let series = instance
                .event
                .external_uid
                .clone()
                .unwrap_or_else(|| format!("{}@tenjee-vault.local", instance.event.id));
            materialized.push(crate::portability::ical::IcalExportEvent {
                uid: if repeating {
                    format!(
                        "{}-{}@tenjee-vault.local",
                        instance.event.id, instance.original_start_at
                    )
                } else {
                    series.clone()
                },
                title: instance.event.title,
                description: instance.event.description,
                location: instance.event.location,
                start_at: instance.start_at,
                end_at: instance.end_at,
                all_day: instance.event.all_day,
                timezone: instance.event.timezone,
                recurrence_rule: None,
                exceptions: Vec::new(),
                related_to: repeating.then_some(series.clone()),
                lunar_series: instance.event.lunar_recurrence.is_some().then_some(series),
            });
        }
        return crate::portability::ical::render(&materialized);
    }

    let mut exported = Vec::with_capacity(events.len());
    for event in events
        .iter()
        .filter(|event| event.lunar_recurrence.is_none())
    {
        let mut statement = conn.prepare("SELECT original_start_at, new_start_at, is_cancelled FROM event_exceptions WHERE event_id = ?1 ORDER BY original_start_at")?;
        let exceptions = statement
            .query_map(params![event.id], |row| {
                Ok(crate::portability::ical::IcalException {
                    original_start_at: row.get(0)?,
                    new_start_at: row.get(1)?,
                    cancelled: row.get::<_, i64>(2)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        exported.push(crate::portability::ical::IcalExportEvent {
            uid: event
                .external_uid
                .clone()
                .unwrap_or_else(|| format!("{}@tenjee-vault.local", event.id)),
            title: event.title.clone(),
            description: event.description.clone(),
            location: event.location.clone(),
            start_at: event.start_at.clone(),
            end_at: event.end_at.clone(),
            all_day: event.all_day,
            timezone: event.timezone.clone(),
            recurrence_rule: event.recurrence_rule.clone(),
            exceptions,
            related_to: None,
            lunar_series: None,
        });
    }
    crate::portability::ical::render(&exported)
}

/// Resolve cross-database references lazily. Missing targets are represented by
/// `exists: false`; they never make reading the event fail.
pub fn resolve_links<F>(
    event: &Event,
    tasks_conn: &Connection,
    page_exists: F,
) -> VaultResult<EventLinkState>
where
    F: FnOnce(&str) -> VaultResult<bool>,
{
    let page = match &event.linked_page_ref {
        Some(reference) => Some(LinkResolution {
            reference: reference.clone(),
            exists: page_exists(reference).unwrap_or(false),
        }),
        None => None,
    };
    let task = match &event.linked_task_id {
        Some(reference) => {
            let exists = tasks_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
                    params![reference],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false);
            Some(LinkResolution {
                reference: reference.clone(),
                exists,
            })
        }
        None => None,
    };
    Ok(EventLinkState { page, task })
}

/// 事件起止的时刻表示：全天 = 当日 00:00（起）/ 次日 00:00（止，排他）；
/// 定时 = 解析出的日期时间。
#[derive(Debug, Clone, Copy)]
struct Span {
    start: NaiveDateTime,
    /// 排他结束（all-day 为次日 00:00）。
    end: NaiveDateTime,
}

fn parse_span(all_day: bool, start_at: &str, end_at: &str) -> VaultResult<Span> {
    let start = if all_day {
        let d = NaiveDate::parse_from_str(start_at, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation(format!("开始日期 {start_at} 非法")))?;
        d.and_hms_opt(0, 0, 0).expect("合法时刻")
    } else {
        NaiveDateTime::parse_from_str(start_at, "%Y-%m-%dT%H:%M:%S")
            .map_err(|_| VaultError::Validation(format!("开始时间 {start_at} 非法")))?
    };
    let end = if all_day {
        let d = NaiveDate::parse_from_str(end_at, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation(format!("结束日期 {end_at} 非法")))?;
        (d + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .expect("合法时刻")
    } else {
        NaiveDateTime::parse_from_str(end_at, "%Y-%m-%dT%H:%M:%S")
            .map_err(|_| VaultError::Validation(format!("结束时间 {end_at} 非法")))?
    };
    Ok(Span { start, end })
}

fn format_start(all_day: bool, dt: NaiveDateTime) -> String {
    if all_day {
        dt.format("%Y-%m-%d").to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

fn parse_start(all_day: bool, value: &str) -> VaultResult<NaiveDateTime> {
    if all_day {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation(format!("开始日期 {value} 非法")))?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| VaultError::Validation(format!("开始日期 {value} 非法")))
    } else {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
            .map_err(|_| VaultError::Validation(format!("开始时间 {value} 非法")))
    }
}

fn format_end(all_day: bool, dt: NaiveDateTime) -> String {
    if all_day {
        // All-day end dates are stored inclusively.
        (dt - chrono::Duration::seconds(1))
            .format("%Y-%m-%d")
            .to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

fn validate_event_input(all_day: bool, start_at: &str, end_at: &str) -> VaultResult<()> {
    let span = parse_span(all_day, start_at, end_at)?;
    if span.end < span.start {
        return Err(VaultError::Validation("结束时间不得早于开始时间".into()));
    }
    Ok(())
}

/// 新建事件。重复规则（RRULE/农历）若提供需可解析。
pub fn create_event(
    conn: &Connection,
    title: &str,
    description: Option<&str>,
    location: Option<&str>,
    start_at: &str,
    end_at: &str,
    all_day: bool,
    timezone: Option<&str>,
    recurrence_rule: Option<&str>,
    lunar_recurrence: Option<&str>,
    color: Option<&str>,
    linked_page_ref: Option<&str>,
    linked_task_id: Option<&str>,
) -> VaultResult<Event> {
    let title = title.trim();
    if title.is_empty() {
        return Err(VaultError::Validation("事件标题不能为空".into()));
    }
    validate_event_input(all_day, start_at, end_at)?;
    if let Some(rule) = recurrence_rule {
        RRule::parse(rule)?;
    }
    if let Some(rule) = lunar_recurrence {
        LunarRecurrence::parse(rule)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO events (id, title, description, location, start_at, end_at, all_day, timezone, recurrence_rule, lunar_recurrence, color, linked_page_ref, linked_task_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            id, title, description, location, start_at, end_at, all_day as i64, timezone,
            recurrence_rule, lunar_recurrence, color, linked_page_ref, linked_task_id
        ],
    )?;
    get_event(conn, &id)
}

/// Apply a fully-parsed iCalendar import plan in one calendar transaction. Existing UIDs are
/// never silently duplicated: callers explicitly select update (preserve internal id and local
/// associations) or copy (fresh internal and external UID).
pub fn import_ical_plan(
    conn: &mut Connection,
    plan: &crate::portability::ical::IcalImportPlan,
    policy: IcalConflictPolicy,
) -> VaultResult<IcalImportResult> {
    let tx = conn.unchecked_transaction()?;
    let mut result = IcalImportResult::default();
    let mut ids = std::collections::HashMap::new();
    for item in plan
        .events
        .iter()
        .filter(|item| item.recurrence_id.is_none())
    {
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM events WHERE external_uid = ?1",
                params![item.uid],
                |row| row.get(0),
            )
            .optional()?;
        let (id, uid) = match existing {
            Some(id) if policy == IcalConflictPolicy::Update => {
                update_imported_event(&tx, &id, item)?;
                result.updated += 1;
                (id, item.uid.clone())
            }
            Some(_) => {
                let uid = format!("{}-copy-{}", item.uid, uuid::Uuid::new_v4());
                let id = insert_imported_event(&tx, item, &uid)?;
                result.copied += 1;
                (id, uid)
            }
            None => {
                let id = insert_imported_event(&tx, item, &item.uid)?;
                result.created += 1;
                (id, item.uid.clone())
            }
        };
        replace_ical_exdates(&tx, &id, &item.exdates)?;
        ids.insert(item.uid.clone(), (id, uid));
    }
    for item in plan
        .events
        .iter()
        .filter(|item| item.recurrence_id.is_some())
    {
        let Some((event_id, _)) = ids.get(&item.uid) else {
            continue;
        };
        let original = item.recurrence_id.as_deref().expect("已筛选");
        tx.execute(
            "DELETE FROM event_exceptions WHERE event_id = ?1 AND original_start_at = ?2",
            params![event_id, original],
        )?;
        tx.execute(
            "INSERT INTO event_exceptions (id, event_id, original_start_at, new_start_at, is_cancelled) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![uuid::Uuid::new_v4().to_string(), event_id, original, (!item.cancelled).then_some(&item.start_at), item.cancelled as i64],
        )?;
    }
    tx.commit()?;
    Ok(result)
}

fn insert_imported_event(
    tx: &rusqlite::Transaction<'_>,
    item: &crate::portability::ical::IcalEventPlan,
    uid: &str,
) -> VaultResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO events (id, title, description, location, start_at, end_at, all_day, timezone, recurrence_rule, external_uid) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, item.title, item.description, item.location, item.start_at, item.end_at, item.all_day as i64, item.timezone, item.recurrence_rule, uid],
    )?;
    Ok(id)
}

fn update_imported_event(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    item: &crate::portability::ical::IcalEventPlan,
) -> VaultResult<()> {
    validate_event_input(item.all_day, &item.start_at, &item.end_at)?;
    tx.execute(
        "UPDATE events SET title=?1, description=?2, location=?3, start_at=?4, end_at=?5, all_day=?6, timezone=?7, recurrence_rule=?8, updated_at=datetime('now') WHERE id=?9",
        params![item.title, item.description, item.location, item.start_at, item.end_at, item.all_day as i64, item.timezone, item.recurrence_rule, id],
    )?;
    Ok(())
}

fn replace_ical_exdates(
    tx: &rusqlite::Transaction<'_>,
    event_id: &str,
    exdates: &[String],
) -> VaultResult<()> {
    tx.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1 AND is_cancelled = 1",
        params![event_id],
    )?;
    for value in exdates {
        tx.execute("INSERT INTO event_exceptions (id, event_id, original_start_at, new_start_at, is_cancelled) VALUES (?1, ?2, ?3, NULL, 1)", params![uuid::Uuid::new_v4().to_string(), event_id, value])?;
    }
    Ok(())
}

/// 编辑事件（修改全部实例：本体字段更新，例外行保留——例外键为绝对时刻，规则未变时仍正确）。
pub fn update_event(conn: &mut Connection, id: &str, patch: EventPatch) -> VaultResult<Event> {
    if let Some(title) = &patch.title {
        if title.trim().is_empty() {
            return Err(VaultError::Validation("事件标题不能为空".into()));
        }
    }
    if let Some(Some(rule)) = &patch.recurrence_rule {
        RRule::parse(rule)?;
    }
    if let Some(Some(rule)) = &patch.lunar_recurrence {
        LunarRecurrence::parse(rule)?;
    }
    // 校验起止（合并补丁后的最终值）
    {
        let cur = get_event(conn, id)?;
        let all_day = patch.all_day.unwrap_or(cur.all_day);
        let start_at = patch.start_at.as_deref().unwrap_or(&cur.start_at);
        let end_at = patch.end_at.as_deref().unwrap_or(&cur.end_at);
        validate_event_input(all_day, start_at, end_at)?;
    }
    let tx = conn.unchecked_transaction()?;
    let set = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> VaultResult<()> {
        tx.execute(sql, p)?;
        Ok(())
    };
    if let Some(title) = &patch.title {
        set(
            "UPDATE events SET title = ?1 WHERE id = ?2",
            &params![title, id],
        )?;
    }
    if let Some(description) = &patch.description {
        set(
            "UPDATE events SET description = ?1 WHERE id = ?2",
            &params![description, id],
        )?;
    }
    if let Some(location) = &patch.location {
        set(
            "UPDATE events SET location = ?1 WHERE id = ?2",
            &params![location, id],
        )?;
    }
    if let Some(start_at) = &patch.start_at {
        set(
            "UPDATE events SET start_at = ?1 WHERE id = ?2",
            &params![start_at, id],
        )?;
    }
    if let Some(end_at) = &patch.end_at {
        set(
            "UPDATE events SET end_at = ?1 WHERE id = ?2",
            &params![end_at, id],
        )?;
    }
    if let Some(all_day) = &patch.all_day {
        set(
            "UPDATE events SET all_day = ?1 WHERE id = ?2",
            &params![*all_day as i64, id],
        )?;
    }
    if let Some(timezone) = &patch.timezone {
        set(
            "UPDATE events SET timezone = ?1 WHERE id = ?2",
            &params![timezone, id],
        )?;
    }
    if let Some(rule) = &patch.recurrence_rule {
        set(
            "UPDATE events SET recurrence_rule = ?1 WHERE id = ?2",
            &params![rule, id],
        )?;
    }
    if let Some(rule) = &patch.lunar_recurrence {
        set(
            "UPDATE events SET lunar_recurrence = ?1 WHERE id = ?2",
            &params![rule, id],
        )?;
    }
    if let Some(color) = &patch.color {
        set(
            "UPDATE events SET color = ?1 WHERE id = ?2",
            &params![color, id],
        )?;
    }
    if let Some(link) = &patch.linked_page_ref {
        set(
            "UPDATE events SET linked_page_ref = ?1 WHERE id = ?2",
            &params![link, id],
        )?;
    }
    if let Some(link) = &patch.linked_task_id {
        set(
            "UPDATE events SET linked_task_id = ?1 WHERE id = ?2",
            &params![link, id],
        )?;
    }
    let n = tx.execute(
        "UPDATE events SET updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    tx.commit()?;
    if n == 0 {
        return Err(VaultError::NotFound(format!("事件 {id}")));
    }
    get_event(conn, id)
}

/// 删除事件（全部实例）：例外与提醒级联清理（提醒 FK 级联，例外手动删）。
pub fn delete_event(conn: &Connection, id: &str) -> VaultResult<()> {
    conn.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM events WHERE id = ?1", params![id])?;
    Ok(())
}

/// 范围内的实例：普通事件重叠命中；重复事件展开 + 例外应用。按实际开始升序。
pub fn instances_in_range(
    conn: &Connection,
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> VaultResult<Vec<EventInstance>> {
    let rs = range_start.and_hms_opt(0, 0, 0).expect("合法时刻");
    let re = range_end.and_hms_opt(23, 59, 59).expect("合法时刻");
    let rs_str = range_start.format("%Y-%m-%d").to_string();
    let re_str = range_end.format("%Y-%m-%d").to_string();

    let mut stmt = conn.prepare(&format!(
        "{SELECT_EVENT} WHERE (start_at <= ?2 AND end_at >= ?1) OR recurrence_rule IS NOT NULL OR lunar_recurrence IS NOT NULL"
    ))?;
    let events: Vec<Event> = {
        let rows = stmt
            .query_map(params![rs_str, re_str], row_event)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };

    // 例外表批量加载（event_id → (original_start, new_start, cancelled)）
    let mut exceptions: std::collections::HashMap<String, Vec<(String, Option<String>, bool)>> =
        std::collections::HashMap::new();
    if !events.is_empty() {
        let ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        let marks = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut estmt = conn.prepare(&format!(
            "SELECT event_id, original_start_at, new_start_at, is_cancelled FROM event_exceptions WHERE event_id IN ({marks})"
        ))?;
        let rows = estmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (eid, orig, new, cancelled) in rows {
            exceptions
                .entry(eid)
                .or_default()
                .push((orig, new, cancelled));
        }
    }

    let mut out = Vec::new();
    for event in events {
        let span = parse_span(event.all_day, &event.start_at, &event.end_at)?;
        let duration = span.end - span.start;
        let exs = exceptions.get(&event.id);
        if event.recurrence_rule.is_none() && event.lunar_recurrence.is_none() {
            if span.start <= re && span.end > rs {
                out.push(EventInstance {
                    original_start_at: event.start_at.clone(),
                    start_at: event.start_at.clone(),
                    end_at: event.end_at.clone(),
                    event,
                });
            }
            continue;
        }
        // 展开原始出现
        let mut occurrences: Vec<NaiveDateTime> = Vec::new();
        if let Some(rule_str) = &event.recurrence_rule {
            let rule = RRule::parse(rule_str)?;
            occurrences.extend(rule.expand(span.start, rs - duration, re));
        }
        if let Some(rule_str) = &event.lunar_recurrence {
            let rule = LunarRecurrence::parse(rule_str)?;
            // 农历重复按公历日展开（全天事件语义；全天农历事件天然为全天）
            let dates = rule.expand(
                (rs - chrono::Duration::days(duration.num_days().max(0))).date(),
                range_end,
            );
            for d in dates {
                occurrences.push(d.and_hms_opt(0, 0, 0).expect("合法时刻"));
            }
        }
        // An exception may move an occurrence from outside the requested range into it.
        // Include its original identity so the normal exception application below can
        // decide whether the effective span overlaps the visible window.
        if let Some(exs) = exs {
            for (original, new_start, cancelled) in exs {
                if !*cancelled && new_start.is_some() {
                    occurrences.push(parse_start(event.all_day, original)?);
                }
            }
        }
        occurrences.sort();
        occurrences.dedup();
        for occ in occurrences {
            let key = format_start(event.all_day, occ);
            let mut effective = occ;
            let mut cancelled = false;
            if let Some(exs) = exs {
                for (orig, new, is_cancelled) in exs {
                    if orig == &key {
                        if *is_cancelled {
                            cancelled = true;
                        } else if let Some(new_start) = new {
                            // 改期：按新起止解析（格式与事件一致）
                            effective = parse_start(event.all_day, new_start)?;
                        }
                    }
                }
            }
            if cancelled {
                continue;
            }
            let eff_span = Span {
                start: effective,
                end: effective + duration,
            };
            if eff_span.start > re || eff_span.end <= rs {
                continue;
            }
            out.push(EventInstance {
                original_start_at: key,
                start_at: format_start(event.all_day, eff_span.start),
                end_at: format_end(event.all_day, eff_span.end),
                event: event.clone(),
            });
        }
    }
    out.sort_by(|a, b| {
        a.start_at
            .cmp(&b.start_at)
            .then(a.event.id.cmp(&b.event.id))
    });
    Ok(out)
}

/// 单次例外：修改某次实例的起止（仅该次生效）。
pub fn move_instance(
    conn: &Connection,
    event_id: &str,
    original_start_at: &str,
    new_start_at: &str,
    new_end_at: &str,
) -> VaultResult<()> {
    let event = get_event(conn, event_id)?;
    validate_event_input(event.all_day, new_start_at, new_end_at)?;
    let original_span = parse_span(event.all_day, &event.start_at, &event.end_at)?;
    let moved_span = parse_span(event.all_day, new_start_at, new_end_at)?;
    if moved_span.end - moved_span.start != original_span.end - original_span.start {
        return Err(VaultError::Validation(
            "单次改期必须保持原事件时长；修改时长请修改全部实例".into(),
        ));
    }
    // M1 databases did not have a composite UNIQUE constraint, so replace explicitly.
    conn.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1 AND original_start_at = ?2",
        params![event_id, original_start_at],
    )?;
    conn.execute(
        "INSERT INTO event_exceptions (id, event_id, original_start_at, new_start_at, is_cancelled)
         VALUES (?1, ?2, ?3, ?4, 0)",
        params![
            uuid::Uuid::new_v4().to_string(),
            event_id,
            original_start_at,
            new_start_at
        ],
    )?;
    Ok(())
}

/// 单次例外：取消某次实例。
pub fn cancel_instance(
    conn: &Connection,
    event_id: &str,
    original_start_at: &str,
) -> VaultResult<()> {
    get_event(conn, event_id)?;
    conn.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1 AND original_start_at = ?2",
        params![event_id, original_start_at],
    )?;
    conn.execute(
        "INSERT INTO event_exceptions (id, event_id, original_start_at, new_start_at, is_cancelled)
         VALUES (?1, ?2, ?3, NULL, 1)",
        params![
            uuid::Uuid::new_v4().to_string(),
            event_id,
            original_start_at
        ],
    )?;
    Ok(())
}

pub fn list_reminders(conn: &Connection, event_id: &str) -> VaultResult<Vec<EventReminder>> {
    let mut stmt = conn.prepare(
        "SELECT id, event_id, minutes_before FROM event_reminders WHERE event_id = ?1 ORDER BY minutes_before",
    )?;
    let rows = stmt
        .query_map(params![event_id], |r| {
            Ok(EventReminder {
                id: r.get(0)?,
                event_id: r.get(1)?,
                minutes_before: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 覆盖式设置事件提醒（多提前量）。
pub fn set_reminders(conn: &Connection, event_id: &str, minutes: &[i64]) -> VaultResult<()> {
    get_event(conn, event_id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM event_reminders WHERE event_id = ?1",
        params![event_id],
    )?;
    for m in minutes {
        if *m < 0 {
            return Err(VaultError::Validation("提醒提前量不得为负".into()));
        }
        tx.execute(
            "INSERT INTO event_reminders (id, event_id, minutes_before) VALUES (?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), event_id, m],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 农历叠加数据（spec: 农历日期叠加显示 / 传统节日与节气显示）。
/// `festivals_enabled`/`solar_terms_enabled` 分别控制传统节日与节气输出。
pub fn lunar_overlay(
    range_start: NaiveDate,
    range_end: NaiveDate,
    festivals_enabled: bool,
    solar_terms_enabled: bool,
) -> Vec<DayInfo> {
    let mut out = Vec::new();
    let mut d = range_start;
    while d <= range_end {
        let lunar = super::lunar::solar_to_lunar(d);
        let (lunar_day, term, festival) = match lunar {
            Some(l) => {
                let day = if l.day == 1 {
                    let name = super::lunar::month_name(l.month);
                    if l.is_leap {
                        format!("闰{name}初一")
                    } else {
                        format!("{name}初一")
                    }
                } else {
                    super::lunar::day_name(l.day).to_string()
                };
                let term = if solar_terms_enabled {
                    super::festivals::solar_term(d)
                        .map(|i| super::festivals::TERM_NAMES[i as usize].to_string())
                } else {
                    None
                };
                let festival = if festivals_enabled {
                    super::festivals::traditional_festival(&l).map(str::to_string)
                } else {
                    None
                };
                (day, term, festival)
            }
            None => (String::new(), None, None),
        };
        out.push(DayInfo {
            date: d.format("%Y-%m-%d").to_string(),
            lunar_month: lunar.map(|value| value.month),
            lunar_date: lunar.map(|value| value.day),
            lunar_leap: lunar.is_some_and(|value| value.is_leap),
            lunar_day,
            term,
            festival,
        });
        d += chrono::Duration::days(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        crate::db::migrate::run_migrations(
            &mut conn,
            crate::db::migrate::DbKind::Calendar.migrations(),
        )
        .unwrap();
        conn
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn create_rejects_invalid_span() {
        let conn = conn();
        let err = create_event(
            &conn,
            "事件",
            None,
            None,
            "2026-03-02T11:00:00",
            "2026-03-02T10:00:00",
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(
            matches!(err, Err(VaultError::Validation(_))),
            "结束早于开始应被拒绝"
        );
        // 等于允许
        let e = create_event(
            &conn,
            "相等",
            None,
            None,
            "2026-03-02T10:00:00",
            "2026-03-02T10:00:00",
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(e.is_ok());
    }

    #[test]
    fn ical_uid_conflicts_update_or_copy_without_silent_duplicates() {
        let mut conn = conn();
        let first = crate::portability::ical::parse("BEGIN:VEVENT\nUID:external-1\nSUMMARY:Original\nDTSTART:20260302T090000\nDTEND:20260302T093000\nEND:VEVENT").unwrap();
        let created = import_ical_plan(&mut conn, &first, IcalConflictPolicy::Update).unwrap();
        assert_eq!(created.created, 1);
        let id: String = conn
            .query_row(
                "SELECT id FROM events WHERE external_uid = 'external-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let updated = crate::portability::ical::parse("BEGIN:VEVENT\nUID:external-1\nSUMMARY:Updated\nDTSTART:20260302T100000\nDTEND:20260302T103000\nEND:VEVENT").unwrap();
        assert_eq!(
            import_ical_plan(&mut conn, &updated, IcalConflictPolicy::Update)
                .unwrap()
                .updated,
            1
        );
        let updated_id: String = conn
            .query_row(
                "SELECT id FROM events WHERE external_uid = 'external-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(id, updated_id);
        assert_eq!(get_event(&conn, &id).unwrap().title, "Updated");

        assert_eq!(
            import_ical_plan(&mut conn, &updated, IcalConflictPolicy::Copy)
                .unwrap()
                .copied,
            1
        );
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        let distinct: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT external_uid) FROM events",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((count, distinct), (2, 2));
    }

    #[test]
    fn ical_export_round_trips_recurrence_cancellation_and_reschedule() {
        let source = conn();
        let event = create_event(
            &source,
            "Weekly, sync",
            Some("Line one\nLine two"),
            Some("Room; A"),
            "2026-03-02T09:00:00",
            "2026-03-02T09:30:00",
            false,
            Some("Asia/Hong_Kong"),
            Some("FREQ=WEEKLY;COUNT=4"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        source
            .execute(
                "UPDATE events SET external_uid = 'weekly-demo' WHERE id = ?1",
                params![event.id],
            )
            .unwrap();
        cancel_instance(&source, &event.id, "2026-03-09T09:00:00").unwrap();
        move_instance(
            &source,
            &event.id,
            "2026-03-16T09:00:00",
            "2026-03-16T10:00:00",
            "2026-03-16T10:30:00",
        )
        .unwrap();
        let output = export_ical(&source, None, None).unwrap();
        assert!(output.contains("TZID=Asia/Hong_Kong"));
        assert!(output.contains("SUMMARY:Weekly\\, sync"));
        assert!(output.contains("STATUS:CANCELLED"));
        assert!(output.lines().all(|line| line.as_bytes().len() <= 75));

        let plan = crate::portability::ical::parse(&output).unwrap();
        let mut restored = conn();
        import_ical_plan(&mut restored, &plan, IcalConflictPolicy::Update).unwrap();
        let instances = instances_in_range(&restored, d(2026, 3, 1), d(2026, 3, 31)).unwrap();
        let starts: Vec<_> = instances
            .iter()
            .map(|instance| instance.start_at.as_str())
            .collect();
        assert_eq!(
            starts,
            [
                "2026-03-02T09:00:00",
                "2026-03-16T10:00:00",
                "2026-03-23T09:00:00"
            ]
        );
    }

    #[test]
    fn weekly_recurrence_expands_with_exceptions() {
        let conn = conn();
        // 每周一 9:00，共 4 次（2026-03-02 起）
        let ev = create_event(
            &conn,
            "站会",
            None,
            None,
            "2026-03-02T09:00:00",
            "2026-03-02T09:30:00",
            false,
            None,
            Some("FREQ=WEEKLY;COUNT=4"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let inst = instances_in_range(&conn, d(2026, 3, 1), d(2026, 4, 30)).unwrap();
        let starts: Vec<&str> = inst.iter().map(|i| i.start_at.as_str()).collect();
        assert_eq!(
            starts,
            vec![
                "2026-03-02T09:00:00",
                "2026-03-09T09:00:00",
                "2026-03-16T09:00:00",
                "2026-03-23T09:00:00"
            ],
            "连续 4 个周一，第 5 周不出现"
        );

        // 单次改期：3-09 → 3-10 10:00
        move_instance(
            &conn,
            &ev.id,
            "2026-03-09T09:00:00",
            "2026-03-10T10:00:00",
            "2026-03-10T10:30:00",
        )
        .unwrap();
        let inst = instances_in_range(&conn, d(2026, 3, 1), d(2026, 4, 30)).unwrap();
        let moved: Vec<&EventInstance> = inst
            .iter()
            .filter(|i| i.original_start_at == "2026-03-09T09:00:00")
            .collect();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].start_at, "2026-03-10T10:00:00", "仅该次改期");
        assert_eq!(moved[0].end_at, "2026-03-10T10:30:00", "时长保持 30 分钟");
        let others: Vec<&str> = inst
            .iter()
            .filter(|i| i.original_start_at != "2026-03-09T09:00:00")
            .map(|i| i.start_at.as_str())
            .collect();
        assert_eq!(
            others,
            vec![
                "2026-03-02T09:00:00",
                "2026-03-16T09:00:00",
                "2026-03-23T09:00:00"
            ]
        );

        // 取消单次：3-16
        cancel_instance(&conn, &ev.id, "2026-03-16T09:00:00").unwrap();
        let inst = instances_in_range(&conn, d(2026, 3, 1), d(2026, 4, 30)).unwrap();
        assert_eq!(inst.len(), 3, "取消后少一个实例");

        // 例外持久化（重建连接后仍生效）
        let conn2 = self::conn();
        // 迁移同一数据：重新插入事件与例外
        let ev2 = create_event(
            &conn2,
            "站会",
            None,
            None,
            "2026-03-02T09:00:00",
            "2026-03-02T09:30:00",
            false,
            None,
            Some("FREQ=WEEKLY;COUNT=4"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        move_instance(
            &conn2,
            &ev2.id,
            "2026-03-09T09:00:00",
            "2026-03-10T10:00:00",
            "2026-03-10T10:30:00",
        )
        .unwrap();
        cancel_instance(&conn2, &ev2.id, "2026-03-16T09:00:00").unwrap();
        let inst = instances_in_range(&conn2, d(2026, 3, 1), d(2026, 4, 30)).unwrap();
        assert_eq!(inst.len(), 3, "重建连接（模拟重启）后例外保持");
    }

    #[test]
    fn lunar_recurrence_merges_into_pipeline() {
        let conn = conn();
        // 农历八月初十生日：2024-09-12 / 2025-10-01
        let rule = crate::calendar::lunar_rule::LunarRecurrence {
            month: 8,
            day: 10,
            leap_month: crate::calendar::lunar_rule::LeapMonthPolicy::Ignore,
        };
        create_event(
            &conn,
            "生日",
            None,
            None,
            "2024-09-12",
            "2024-09-13",
            true,
            None,
            None,
            Some(&rule.to_json().unwrap()),
            Some("#ff4081"),
            None,
            None,
        )
        .unwrap();
        let inst = instances_in_range(&conn, d(2024, 1, 1), d(2025, 12, 31)).unwrap();
        let starts: Vec<&str> = inst.iter().map(|i| i.start_at.as_str()).collect();
        assert_eq!(
            starts,
            vec!["2024-09-12", "2025-10-01"],
            "农历重复并入展开管线"
        );
        assert!(inst[0].event.all_day);
    }

    #[test]
    fn lunar_ical_export_requires_a_range_and_materializes_the_domain_instances() {
        let conn = conn();
        let rule = crate::calendar::lunar_rule::LunarRecurrence {
            month: 4,
            day: 1,
            leap_month: crate::calendar::lunar_rule::LeapMonthPolicy::Only,
        };
        let event = create_event(
            &conn,
            "闰月纪念日",
            None,
            None,
            "2020-05-23",
            "2020-05-24",
            true,
            None,
            None,
            Some(&rule.to_json().unwrap()),
            None,
            None,
            None,
        )
        .unwrap();

        assert!(matches!(
            export_ical(&conn, Some(&[event.id.clone()]), None),
            Err(VaultError::Validation(_))
        ));
        let range = (d(2019, 1, 1), d(2021, 12, 31));
        let expected: Vec<String> = instances_in_range(&conn, range.0, range.1)
            .unwrap()
            .into_iter()
            .map(|instance| instance.start_at)
            .collect();
        let output = export_ical(&conn, Some(&[event.id]), Some(range)).unwrap();
        assert!(output.contains("RELATED-TO:"));
        assert!(output.contains("X-TENJEE-LUNAR-SERIES:"));
        assert!(!output.contains("RRULE:"));
        let imported = crate::portability::ical::parse(&output).unwrap();
        assert_eq!(
            imported
                .events
                .into_iter()
                .map(|item| item.start_at)
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn multi_reminders_crud() {
        let conn = conn();
        let ev = create_event(
            &conn,
            "例会",
            None,
            None,
            "2026-03-02T15:00:00",
            "2026-03-02T16:00:00",
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        set_reminders(&conn, &ev.id, &[10, 1440]).unwrap();
        let reminders = list_reminders(&conn, &ev.id).unwrap();
        assert_eq!(reminders.len(), 2);
        assert_eq!(reminders[0].minutes_before, 10);
        assert_eq!(reminders[1].minutes_before, 1440);
        // 覆盖式更新
        set_reminders(&conn, &ev.id, &[5]).unwrap();
        let reminders = list_reminders(&conn, &ev.id).unwrap();
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].minutes_before, 5);
        assert!(set_reminders(&conn, &ev.id, &[-1]).is_err());
    }

    #[test]
    fn delete_event_cascades_exceptions() {
        let conn = conn();
        let ev = create_event(
            &conn,
            "重复",
            None,
            None,
            "2026-03-02T09:00:00",
            "2026-03-02T10:00:00",
            false,
            None,
            Some("FREQ=WEEKLY"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        cancel_instance(&conn, &ev.id, "2026-03-09T09:00:00").unwrap();
        set_reminders(&conn, &ev.id, &[10]).unwrap();
        delete_event(&conn, &ev.id).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM event_exceptions WHERE event_id = ?1",
                params![ev.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "例外级联清理");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM event_reminders WHERE event_id = ?1",
                params![ev.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "提醒级联清理");
        assert!(get_event(&conn, &ev.id).is_err());
    }

    #[test]
    fn single_and_all_instance_operations_affect_range_results() {
        let mut conn = conn();
        let event = create_event(
            &conn,
            "重复",
            None,
            None,
            "2026-03-02T09:00:00",
            "2026-03-02T10:00:00",
            false,
            None,
            Some("FREQ=WEEKLY;COUNT=3"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        move_instance(
            &conn,
            &event.id,
            "2026-03-09T09:00:00",
            "2026-03-10T09:00:00",
            "2026-03-10T10:00:00",
        )
        .unwrap();
        cancel_instance(&conn, &event.id, "2026-03-16T09:00:00").unwrap();
        let instances = instances_in_range(&conn, d(2026, 3, 1), d(2026, 3, 31)).unwrap();
        assert_eq!(instances.len(), 2);
        assert!(instances
            .iter()
            .any(|item| item.start_at == "2026-03-10T09:00:00"));

        update_event(
            &mut conn,
            &event.id,
            EventPatch {
                title: Some("全部修改".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let instances = instances_in_range(&conn, d(2026, 3, 1), d(2026, 3, 31)).unwrap();
        assert!(instances.iter().all(|item| item.event.title == "全部修改"));

        delete_event(&conn, &event.id).unwrap();
        assert!(instances_in_range(&conn, d(2026, 3, 1), d(2026, 3, 31))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn links_read_write() {
        let mut conn = conn();
        let ev = create_event(
            &conn,
            "评审",
            None,
            None,
            "2026-03-02T10:00:00",
            "2026-03-02T11:00:00",
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        update_event(
            &mut conn,
            &ev.id,
            EventPatch {
                linked_page_ref: Some(Some("sp1:pg9".into())),
                linked_task_id: Some(Some("task1".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let ev = get_event(&conn, &ev.id).unwrap();
        assert_eq!(ev.linked_page_ref.as_deref(), Some("sp1:pg9"));
        assert_eq!(ev.linked_task_id.as_deref(), Some("task1"));
    }

    #[test]
    fn links_resolve_and_missing_targets_degrade() {
        let calendar = conn();
        let mut tasks = Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&tasks).unwrap();
        crate::db::migrate::run_migrations(
            &mut tasks,
            crate::db::migrate::DbKind::Tasks.migrations(),
        )
        .unwrap();
        tasks
            .execute(
                "INSERT INTO task_lists (id, name) VALUES ('list', '收件箱')",
                [],
            )
            .unwrap();
        tasks
            .execute(
                "INSERT INTO tasks (id, list_id, title) VALUES ('task1', 'list', '关联任务')",
                [],
            )
            .unwrap();
        let event = create_event(
            &calendar,
            "评审",
            None,
            None,
            "2026-03-02T10:00:00",
            "2026-03-02T11:00:00",
            false,
            None,
            None,
            None,
            None,
            Some("space:page"),
            Some("task1"),
        )
        .unwrap();

        let resolved =
            resolve_links(&event, &tasks, |reference| Ok(reference == "space:page")).unwrap();
        assert_eq!(resolved.page.as_ref().map(|v| v.exists), Some(true));
        assert_eq!(resolved.task.as_ref().map(|v| v.exists), Some(true));

        tasks
            .execute("DELETE FROM tasks WHERE id = 'task1'", [])
            .unwrap();
        let degraded = resolve_links(&event, &tasks, |_| Ok(false)).unwrap();
        assert_eq!(degraded.page.as_ref().map(|v| v.exists), Some(false));
        assert_eq!(degraded.task.as_ref().map(|v| v.exists), Some(false));
    }

    #[test]
    fn exceptions_survive_connection_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calendar.db");
        let event_id = {
            let mut conn = crate::db::connection::open_db(&path).unwrap();
            crate::db::migrate::run_migrations(
                &mut conn,
                crate::db::migrate::DbKind::Calendar.migrations(),
            )
            .unwrap();
            let event = create_event(
                &conn,
                "站会",
                None,
                None,
                "2026-03-02T09:00:00",
                "2026-03-02T09:30:00",
                false,
                None,
                Some("FREQ=WEEKLY;COUNT=3"),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            move_instance(
                &conn,
                &event.id,
                "2026-03-09T09:00:00",
                "2026-03-10T10:00:00",
                "2026-03-10T10:30:00",
            )
            .unwrap();
            cancel_instance(&conn, &event.id, "2026-03-16T09:00:00").unwrap();
            event.id
        };

        let conn = crate::db::connection::open_db(&path).unwrap();
        let instances = instances_in_range(&conn, d(2026, 3, 1), d(2026, 3, 31)).unwrap();
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().any(|instance| {
            instance.event.id == event_id && instance.start_at == "2026-03-10T10:00:00"
        }));
    }

    #[test]
    fn lunar_overlay_combines_day_term_festival() {
        let days = lunar_overlay(d(2026, 2, 16), d(2026, 2, 18), true, true);
        assert_eq!(days.len(), 3);
        // 2026-02-17 = 正月初一/春节；02-16 除夕
        assert_eq!(days[0].festival.as_deref(), Some("除夕"));
        assert_eq!(days[1].lunar_day, "正月初一");
        assert_eq!(days[1].festival.as_deref(), Some("春节"));
        // 2026-02-18 雨水（节气）→ 验证节气字段
        let days = lunar_overlay(d(2026, 2, 18), d(2026, 2, 18), true, true);
        assert_eq!(days[0].term.as_deref(), Some("雨水"), "2026-02-18 应为雨水");
        // 开关独立生效
        let off = lunar_overlay(d(2026, 2, 17), d(2026, 2, 17), false, true);
        assert_eq!(off[0].festival, None, "传统节日开关关闭");
        assert_eq!(off[0].lunar_day, "正月初一", "农历日仍显示");
        let term_off = lunar_overlay(d(2026, 2, 18), d(2026, 2, 18), true, false);
        assert_eq!(term_off[0].term, None, "节气开关关闭");
    }

    #[test]
    fn agenda_expands_recurring_entries() {
        let conn = conn();
        create_event(
            &conn,
            "每日站会",
            None,
            None,
            "2026-03-02T09:00:00",
            "2026-03-02T09:15:00",
            false,
            None,
            Some("FREQ=DAILY;COUNT=3"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let inst = instances_in_range(&conn, d(2026, 3, 1), d(2026, 3, 31)).unwrap();
        assert_eq!(inst.len(), 3, "重复事件展开为独立条目");
        assert!(
            inst.windows(2).all(|w| w[0].start_at < w[1].start_at),
            "按时间排序"
        );
    }
}

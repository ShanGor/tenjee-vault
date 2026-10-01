-- M3: 提醒已发记录（防重启重复发送，见 design D6）。
-- entity_kind: 'task' | 'event'；occurrence_key: 重复事件实例身份（original_start_at），
-- 非重复事件与任务为 ''；slot: 事件为 minutes_before，任务恒为 0。
CREATE TABLE reminder_fires (
    entity_kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    occurrence_key TEXT NOT NULL DEFAULT '',
    slot INTEGER NOT NULL DEFAULT 0,
    fired_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (entity_kind, entity_id, occurrence_key, slot)
);

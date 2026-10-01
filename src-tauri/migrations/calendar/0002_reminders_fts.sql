-- M3: 事件多提醒（event_reminders）与事件全文搜索（FTS5）。
-- spec §4.1 要求可设多个提前量；既有 events.reminder_minutes（单个）迁入新表后废弃。
-- events_fts 为独立（非 external content）FTS5 虚拟表，rowid 与 events.rowid 对齐。
-- 中文单字分词：经 tv_seg 标量函数（应用注册，见 search 模块）切分后入索引。

CREATE TABLE event_reminders (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    minutes_before INTEGER NOT NULL CHECK (minutes_before >= 0)
);

CREATE INDEX idx_event_reminders_event ON event_reminders(event_id);

-- 既有单提醒数据迁入多提醒表（id 加迁移前缀保证唯一）
INSERT INTO event_reminders (id, event_id, minutes_before)
SELECT 'mig-' || id, id, reminder_minutes
FROM events
WHERE reminder_minutes IS NOT NULL;

ALTER TABLE events DROP COLUMN reminder_minutes;

CREATE VIRTUAL TABLE events_fts USING fts5(title, description, tokenize = 'unicode61');

CREATE TRIGGER events_fts_insert AFTER INSERT ON events BEGIN
    INSERT INTO events_fts (rowid, title, description)
    VALUES (new.rowid, tv_seg(new.title), tv_seg(COALESCE(new.description, '')));
END;

CREATE TRIGGER events_fts_delete AFTER DELETE ON events BEGIN
    DELETE FROM events_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER events_fts_update AFTER UPDATE ON events BEGIN
    DELETE FROM events_fts WHERE rowid = old.rowid;
    INSERT INTO events_fts (rowid, title, description)
    VALUES (new.rowid, tv_seg(new.title), tv_seg(COALESCE(new.description, '')));
END;

-- 存量事件回填索引
INSERT INTO events_fts (rowid, title, description)
SELECT rowid, tv_seg(title), tv_seg(COALESCE(description, '')) FROM events;

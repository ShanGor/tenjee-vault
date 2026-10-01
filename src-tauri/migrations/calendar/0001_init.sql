CREATE TABLE events (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    all_day INTEGER NOT NULL DEFAULT 0,
    timezone TEXT,
    recurrence_rule TEXT,
    lunar_recurrence TEXT,
    reminder_minutes INTEGER,
    color TEXT,
    linked_page_ref TEXT,
    linked_task_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE event_exceptions (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES events(id),
    original_start_at TEXT NOT NULL,
    new_start_at TEXT,
    is_cancelled INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_events_start ON events(start_at);
CREATE INDEX idx_exceptions_event ON event_exceptions(event_id);

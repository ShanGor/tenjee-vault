CREATE TABLE task_lists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    list_id TEXT NOT NULL REFERENCES task_lists(id),
    title TEXT NOT NULL,
    notes TEXT,
    status TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'in_progress', 'done', 'cancelled')),
    priority TEXT CHECK (priority IN ('none', 'low', 'medium', 'high')),
    due_date TEXT,
    due_time TEXT,
    reminder_at TEXT,
    recurrence_rule TEXT,
    parent_task_id TEXT REFERENCES tasks(id),
    sort_order INTEGER NOT NULL DEFAULT 0,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_tasks_list ON tasks(list_id);
CREATE INDEX idx_tasks_status ON tasks(status);

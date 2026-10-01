-- M3: 任务归档字段、附件/标签表、任务全文搜索（FTS5）。
-- tasks_fts 为独立（非 external content）FTS5 虚拟表，rowid 与 tasks.rowid 对齐。
-- 归档任务仍保留在索引中，由查询层按 archived_at 过滤（见 design D7）。
-- 中文单字分词：经 tv_seg 标量函数（应用注册，见 search 模块）切分后入索引。

ALTER TABLE tasks ADD COLUMN archived_at TEXT;

CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    mime TEXT,
    size INTEGER,
    hash TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 跨库引用：tag_id 指向 meta.db 的 tags 表，不做外键约束（应用层容错）
CREATE TABLE taggings (
    tag_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL
);

CREATE INDEX idx_attachments_entity ON attachments(entity_type, entity_id);
CREATE INDEX idx_attachments_hash ON attachments(hash);
CREATE INDEX idx_taggings_entity ON taggings(entity_type, entity_id);

CREATE VIRTUAL TABLE tasks_fts USING fts5(title, notes, tokenize = 'unicode61');

CREATE TRIGGER tasks_fts_insert AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts (rowid, title, notes)
    VALUES (new.rowid, tv_seg(new.title), tv_seg(COALESCE(new.notes, '')));
END;

CREATE TRIGGER tasks_fts_delete AFTER DELETE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER tasks_fts_update AFTER UPDATE ON tasks BEGIN
    DELETE FROM tasks_fts WHERE rowid = old.rowid;
    INSERT INTO tasks_fts (rowid, title, notes)
    VALUES (new.rowid, tv_seg(new.title), tv_seg(COALESCE(new.notes, '')));
END;

-- 存量任务回填索引
INSERT INTO tasks_fts (rowid, title, notes)
SELECT rowid, tv_seg(title), tv_seg(COALESCE(notes, '')) FROM tasks;

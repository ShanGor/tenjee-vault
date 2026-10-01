-- M2: 最近使用页面（spec: 最近使用页面，按最后打开时间排序，跨空间）。
CREATE TABLE recent_pages (
    space_id TEXT NOT NULL,
    page_id TEXT NOT NULL,
    opened_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (space_id, page_id)
);

CREATE INDEX idx_recent_pages_opened ON recent_pages(opened_at DESC);

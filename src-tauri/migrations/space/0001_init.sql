CREATE TABLE notebooks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE section_groups (
    id TEXT PRIMARY KEY,
    notebook_id TEXT NOT NULL REFERENCES notebooks(id),
    parent_group_id TEXT REFERENCES section_groups(id),
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

-- 加密分区：仅存 KDF salt/参数、验证器与 wrapped DSK；密钥永不落盘
CREATE TABLE sections (
    id TEXT PRIMARY KEY,
    notebook_id TEXT NOT NULL REFERENCES notebooks(id),
    section_group_id TEXT REFERENCES section_groups(id),
    name TEXT NOT NULL,
    color TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_encrypted INTEGER NOT NULL DEFAULT 0,
    kdf_salt BLOB,
    kdf_params TEXT,
    verifier BLOB,
    wrapped_dsk BLOB
);

CREATE TABLE pages (
    id TEXT PRIMARY KEY,
    section_id TEXT NOT NULL REFERENCES sections(id),
    parent_page_id TEXT REFERENCES pages(id),
    title TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE page_versions (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

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

CREATE INDEX idx_pages_section ON pages(section_id);
CREATE INDEX idx_versions_page ON page_versions(page_id);
CREATE INDEX idx_taggings_entity ON taggings(entity_type, entity_id);

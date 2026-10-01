-- M4: 分区模板始终由该分区的 DSK 加密。名称同样是密文，避免仅从磁盘目录泄露内容。
CREATE TABLE section_templates (
    id TEXT PRIMARY KEY,
    section_id TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
    name_ciphertext BLOB NOT NULL,
    content_ciphertext BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_section_templates_section ON section_templates(section_id, created_at);

-- SQLite CHECK 不能引用 sections；用触发器阻止把分区模板写入普通分区。
CREATE TRIGGER section_templates_require_encrypted_section_insert
BEFORE INSERT ON section_templates
WHEN COALESCE((SELECT is_encrypted FROM sections WHERE id = NEW.section_id), 0) != 1
BEGIN
    SELECT RAISE(ABORT, 'section templates require an encrypted section');
END;

CREATE TRIGGER section_templates_require_encrypted_section_update
BEFORE UPDATE OF section_id ON section_templates
WHEN COALESCE((SELECT is_encrypted FROM sections WHERE id = NEW.section_id), 0) != 1
BEGIN
    SELECT RAISE(ABORT, 'section templates require an encrypted section');
END;

-- M2: 笔记全文搜索（FTS5）。
-- pages_fts 为独立（非 external content）FTS5 虚拟表，rowid 与 pages.rowid 对齐。
-- 触发器按 sections.is_encrypted 分派：加密分区页面完全不进入持久索引；
-- 回收站中的页面（is_deleted=1）也不进入索引，恢复时由触发器自动回填。
-- 中文单字分词：经 tv_seg 标量函数（应用注册，见 search 模块）切分后入索引。

CREATE VIRTUAL TABLE pages_fts USING fts5(title, content, tokenize = 'unicode61');

CREATE TRIGGER pages_fts_insert AFTER INSERT ON pages BEGIN
    INSERT INTO pages_fts (rowid, title, content)
    SELECT new.rowid, tv_seg(new.title), tv_seg(new.content)
    WHERE new.is_deleted = 0
      AND NOT EXISTS (
          SELECT 1 FROM sections
          WHERE sections.id = new.section_id AND sections.is_encrypted = 1
      );
END;

CREATE TRIGGER pages_fts_delete AFTER DELETE ON pages BEGIN
    DELETE FROM pages_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER pages_fts_update AFTER UPDATE ON pages BEGIN
    DELETE FROM pages_fts WHERE rowid = old.rowid;
    INSERT INTO pages_fts (rowid, title, content)
    SELECT new.rowid, tv_seg(new.title), tv_seg(new.content)
    WHERE new.is_deleted = 0
      AND NOT EXISTS (
          SELECT 1 FROM sections
          WHERE sections.id = new.section_id AND sections.is_encrypted = 1
      );
END;

-- 存量明文页面回填（加密分区页面与回收站页面跳过）
INSERT INTO pages_fts (rowid, title, content)
SELECT p.rowid, tv_seg(p.title), tv_seg(p.content)
FROM pages p
JOIN sections s ON s.id = p.section_id
WHERE s.is_encrypted = 0 AND p.is_deleted = 0;

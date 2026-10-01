-- Sections remain private crypto domains. Their root is an ordinary editable page.
ALTER TABLE sections ADD COLUMN root_page_id TEXT;
ALTER TABLE pages ADD COLUMN deleted_batch TEXT;
CREATE INDEX idx_pages_parent ON pages(parent_page_id);
CREATE INDEX idx_sections_root_page ON sections(root_page_id);

INSERT INTO notebooks (id, name) VALUES ('__page_storage__', 'Pages');
INSERT INTO sections (id, notebook_id, name) VALUES ('__plain_pages__', '__page_storage__', 'Pages');

INSERT INTO pages (id, section_id, title, sort_order, created_at, updated_at)
SELECT 'notebook:' || id, '__plain_pages__', name, sort_order, created_at, updated_at
FROM notebooks WHERE id != '__page_storage__';

-- Insert all groups before assigning parents, so nested foreign keys are valid.
INSERT INTO pages (id, section_id, title, sort_order)
SELECT 'group:' || id, '__plain_pages__', name, sort_order FROM section_groups;
UPDATE pages SET parent_page_id = (
    SELECT CASE WHEN g.parent_group_id IS NULL THEN 'notebook:' || g.notebook_id
                ELSE 'group:' || g.parent_group_id END
    FROM section_groups g WHERE pages.id = 'group:' || g.id
) WHERE id IN (SELECT 'group:' || id FROM section_groups);

INSERT INTO pages (id, section_id, parent_page_id, title, sort_order)
SELECT 'section:' || id, id,
       CASE WHEN section_group_id IS NULL THEN 'notebook:' || notebook_id
            ELSE 'group:' || section_group_id END,
       name, sort_order
FROM sections WHERE id != '__plain_pages__';

UPDATE sections SET root_page_id = 'section:' || id WHERE id != '__plain_pages__';
UPDATE pages SET parent_page_id = 'section:' || section_id
WHERE parent_page_id IS NULL AND section_id != '__plain_pages__'
  AND id NOT LIKE 'section:%';

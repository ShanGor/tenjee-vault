-- M4: 外部 iCalendar UID 仅在导入后存在；NULL 保持手动创建事件与既有数据的语义。
ALTER TABLE events ADD COLUMN external_uid TEXT;

CREATE UNIQUE INDEX idx_events_external_uid
ON events(external_uid)
WHERE external_uid IS NOT NULL;

-- tag_id 指向 meta.db 的全局字典，不设跨库外键。一个事件的同一标签只能关联一次。
CREATE TABLE taggings (
    tag_id TEXT NOT NULL,
    entity_type TEXT NOT NULL DEFAULT 'event' CHECK (entity_type = 'event'),
    entity_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    PRIMARY KEY (tag_id, entity_type, entity_id)
);

CREATE INDEX idx_calendar_taggings_entity ON taggings(entity_type, entity_id);
CREATE INDEX idx_calendar_taggings_tag ON taggings(tag_id, entity_id);

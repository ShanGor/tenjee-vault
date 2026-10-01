-- M4: 任务可追溯到笔记待办节点。NULL 表示 M3 及更早创建的普通任务，
-- 因而既有任务不被强行关联或改写。
ALTER TABLE tasks ADD COLUMN source_page_ref TEXT;
ALTER TABLE tasks ADD COLUMN source_node_id TEXT;

CREATE UNIQUE INDEX idx_tasks_note_source
ON tasks(source_page_ref, source_node_id)
WHERE source_page_ref IS NOT NULL AND source_node_id IS NOT NULL;

-- 锁定的加密分区只在 tasks.db 留下不透明引用和目标勾选状态；不得保存标题、正文或密钥。
-- 同一任务来源在队列中最多一项，新的状态变化更新该项，以支持幂等消费。
CREATE TABLE note_sync_queue (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    source_page_ref TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    checked INTEGER NOT NULL CHECK (checked IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (task_id, source_page_ref, source_node_id)
);

CREATE INDEX idx_note_sync_queue_pending
ON note_sync_queue(status, created_at);

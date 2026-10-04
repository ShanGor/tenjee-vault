-- Local derived checkbox state. Only logical tasks/pages travel over LAN.
CREATE TABLE note_task_projections (
    task_id TEXT NOT NULL,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    checked INTEGER NOT NULL CHECK(checked IN (0,1)),
    source_context TEXT NOT NULL,
    PRIMARY KEY(task_id,page_id,node_id)
);

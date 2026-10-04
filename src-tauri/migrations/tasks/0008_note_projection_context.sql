ALTER TABLE note_sync_queue ADD COLUMN source_context TEXT;
DROP TRIGGER tasks_note_status_sync;
CREATE TRIGGER tasks_note_status_sync AFTER UPDATE OF status ON tasks
WHEN NEW.status != OLD.status AND NEW.source_page_ref IS NOT NULL AND NEW.source_node_id IS NOT NULL
BEGIN
    INSERT INTO note_sync_queue(id,task_id,source_page_ref,source_node_id,checked,source_context)
    VALUES (lower(hex(randomblob(16))),NEW.id,NEW.source_page_ref,NEW.source_node_id,NEW.status='done',
      (SELECT context FROM sync_objects WHERE entity='tasks' AND entity_key=json_array(NEW.id)))
    ON CONFLICT(task_id,source_page_ref,source_node_id) DO UPDATE SET
      checked=excluded.checked,source_context=excluded.source_context,status='pending',last_error=NULL,updated_at=datetime('now');
END;

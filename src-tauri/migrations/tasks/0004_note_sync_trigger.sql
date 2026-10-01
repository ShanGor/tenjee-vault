-- Keep status changes and pending note synchronization in the same transaction,
-- including batch changes and recurring-task completion.
CREATE TRIGGER tasks_note_status_sync AFTER UPDATE OF status ON tasks
WHEN NEW.status != OLD.status AND NEW.source_page_ref IS NOT NULL AND NEW.source_node_id IS NOT NULL
BEGIN
    INSERT INTO note_sync_queue(id,task_id,source_page_ref,source_node_id,checked)
    VALUES (lower(hex(randomblob(16))),NEW.id,NEW.source_page_ref,NEW.source_node_id,NEW.status='done')
    ON CONFLICT(task_id,source_page_ref,source_node_id) DO UPDATE SET
      checked=excluded.checked,status='pending',last_error=NULL,updated_at=datetime('now');
END;

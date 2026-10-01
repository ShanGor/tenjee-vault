-- Older task rows may have NULL priority because the original column was nullable.
UPDATE tasks SET priority = 'none' WHERE priority IS NULL;

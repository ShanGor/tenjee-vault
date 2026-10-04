ALTER TABLE pages ADD COLUMN title_is_encrypted INTEGER NOT NULL DEFAULT 0 CHECK(title_is_encrypted IN (0,1));
UPDATE sync_objects SET payload=json_set(payload,'$.title_is_encrypted',0) WHERE entity='pages' AND deleted=0;
UPDATE sync_conflicts SET payload=json_set(payload,'$.title_is_encrypted',0) WHERE entity='pages' AND deleted=0;

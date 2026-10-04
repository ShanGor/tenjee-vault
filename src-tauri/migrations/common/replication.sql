-- Payloads are current heads, not a plaintext historical copy of protected data.
CREATE TABLE sync_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    origin TEXT NOT NULL,
    sequence INTEGER NOT NULL DEFAULT 0,
    importing INTEGER NOT NULL DEFAULT 0 CHECK(importing IN (0, 1))
);
INSERT INTO sync_state(singleton, origin) VALUES(1, lower(hex(randomblob(16))));
CREATE TABLE sync_objects (
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    context TEXT NOT NULL CHECK(json_valid(context)),
    payload TEXT CHECK(payload IS NULL OR json_valid(payload)),
    deleted INTEGER NOT NULL CHECK(deleted IN (0, 1)),
    PRIMARY KEY(entity, entity_key)
);
CREATE TABLE sync_changes (
    origin TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    PRIMARY KEY(origin, sequence)
);
CREATE TABLE sync_conflicts (
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    variant_id TEXT NOT NULL,
    context TEXT NOT NULL CHECK(json_valid(context)),
    payload TEXT CHECK(payload IS NULL OR json_valid(payload)),
    deleted INTEGER NOT NULL CHECK(deleted IN (0, 1)),
    PRIMARY KEY(entity, entity_key, variant_id)
);
CREATE TABLE sync_checkpoints (
    peer TEXT NOT NULL,
    origin TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    PRIMARY KEY(peer, origin)
);

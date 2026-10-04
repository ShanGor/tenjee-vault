CREATE TABLE sync_groups (
    group_id TEXT PRIMARY KEY,
    context TEXT NOT NULL CHECK(json_valid(context))
);
CREATE TABLE sync_members (
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    group_id TEXT NOT NULL,
    PRIMARY KEY(entity,entity_key)
);
CREATE INDEX sync_members_group ON sync_members(group_id);
CREATE TABLE sync_domain_variants (
    group_id TEXT NOT NULL,
    variant_id TEXT NOT NULL,
    context TEXT NOT NULL CHECK(json_valid(context)),
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    PRIMARY KEY(group_id,variant_id)
);
CREATE TABLE sync_pending_groups (
    group_id TEXT NOT NULL,
    peer TEXT NOT NULL,
    context TEXT NOT NULL CHECK(json_valid(context)),
    reason TEXT NOT NULL,
    PRIMARY KEY(group_id,peer)
);

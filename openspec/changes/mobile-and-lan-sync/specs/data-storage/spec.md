# Spec Delta

## ADDED Requirements

### Requirement: Durable replication revisions

Every synchronized mutation SHALL atomically persist its record change and durable causal revision within the owning database transaction. The system SHALL retain stable entity identifiers, origin replica identity, duplicate-detection information, conflict variants, and deletion tombstones. Imports, bulk operations, task recurrence/course creation, page moves/protection, attachment changes, and conflict resolutions SHALL participate. Receiving an existing revision SHALL NOT create another logical change. Wall-clock timestamps SHALL NOT determine causal order.

#### Scenario: Crash during a local edit
- **WHEN** the app terminates during a record and revision write
- **THEN** either both persist or neither persists and later replication has no untracked committed edit

#### Scenario: Receive the same change twice
- **WHEN** a previously applied revision is retransmitted
- **THEN** it has no duplicate effect and retains the original origin/revision identity

### Requirement: Safe deletion and checkpoint retention

The system SHALL persist replication progress and deletion knowledge separately from live objects. Deletion tombstones and causal knowledge SHALL NOT be discarded solely because of age or one peer's acknowledgment; a returning offline replica SHALL NOT resurrect causally deleted objects. Acknowledgment SHALL follow durable application, and interrupted cross-database operations SHALL be recoverable through staged manifests and idempotent continuation. Referenced attachment blobs SHALL NOT be collected while required by current data or retained conflicts.

#### Scenario: Return after a long offline period
- **WHEN** an old replica reconnects after another device deleted an unchanged object
- **THEN** the deletion propagates without resurrecting the old live copy

#### Scenario: Crash between domain commits
- **WHEN** exchange terminates after one database commits but before related databases finish
- **THEN** recovery continues the durable batch without duplicate effects and completion remains pending until required records/blobs are valid

### Requirement: Restore-safe replica identity

Each installation SHALL maintain a distinct replica identity and monotonically advancing local revision namespace. Backup restoration or vault cloning SHALL allocate a fresh local replica identity while preserving imported entity identifiers and causal history. Old peer checkpoints SHALL NOT be trusted to skip reconciliation after restore. Existing revisionless records SHALL receive a migration baseline without changing public entity identifiers or encryption materials.

#### Scenario: Restore the same backup on two devices
- **WHEN** two installations restore the same backup and independently edit a page
- **THEN** their new revisions have different origins, exchange detects concurrency, and no edit is skipped because their counters coincide

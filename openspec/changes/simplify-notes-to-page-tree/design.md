# Design

## Context

Pages already have parent IDs, but navigation requires notebook and section routes. Existing encryption and templates use section IDs as key handles, including attachments and search indexes.

## Goals / Non-Goals

Goals: one page tree per space, stable existing IDs and ciphertext, inherited subtree protection, compatible content tools.
Non-goals: collaboration, new external services, or introducing another visible namespace layer.

## Decisions

- Keep the existing space database boundary. Use a space/page URL as the canonical route.
- Migrate notebook/group/section names into empty editable parent pages, then attach existing root pages to the corresponding migrated section page. Add a root-page reference to internal encryption domains.
- Retain section records as internal crypto domains to preserve existing keys, attachments and encrypted templates. New ordinary pages use an internal plaintext domain. No user-facing notebook/group/section ownership remains.
- Isolate an ordinary subtree into a new crypto domain when protecting it. Encrypt content, versions and attachment blobs transactionally, retaining resource IDs. Existing independent protected branches retain their domains; conflicting nested protections are rejected explicitly.
- Moves reassign ordinary descendants to the destination domain with appropriate encryption/decryption. A protected root moved under an ordinary page retains its domain; inherited children cannot leave a protected subtree without removing protection. Recursive queries guard against cycles.
- Search/trash show ancestor paths. Templates retain domain scoping with page terminology. Imports become children of the selected page; export targets the selected subtree.

## Risks / Trade-offs

- Ciphertext compatibility requires retaining internal legacy records; UI and page APIs do not expose them as organizational concepts.
- Migrated encrypted wrapper pages have empty content until first edited. Treat empty stored content as an empty document only after verifying the domain is unlocked.
- Filesystem writes cannot be committed with SQLite. Stage content-addressed blobs before committing new hashes, then clean obsolete blobs after commit. On failure keep original references.

## Migration Plan

A versioned transaction creates editable wrapper pages and domain root references without rewriting existing encrypted data. Existing page routes redirect to canonical URLs. Backups remain usable; reverting to the prior binary requires restoring a pre-upgrade backup.

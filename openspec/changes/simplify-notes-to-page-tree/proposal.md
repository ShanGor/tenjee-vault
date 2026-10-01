# Proposal

## Why

Notebook, group, and section containers make creating and finding notes unnecessarily complex. Spaces should contain one tree of editable pages.

## What Changes

- **BREAKING** Replace the notebook navigation DTO and notebook, group, and section navigation with a single expandable page tree and editor.
- Create root pages directly in a space and child pages beneath any page.
- Migrate existing containers to editable parent pages, preserving existing page IDs, content, histories, attachments, and encryption keys.
- Protect a page subtree with inherited encryption; support moving, ordering, recycling, and restoring subtrees.
- Preserve old links through redirects and update search, templates, import/export, and quick capture to the page model.

## Capabilities

### New Capabilities

### Modified Capabilities

- `notes-module`: Page-only organization, subtree protection, migration, and navigation.

## Impact

React notes navigation, routes, editor and supporting tools; Rust hierarchy commands, storage migration and crypto integration. Existing section records remain internal encryption domains for ciphertext compatibility, with no container types exposed to users.

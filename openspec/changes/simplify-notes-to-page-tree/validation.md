# Validation

- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: 215 passed; one existing large encryption benchmark intentionally ignored.
- `cargo check --manifest-path src-tauri/Cargo.toml --quiet`: passed after the final navigation DTO cleanup.
- `npm run build`: passed; Vite retains its existing large-bundle advisory.
- `npm test`: eight frontend tests and two release script checks passed.
- `openspec validate simplify-notes-to-page-tree --strict`: passed.
- `git diff --check`: passed.

New backend coverage exercises fresh spaces, migration of nested containers with unchanged ciphertext and page IDs, cycle prevention, subtree moves, recycling/restoration and permanent deletion, inherited protection, versions and attachments, failed-decryption rollback with no staged plaintext blob retained, and subtree exports including empty parent pages and locked descendants.

A React Router server-render check confirms that the layout receives the space and page parameters needed by the page sidebar. A desktop visual session was not run.

Existing spaces migrate when opened by the updated backend. Independent protected trees retain their encryption domains and cannot be nested within another independent protected tree; moving an inherited child outside its protected tree requires removing protection first.

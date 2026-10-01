# Tasks

## 1. Backend task generation

- [x] 1.1 Add validated medication-course input and atomic task generation in the task domain; verify Rust compilation and confirm validation precedes transactional inserts by code inspection.
- [x] 1.2 Add and register a Tauri command for medication-course generation; verify the API contract compiles and the command is included in the Tauri handler.

## 2. Medication-course creation UI

- [x] 2.1 Add an API wrapper and localized medication-course form with list, start date, duration, optional medicine name, and editable labeled dose times; verify TypeScript production build succeeds and the form maps these fields to the API contract.
- [x] 2.2 Connect form submission to the atomic command and refresh the task list; verify TypeScript production build succeeds and the generated tasks are shown through the existing independently checkable task rows.

# Proposal

## Why

Periodic reminders already fit calendar events, but recording a finite treatment course requires a separate task for every dose. Creating those doses manually is tedious and makes it harder to see which individual doses were completed or missed.

## What Changes

- Add a medication-course task flow that generates one independent, checkable task per scheduled dose across a finite date range.
- Let users optionally provide the medicine name and define each daily dose with a label and scheduled time.
- Set each generated task's due date, due time, and reminder to its scheduled dose time.
- Keep ordinary periodic reminders on the existing calendar recurrence and notification flow.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `tasks-module`: add finite medication-course generation with independently completable dose tasks.

## Impact

- Task creation UI and task API.
- Tauri task command and task domain logic; no database schema change is expected because each dose uses existing task fields.
- Existing calendar recurrence and reminder behavior is reused without changing its contract.

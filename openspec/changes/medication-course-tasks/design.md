# Design

## Context

The task database already stores a due date, due time, reminder timestamp, and editable title for each task. The reminder service delivers task notifications and excludes completed tasks. Task lists are stored separately from calendar events. Calendar RRULE events already cover repeating reminders with intervals and bounded end conditions; this change does not alter that behavior.

## Goals / Non-Goals

**Goals:**

- Generate all doses in a finite medication course as separate ordinary tasks.
- Let users set a label and local time for each daily dose and optionally name the medicine.
- Use existing task reminder delivery and completion tracking.
- Avoid partial courses when input is invalid or persistence fails.

**Non-Goals:**

- Add a medication database, dose amount/unit tracking, refill management, or medical advice.
- Change calendar recurrence or the existing completion-driven recurring-task behavior.
- Add persistent course/group records or bulk editing of a course after generation.

## Decisions

1. **Use independent task rows rather than a recurring task rule.** The current recurring-task engine creates the next occurrence only after completion and skips overdue occurrences. Pre-generating the finite course keeps missed doses visible and each dose independently checkable.

2. **Represent the course as ordinary tasks without a new database entity.** The task title contains a localized medication prefix, optional medicine name, and dose label. Users can later edit any generated task like any other task. A course ID or parent task would add lifecycle and grouping semantics not needed for marking doses.

3. **Create the full batch in one backend transaction.** A single task command receives list, start date, duration, title prefix, and labeled dose times. The task domain validates the list, date, duration, labels, and times before inserting rows. Each row uses its scheduled local date/time for due date, due time, and reminder timestamp.

4. **Use the existing task list selection.** Generated doses are placed in the selected task list and use the existing list, today, week, overdue, and search views.

5. **Leave general periodic reminders in Calendar.** Calendar event recurrence already supports daily/weekly frequencies, intervals, occurrence counts or end dates, and event reminders; no duplicate reminder model is introduced.

## Risks / Trade-offs

- [Generated tasks are not grouped for later course-wide edits] → Use a consistent title prefix and let each task remain individually editable; persistent course management can be designed separately if needed.
- [A large duration combined with many daily doses can create many rows] → Validate positive bounded duration and dose count before opening the transaction.
- [A reminder is scheduled at the dose time] → This is appropriate for adherence checklists; users can edit or clear individual reminders afterward.

## Migration Plan

No data migration is needed. Existing tasks and calendar events remain unchanged.

# Spec Delta

## ADDED Requirements

### Requirement: Finite medication course tasks

The system SHALL let users generate a finite course of independent medication dose tasks by choosing a task list, start date, number of days, and one or more daily dose labels and times. The medicine name SHALL be optional. Each generated task SHALL have its own due date, due time, and reminder at the scheduled dose time, and SHALL be independently completable. When provided, the medicine name and dose label SHALL be included in each generated task title. The system SHALL create the course atomically so validation or storage failure does not leave a partial course.

#### Scenario: Generate a seven-day course with three doses per day

- **WHEN** the user creates a seven-day course starting on a selected date with three labeled dose times per day
- **THEN** the system creates 21 separate tasks, one for each date and dose time, and each task can be completed independently

#### Scenario: Include an optional medicine name

- **WHEN** the user supplies a medicine name while creating a course
- **THEN** every generated dose task title includes that medicine name and its dose label

#### Scenario: Schedule dose reminders

- **WHEN** a course dose is scheduled for a particular local date and time
- **THEN** its task due date, due time, and reminder are set to that date and time

#### Scenario: Reject invalid course input without partial tasks

- **WHEN** the course has an invalid date, duration, dose label, dose time, or task list
- **THEN** the system reports an error and creates no tasks from that course

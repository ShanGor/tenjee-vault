# Spec Delta

## Purpose

Provide a usable phone and tablet experience for Tenjee Vault while sharing its local data model, page protection, and core functionality with the desktop application.

## ADDED Requirements

### Requirement: Mobile application targets

The system SHALL provide Android and iOS applications with local storage and offline notes, tasks, calendar, search, and protection. Platform-dependent file selection, sharing, notifications, and permissions SHALL use supported mobile integration. Desktop-only capabilities SHALL be gated by platform. Reminder delivery while suspended SHALL use OS scheduling subject to OS permissions and limits; the application SHALL communicate unavailable capabilities.

#### Scenario: Use a phone offline
- **WHEN** a user opens an installed mobile application without network access
- **THEN** the user can read and edit local notes, complete tasks, inspect the calendar, and unlock protected trees using their existing protection passwords

#### Scenario: Permission is denied
- **WHEN** notification or file access permission is denied
- **THEN** the application explains the affected operation and remains usable for other local operations

### Requirement: Adaptive navigation and touch interaction

At phone widths the system SHALL present a single primary pane, bottom navigation for Notes, Tasks, Calendar, and More, accessible search and quick capture, and Tags, Settings, and local network exchange under More. Primary touch targets SHALL be at least 44 by 44 CSS pixels, text inputs SHALL use at least 16 CSS pixels, and core screens SHALL work at viewport widths of 320–600 CSS pixels without horizontal page scrolling. Tablet and desktop layouts SHALL adapt to available width. Safe areas, larger text, orientation changes, and virtual keyboards SHALL leave focused inputs and actions reachable. Required actions SHALL have visible touch controls rather than depend on hover, keyboard shortcuts, or dragging.

#### Scenario: Navigate on a narrow phone
- **WHEN** a user opens the application at a 320 CSS pixel viewport width
- **THEN** the current module fits the screen, its primary actions are reachable by touch, and the user can switch modules and open More without horizontal page scrolling

#### Scenario: Keyboard opens during capture
- **WHEN** the keyboard opens while entering a note or task
- **THEN** the focused field and completion action remain reachable and the bottom navigation does not obscure the form

### Requirement: Mobile notes workflow

The system SHALL show a full-width reading or editing page on phones, move the space selector and expandable page tree into a dismissible navigation drawer, and preserve the existing read-first, Edit, and Done editing workflow. It SHALL provide compact breadcrumbs, contextual formatting and insertion controls, touch alternatives for move/reorder, and saved/pending/error status. Changing pages or leaving editing SHALL flush pending saves or communicate failure. Wide document tables SHALL scroll within their own region.

#### Scenario: Open and edit a child page
- **WHEN** a user selects a child page from the drawer and chooses Edit
- **THEN** the drawer closes, the page occupies the main screen, editing controls remain accessible, and Done editing saves the page and returns to reading

### Requirement: Mobile tasks workflow

The system SHALL default to a task list on phones, expose list/smart-view selection through a compact selector, open task details and long forms as full-screen views, and provide explicit bulk selection and move/status controls. Optional Kanban SHALL display one navigable status column at a time on phones. Task hierarchies and medication course creation SHALL remain usable by touch.

#### Scenario: Update task details
- **WHEN** a user opens a task from a phone list
- **THEN** a full-screen detail view appears and returning restores the prior list/filter/scroll position

### Requirement: Mobile calendar workflow

The system SHALL default to Agenda on first phone use and retain the user's subsequent choice separately from desktop view state. Month view SHALL use a compact seven-column date picker with activity markers and a selected-day agenda; Day SHALL provide the detailed timeline. Optional Week SHALL show readable day summaries or a selected-day timeline. Event forms SHALL use full-screen layouts, secondary controls SHALL be grouped in menus, and event creation/rescheduling SHALL have explicit touch controls. Lunar, festival, and solar-term information SHALL remain available without overcrowding date cells.

#### Scenario: Inspect a busy calendar day
- **WHEN** a phone user selects a date in Month view
- **THEN** that date's events and due tasks appear in a readable list below the date picker without requiring a 700 pixel calendar surface

### Requirement: Mobile exchange lifecycle

The system SHALL present discovery, code entry, approval, progress, conflicts, and completion as readable mobile screens. When the app is suspended or closed, it SHALL stop discovery and terminate the authenticated session while preserving durable transfer progress. Returning SHALL explain the interruption and require fresh pairing to continue.

#### Scenario: Suspend during exchange
- **WHEN** the OS suspends the phone while an exchange is active
- **THEN** secrets and network session resources are released, incomplete work is recoverable, and reopening offers a new pairing session to resume

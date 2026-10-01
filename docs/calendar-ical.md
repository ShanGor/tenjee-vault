# iCalendar interoperability

Tenjee Vault exports Gregorian events as UTF-8 RFC 5545 `VCALENDAR` data. The generated file uses
standard `VEVENT` fields for the event UID, title, description, location, start/end values,
supported `RRULE` values, and `TZID` when an event has a named time zone. All-day events use the
exclusive `DTEND;VALUE=DATE` convention required by iCalendar.

Recurring-event exceptions are exported as override VEVENTs with the same UID and a
`RECURRENCE-ID`: cancelled occurrences have `STATUS:CANCELLED`; moved occurrences carry their new
start and duration-preserving end. Text is escaped and content lines are folded at the RFC's
75-octet boundary, so output is suitable for common calendar clients.

The current exporter intentionally covers Gregorian events only. Lunar recurrences require a
finite export range and are materialized as Gregorian instances by the subsequent lunar-export
work; they are never emitted as an inaccurate Gregorian `RRULE`.

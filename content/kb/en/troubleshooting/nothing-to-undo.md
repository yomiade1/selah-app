---
id: troubleshooting.nothing-to-undo
title: Nothing left to undo
type: troubleshooting
status: review
audience: [volunteer, producer, admin, it]
modes: [simple, producer]
context: []
settings: []
error_codes: [SEL-LIB-0004]
summary: Selah already reached the start or end of the undo steps for this item.
popover: ""
user_need: As a producer, I want to know why undo or redo stopped so that I don't think work was lost.
keywords: [undo, redo, history, revert, changes]
related: [troubleshooting.library-item-not-found, getting-started.report-a-problem]
collections: [producer-guide]
applies_to: ">=1.0"
platforms: [windows, macos]
last_reviewed: 2026-11-09
reviewed_by: ""
---

# Nothing left to undo

You reached the end of the undo or redo steps. No more changes wait in that list.

## Why this happens

- You already undid back to when you opened the library or saved the first copy.
- You made a new change after an undo, which clears old redo steps for that item.

## Related

- [Library item not found](kb:troubleshooting.library-item-not-found)
- [Report a problem](kb:getting-started.report-a-problem)

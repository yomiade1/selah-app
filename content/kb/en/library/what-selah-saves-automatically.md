---
id: library.what-selah-saves-automatically
title: What Selah saves automatically
type: concept
status: review
audience: [volunteer, producer, admin]
modes: [simple, producer]
context: [editor.paste, library.exchange]
settings: []
error_codes: []
summary: Selah autosaves song drafts while you type, writes changes atomically, and checks files when the app opens.
popover: ""
user_need: As a volunteer, I want to know what Selah saves on its own so that I don't lose work if the app closes.
keywords: [autosave, draft, crash recovery, sqlite, media hash, save automatically]
related: [library.undo-and-redo, library.bundles-and-backups]
collections: [volunteer-handbook, admin-guide]
applies_to: ">=1.0"
platforms: [windows, macos]
last_reviewed: 2026-11-09
reviewed_by: ""
---

# What Selah saves automatically

As you edit a song, Selah saves your unsaved draft while you work. If Selah stops unexpectedly, your draft comes back when the app opens again.

## Why it helps

Saved songs, Bibles, and media records write in safe steps. Selah checks the library file every time it starts and keeps broken files from being replaced.

## What to do next

- [Undo and redo changes](kb:library.undo-and-redo)
- [Back up and share your library](kb:library.bundles-and-backups)

---
id: templates.text-that-fits-every-screen
title: Text that fits every screen
type: concept
status: review
audience: [producer]
modes: [producer]
context: [templates.editor, editor.slides]
settings: []
error_codes: []
summary: Selah measures real letter widths so words shrink neatly or split into extra slides before clipping.
popover: ""
user_need: As a producer, I want to understand how slide fit works so that words never clip on wide LED walls.
keywords: [metric fit, shrink to fit, max lines, safe area, overflow, led wall]
related: [templates.fonts-outlines-and-boxes, templates.screen-shapes-and-safe-areas, songs.slide-breaks-and-lines-per-slide]
collections: [producer-guide]
applies_to: ">=1.0"
platforms: [windows, macos]
last_reviewed: 2026-11-10
reviewed_by: ""
---

# Text that fits every screen

Selah measures your exact font, weight, spacing, and margin width before drawing each slide. Words stay inside the **Safe area margins**.

## Why it helps

A wide LED screen holds fewer lines than a tall room screen. If text is too big, Selah shrinks it first down to **Smallest size (pt)**, then splits overflow onto a new slide.

## An example

A four-line chorus fits on **16:9** at full size, then splits into two clean slides on **LED 32:9**.

## What to do next

- [Set fonts, outlines, and card boxes](kb:templates.fonts-outlines-and-boxes)
- [Set screen shapes and safe area margins](kb:templates.screen-shapes-and-safe-areas)

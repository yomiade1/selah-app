---
id: live.lock-simple-mode-for-volunteers
title: Lock Selah to Simple mode for volunteers
type: task
status: review
audience: [producer, admin]
modes: [producer, simple]
context: [live.role-lock, simple.unlock]
settings: []
error_codes: [SEL-SEC-0002, SEL-SEC-0003, SEL-SEC-0004]
summary: Set a PIN to keep volunteers in Simple mode. Only someone with the PIN can open Producer mode or change the library.
popover: "Lock Selah to Simple mode. Only someone with the PIN can unlock it."
user_need: As a producer, I want to keep volunteers in Simple mode so that settings and songs can't change by accident.
keywords: [lock, role, volunteer, simple, pin, unlock, admin, permission]
related: [simple.run-a-service-in-simple-mode, live.lock-the-booth, troubleshooting.you-dont-have-permission]
collections: [producer-guide]
applies_to: ">=1.0"
platforms: [windows, macos]
last_reviewed: 2026-11-11
reviewed_by: ""
---

# Lock Selah to Simple mode for volunteers

You can lock Selah so that volunteers see only Simple mode. They can show slides and clear the screens. They can't open Producer mode, edit songs, or change settings.

## Lock it

1. In Producer mode, select **Lock to Simple mode**.
2. Enter a **PIN** of 4 to 8 numbers.
3. Select **Lock to Simple mode** again.

Selah switches to Simple mode. It stays locked even after a restart.

## Unlock it

1. In Simple mode, select **Unlock**.
2. Enter the **PIN**, then select **Unlock**.

> [!WARNING]
> After 5 wrong tries, Selah waits 30 seconds before you can try again.

## Related

- [Run a service in Simple mode](kb:simple.run-a-service-in-simple-mode)
- [Lock the booth](kb:live.lock-the-booth)
- [You don't have permission](kb:troubleshooting.you-dont-have-permission)

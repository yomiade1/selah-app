# Changes

## Unreleased

Milestone 2 (in progress): library storage foundations.

- Added database storage for songs, templates, media, custom presentations and service data.
  Library editing controls will follow in the remaining milestone 2 work.
- Library storage checks saved data on open, keeps media by its content hash and refuses
  corrupt files without replacing the library.

Milestone 1: the foundations. This is a test build for checking the basics, not for a service yet.

- Selah has a Windows installer. It installs just for you, without administrator rights, and
  adds Selah to the Start menu. Uninstalling it keeps your settings and library.
- Selah can update itself, but only when you ask. Turn it on in Settings, under Updates. It
  checks each download before installing and never installs while something is on the screens.
- Selah doesn't connect to the internet unless you turn on a feature that needs it.
- Passwords and keys are kept in your computer's own secure store (Credential Manager or the
  Keychain).
- If you quit while something is on the screens, Selah asks first. Your computer doesn't go to
  sleep while something is showing.
- Clear everything (or F12) clears the screens even if part of Selah has stopped responding.
- In safe mode, Restart normally starts Selah again with everything on, without touching the
  screens.
- Help, then Report a problem, saves a support file with no passwords or names in it and opens
  the problem form for you.
- Help, then Diagnostics, shows details about your computer and runs a short speed test you
  can send to us.
- Settings has 7 text sizes and Light and Booth looks, and can follow your computer's text size.
- Help articles open inside Selah.

- Selah runs as separate programs for the screens, the data and the window you use. If one of
  them stops, Selah starts it again within 2 seconds and the screens keep showing the same thing.
- Selah can show a test pattern (colour bars, a grid or a grey ramp) on an output window.
- Words on the screens are drawn the same way on every computer, with the fonts Selah brings.
  English, Yorùbá (with tone marks), Korean and Arabic (right to left) are checked.
- If Selah crashes 3 times in a minute, it starts in safe mode.
- Selah's programs only talk to each other on your computer, and only as your user account.
- Logs leave out passwords, keys, email addresses and your user name.
- Every change is checked automatically on Windows, macOS and Linux before it's merged.

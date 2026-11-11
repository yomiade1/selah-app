"""The Windows type-check coverage guard (tools/check-windows-coverage.py).

Gate stage 5 skips the Skia- and FFmpeg-linked crates when cross-checking for Windows, which
is how Windows-only code once stopped being type-checked without anyone noticing (M1-A1).
These tests pin the guard's behaviour: it flags conditional compilation, it ignores runtime
`cfg!(windows)` branches and the benign windows_subsystem attribute, and it accepts an
exclusion that records the loss on purpose.
"""
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/check-windows-coverage.py"
SPEC = importlib.util.spec_from_file_location("check_windows_coverage", SCRIPT)
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


class DetectionTests(unittest.TestCase):
    """What counts as code that needs a Windows target to be type-checked."""

    def test_cfg_attribute_counts(self):
        self.assertTrue(guard.WINDOWS_ATTR.search('#[cfg(windows)]\nfn f() {}'))
        self.assertTrue(guard.WINDOWS_ATTR.search('#[cfg(target_os = "windows")]\nfn f() {}'))
        self.assertTrue(guard.WINDOWS_ATTR.search('#[cfg(all(windows, feature = "x"))]'))
        self.assertTrue(guard.WINDOWS_ATTR.search('#[cfg(target_family = "windows")]'))

    def test_runtime_cfg_branch_does_not_count(self):
        """Both arms of `cfg!(windows)` compile on every platform already."""
        self.assertIsNone(guard.WINDOWS_ATTR.search('let p = if cfg!(windows) { 1 } else { 2 };'))

    def test_windows_subsystem_attribute_is_benign(self):
        attr = '#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]'
        match = guard.WINDOWS_ATTR.search(attr)
        self.assertIsNotNone(match, "the attribute is conditional")
        self.assertTrue(guard.BENIGN_ATTR.search(match.group(0)), "but it gates no code")

    def test_cargo_manifest_target_section_counts(self):
        self.assertTrue(guard.WINDOWS_MANIFEST.search("[target.'cfg(windows)'.dependencies]\n"))
        self.assertTrue(guard.WINDOWS_MANIFEST.search("[target.x86_64-pc-windows-msvc.dependencies]\n"))
        self.assertIsNone(guard.WINDOWS_MANIFEST.search("[dependencies]\nwindows = 1\n"))


class RepositoryTests(unittest.TestCase):
    """The guard's verdict on the repository as it stands."""

    def test_the_repository_passes(self):
        self.assertEqual(guard.main(), 0)

    def test_every_excluded_crate_exists(self):
        for crate in guard.excluded_crates():
            self.assertTrue((guard.CRATES / crate).is_dir(), f"{crate} is not a crate")

    def test_the_crates_holding_windows_code_are_checked(self):
        """selah-ipc, selah-runtime, selah-secrets and selah-watchdog must stay in the check."""
        excluded = set(guard.excluded_crates())
        for crate in ("selah-ipc", "selah-runtime", "selah-secrets", "selah-watchdog"):
            self.assertNotIn(crate, excluded, f"{crate} holds Windows-only code")

    def test_an_unacknowledged_exclusion_fails(self):
        """Adding Windows-only code to an excluded crate must turn the gate red."""
        with patch.object(guard, "has_windows_code", lambda d: [d / "src" / "lib.rs"]):
            with patch.object(guard, "acknowledged_crates", lambda: set()):
                self.assertEqual(guard.main(), 1)

    def test_an_acknowledged_exclusion_passes(self):
        """Recording the loss in the exclusions file is accepted."""
        with patch.object(guard, "has_windows_code", lambda d: [d / "src" / "lib.rs"]):
            with patch.object(guard, "acknowledged_crates", lambda: set(guard.excluded_crates())):
                self.assertEqual(guard.main(), 0)

    def test_media_and_testkit_are_acknowledged(self):
        """Both really do hold Windows-only code the cloud session cannot check."""
        acknowledged = guard.acknowledged_crates()
        self.assertIn("selah-media", acknowledged)
        self.assertIn("selah-testkit", acknowledged)


if __name__ == "__main__":
    unittest.main()

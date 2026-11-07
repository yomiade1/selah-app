"""Exercise prerequisite failure handling without installing anything."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / "tools/check-toolchain.py"
SPEC = importlib.util.spec_from_file_location("toolchain", SCRIPT)
toolchain = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(toolchain)


class ToolchainTests(unittest.TestCase):
    def test_missing_executable_is_actionable(self):
        with patch.object(toolchain.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "cargo is missing"):
                toolchain.run("cargo", "--version")

    def test_failed_executable_is_not_reported_as_installed(self):
        result = subprocess.CompletedProcess([], 1, "", "failure")
        with patch.object(toolchain.shutil, "which", return_value="cargo"), \
                patch.object(toolchain.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "exit 1"):
                toolchain.run("cargo", "--version")

    def test_version_check_has_a_deadline(self):
        result = subprocess.CompletedProcess([], 0, "cargo 1.91.0\n", "")
        with patch.object(toolchain.shutil, "which", return_value="cargo"), \
                patch.object(toolchain.subprocess, "run", return_value=result) as probe:
            self.assertEqual(toolchain.run("cargo", "--version"), "cargo 1.91.0")
            self.assertEqual(probe.call_args.kwargs["timeout"], 30)

    def check_with_versions(self, versions):
        versions.setdefault("rustup", "1.91.0-x86_64-pc-windows-msvc (default)")
        output = io.StringIO()
        with patch.object(toolchain, "run", side_effect=lambda name, *args: versions[name]), \
                patch.object(toolchain.os, "name", "posix"), \
                contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            status = toolchain.check()
        return status, output.getvalue()

    def test_matching_toolchain_passes(self):
        status, output = self.check_with_versions({
            "cargo": "cargo 1.91.0 (test)", "dotnet": "10.0.100 [sdk]",
            "node": "v22.21.0", "pnpm": "10.20.0", "clang": "clang version 20",
        })
        self.assertEqual(status, 0, output)

    def test_wrong_versions_fail_with_annotations(self):
        status, output = self.check_with_versions({
            "cargo": "cargo 1.90.0 (test)", "dotnet": "9.0.100 [sdk]",
            "node": "v24.11.0", "pnpm": "9.0.0", "clang": "clang version 20",
        })
        self.assertEqual(status, 1)
        self.assertEqual(output.count("::error"), 4)

    def test_later_dotnet_10_feature_band_matches_global_roll_forward(self):
        status, output = self.check_with_versions({
            "cargo": "cargo 1.91.0 (test)", "dotnet": "10.0.402 [sdk]",
            "node": "v22.21.1", "pnpm": "10.20.0", "clang": "clang version 20",
        })
        self.assertEqual(status, 0, output)

    def test_check_does_not_invoke_a_shim_for_a_missing_toolchain(self):
        with patch.object(toolchain, "run", return_value="1.92.0-x86_64-pc-windows-msvc") as probe:
            self.assertFalse(toolchain.rust_ok())
            probe.assert_called_once_with("rustup", "toolchain", "list")


if __name__ == "__main__":
    unittest.main()

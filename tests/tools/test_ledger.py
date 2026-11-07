"""The test ledger counts the right suites and never goes down (ADR-016)."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / "tools/test-ledger.py"
SPEC = importlib.util.spec_from_file_location("test_ledger", SCRIPT)
ledger = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ledger)

CARGO_LOG = """
     Running unittests src\\lib.rs (target\\debug\\deps\\selah_ipc-0123abcd.exe)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests\\continuity.rs (target\\debug\\deps\\continuity-f8541e82.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/launch.rs (target/debug/deps/launch-9ef96d35)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests\\transport.rs (target\\debug\\deps\\transport-77.exe)
test result: ok. 6 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
"""


class LedgerTests(unittest.TestCase):
    def test_end_to_end_files_are_counted_separately_and_ignored_tests_do_not_count(self):
        self.assertEqual(ledger.rust_counts(CARGO_LOG), (19, 6))

    def test_update_never_lowers_a_count(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.json"
            path.write_text(json.dumps({"rust": 100, "dotnet": 1}), encoding="utf-8")
            counts = dict.fromkeys(ledger.SUITES, 0)
            counts.update(rust=90, dotnet=5)
            with patch.object(ledger, "LEDGER", path), \
                    patch.object(ledger, "current_counts", return_value=counts), \
                    patch("sys.argv", ["test-ledger.py", "--update"]), \
                    patch.object(ledger, "ROOT", Path(directory)):
                self.assertEqual(ledger.main(), 0)
            saved = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual((saved["rust"], saved["dotnet"]), (100, 5))

    def test_check_fails_when_a_suite_shrinks(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.json"
            path.write_text(json.dumps({"rust": 10}), encoding="utf-8")
            counts = dict.fromkeys(ledger.SUITES, 0)
            counts.update(rust=9, total=9)
            with patch.object(ledger, "LEDGER", path), \
                    patch.object(ledger, "current_counts", return_value=counts), \
                    patch("sys.argv", ["test-ledger.py", "--check"]):
                self.assertEqual(ledger.main(), 1)


if __name__ == "__main__":
    unittest.main()

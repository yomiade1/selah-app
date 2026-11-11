"""The documentation evidence checker (tools/check-evidence.py).

PROGRESS.md and docs/SECURITY.md prove each claim by naming tests. Five of those names were
stale at the end of M3, including both tests cited for SAFE-05 (the audio limiter). These
tests pin the checker that now catches that.
"""
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/check-evidence.py"
SPEC = importlib.util.spec_from_file_location("check_evidence", SCRIPT)
evidence = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(evidence)


class CandidateTests(unittest.TestCase):
    """Which backticked names are treated as test names."""

    def test_four_word_snake_case_is_a_candidate(self):
        found = evidence.CANDIDATE.findall("evidence: `clear_fade_ramps_smoothly_to_silence` passes")
        self.assertEqual(found, ["clear_fade_ramps_smoothly_to_silence"])

    def test_short_names_are_left_alone(self):
        """Field, column and setting names are not tests."""
        for name in ("`max_lines`", "`flash_warning`", "`editor_buffers`", "`m3_p99_ms`"):
            self.assertEqual(evidence.CANDIDATE.findall(name), [], f"{name} should be ignored")

    def test_paths_and_commands_are_not_candidates(self):
        self.assertEqual(evidence.CANDIDATE.findall("`tools/verify.sh --cloud`"), [])
        self.assertEqual(evidence.CANDIDATE.findall("`selah-core/tests/thumbnails.rs`"), [])


class RepositoryTests(unittest.TestCase):
    def test_every_cited_test_name_exists(self):
        self.assertEqual(evidence.main(), 0)

    def test_the_function_index_is_populated(self):
        names = evidence.known_function_names()
        self.assertGreater(len(names), 100)
        # A test that must always be there: the SAFE-03 emergency chaos test.
        self.assertIn("clear_everything_works_with_core_frozen", names)

    def test_a_stale_name_fails(self):
        """The exact failure mode found at the end of M3."""
        with patch.object(evidence, "known_function_names", lambda: {"only_this_one"} | {f"f{i}" for i in range(200)}):
            self.assertEqual(evidence.main(), 1)

    def test_an_empty_index_is_reported_rather_than_passing_silently(self):
        with patch.object(evidence, "known_function_names", lambda: set()):
            self.assertEqual(evidence.main(), 1)


if __name__ == "__main__":
    unittest.main()

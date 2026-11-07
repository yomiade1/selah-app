"""tools/lint-strings.py: plain words in UI strings and no literal text in views."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / "tools/lint-strings.py"
SPEC = importlib.util.spec_from_file_location("lint_strings", SCRIPT)
lint = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lint)


class LintStringsTests(unittest.TestCase):
    def test_banned_words_match_whole_words_only(self):
        pattern = lint.phrase_pattern("tap")
        self.assertTrue(pattern.search("Tap the button"))
        self.assertFalse(pattern.search("Select the tape deck"))
        self.assertTrue(lint.phrase_pattern("e.g.").search("Songs, e.g. hymns"))

    def test_terms_file_shape_is_read(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "terms.yaml"
            path.write_text("banned_words:\n  - click\n  - in order to\nproduct_terms:\n"
                            "  service:\n    - playlist\n  slide:\n    - page\n", encoding="utf-8")
            terms = lint.load_terms(path)
        self.assertEqual(terms["banned_words"], ["click", "in order to"])
        self.assertEqual(terms["product_terms"], [("service", ["playlist"]), ("slide", ["page"])])

    def test_literal_view_text_is_found_and_bindings_are_not(self):
        found = [m["value"] for m in lint.VISIBLE_ATTRIBUTES.finditer(
            '<TextBlock Text="Show now" /><Button Content="{Binding [show-now]}" ToolTip.Tip="Clear" />')]
        self.assertEqual(found, ["Show now", "{Binding [show-now]}", "Clear"])

    def test_the_repository_strings_pass(self):
        self.assertEqual(lint.main(), 0)


if __name__ == "__main__":
    unittest.main()

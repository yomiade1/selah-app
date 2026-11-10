"""tools/gen_help_anchors.py: deterministic help-anchor C# constants from anchors.yaml."""
import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / "tools/gen_help_anchors.py"
SPEC = importlib.util.spec_from_file_location("gen_help_anchors", SCRIPT)
gen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gen)


class HelpAnchorCodegenTests(unittest.TestCase):
    def test_const_name_converts_dots_and_hyphens(self):
        self.assertEqual(gen.to_const_name("shell.status"), "ShellStatus")
        self.assertEqual(gen.to_const_name("shell.show-test-pattern"), "ShellShowTestPattern")
        self.assertEqual(gen.to_const_name("editor.paste"), "EditorPaste")

    def test_parse_and_render_handles_comments_and_quotes(self):
        yaml_text = (
            "# Header comment\n"
            "anchors:\n"
            "  - id: shell.status\n"
            '    description: "Status <pill> & badge."\n'
            "  - id: library.songs\n"
            "    description: Library songs.\n"
        )
        anchors = gen.parse_anchors(yaml_text)
        self.assertEqual(
            anchors,
            [("shell.status", "Status <pill> & badge."), ("library.songs", "Library songs.")],
        )
        cs = gen.render_cs(anchors)
        self.assertIn('public const string ShellStatus = "shell.status";', cs)
        self.assertIn("Status &lt;pill&gt; &amp; badge.", cs)
        self.assertIn('public const string LibrarySongs = "library.songs";', cs)

    def test_rejects_duplicate_anchors(self):
        with self.assertRaises(ValueError):
            gen.render_cs([("shell.status", "One"), ("shell.status", "Two")])

    def test_committed_generated_file_matches_anchors_yaml(self):
        self.assertEqual(gen.main(["--check"]), 0)


if __name__ == "__main__":
    unittest.main()

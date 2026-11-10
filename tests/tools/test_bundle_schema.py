"""Checks for the canonical .selah exchange bundle JSON Schema (M2.6)."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATH = ROOT / "formats" / "selah-bundle.schema.json"


class BundleSchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))

    def test_root_requires_all_canonical_fields_and_forbids_unknown_properties(self):
        self.assertEqual(self.schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertFalse(self.schema["additionalProperties"])
        expected_required = [
            "format",
            "media_items",
            "presentations",
            "schema_version",
            "songs",
            "tags",
            "templates",
        ]
        self.assertEqual(sorted(self.schema["required"]), expected_required)
        self.assertEqual(self.schema["properties"]["format"]["const"], "selah-bundle")
        self.assertEqual(self.schema["properties"]["schema_version"]["const"], 1)

    def test_all_object_definitions_forbid_additional_properties_and_use_uuid_v7(self):
        defs = self.schema["$defs"]
        for name in ("Resolution", "Color", "Chord", "LyricLine", "Section", "Arrangement",
                     "Song", "TemplateVariant", "Template", "MediaItem", "Slide", "Presentation", "Tag"):
            self.assertIn(name, defs)
            self.assertIs(defs[name]["additionalProperties"], False, f"{name} must deny unknown fields")

        uuid_re = re.compile(defs["UuidV7"]["pattern"])
        self.assertRegex("01936a4e-8f2a-7b30-8000-010203040506", uuid_re)
        self.assertNotRegex("550e8400-e29b-41d4-a716-446655440000", uuid_re)  # v4 rejected
        self.assertNotRegex("01936A4E-8F2A-7B30-8000-010203040506", uuid_re)  # uppercase rejected

    def test_canonical_key_order_has_no_timestamps(self):
        forbidden = {"created_at", "updated_at", "timestamp", "created_at_unix_ms"}

        def walk(node):
            if isinstance(node, dict):
                props = node.get("properties")
                if isinstance(props, dict):
                    self.assertTrue(
                        forbidden.isdisjoint(props.keys()),
                        f"unexpected timestamp properties in {list(props.keys())}",
                    )
                req = node.get("required")
                if isinstance(req, list):
                    self.assertTrue(
                        forbidden.isdisjoint(req),
                        f"unexpected timestamp required fields in {req}",
                    )
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for item in node:
                    walk(item)

        walk(self.schema)


if __name__ == "__main__":
    unittest.main()

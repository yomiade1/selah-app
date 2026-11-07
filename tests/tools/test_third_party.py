"""Supply-chain checks run offline against temporary files and fake downloads."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
import tarfile
import unittest
import zipfile
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / "tools/third-party.py"
SPEC = importlib.util.spec_from_file_location("third_party", SCRIPT)
third_party = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(third_party)


class Download(io.BytesIO):
    url = "https://raw.githubusercontent.com/example/commit/font.ttf"


class ThirdPartyTests(unittest.TestCase):
    def asset(self, **kwargs):
        return dict(url=Download.url, path="fonts/example.ttf",
                    sha256=hashlib.sha256(b"fixture").hexdigest(), **kwargs)

    def test_rejects_traversal_absolute_and_windows_paths(self):
        for path in ("../outside", "/absolute", "C:/outside", "fonts\\outside"):
            asset = self.asset()
            asset["path"] = path
            with self.subTest(path=path), self.assertRaises(ValueError):
                third_party.validate(asset)

    def test_rejects_insecure_or_untrusted_source(self):
        for url in ("http://github.com/a", "https://untrusted.invalid/a",
                    "https://user:pass@github.com/a", "file:///a"):
            asset = self.asset()
            asset["url"] = url
            with self.subTest(url=url), self.assertRaises(ValueError):
                third_party.validate(asset)

    def test_matching_cache_never_opens_network(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fonts").mkdir()
            (root / "fonts/example.ttf").write_bytes(b"fixture")
            with patch.object(third_party.urllib.request, "urlopen") as download:
                third_party.fetch(self.asset(), root)
                download.assert_not_called()

    def test_hash_mismatch_preserves_existing_file_and_cleans_temporary_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fonts").mkdir()
            target = root / "fonts/example.ttf"
            target.write_bytes(b"previous")
            with patch.object(third_party.urllib.request, "urlopen", return_value=Download(b"tampered")):
                with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                    third_party.fetch(self.asset(), root)
            self.assertEqual(target.read_bytes(), b"previous")
            self.assertEqual(list(target.parent.iterdir()), [target])

    def test_verified_download_is_atomically_published(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(third_party.urllib.request, "urlopen", return_value=Download(b"fixture")):
                third_party.fetch(self.asset(), root)
            self.assertEqual((root / "fonts/example.ttf").read_bytes(), b"fixture")

    def test_oversized_download_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(third_party, "MAX_BYTES", 3), \
                    patch.object(third_party.urllib.request, "urlopen", return_value=Download(b"fixture")):
                with self.assertRaisesRegex(ValueError, "size limit"):
                    third_party.fetch(self.asset(), Path(directory))
            self.assertEqual(list((Path(directory) / "fonts").iterdir()), [])

    def test_slow_download_has_a_total_deadline_and_cleans_up(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(third_party.time, "monotonic",
                              side_effect=[0, third_party.MAX_SECONDS + 1]), \
                    patch.object(third_party.urllib.request, "urlopen", return_value=Download(b"fixture")):
                with self.assertRaisesRegex(TimeoutError, "time limit"):
                    third_party.fetch(self.asset(), root)
            self.assertEqual(list((root / "fonts").iterdir()), [])

    def archive_asset(self, root, data, name="ffmpeg.zip"):
        target = root / "ffmpeg" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        return {"url": Download.url, "path": f"ffmpeg/{name}",
                "sha256": hashlib.sha256(data).hexdigest()}

    def zip_bytes(self, name="build/bin/ffmpeg", data=b"fixture"):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr(name, data)
        return buffer.getvalue()

    def test_verified_zip_is_extracted_with_a_source_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = self.archive_asset(root, self.zip_bytes())
            third_party.extract(asset, root)
            target = root / "ffmpeg/shared/build/bin/ffmpeg"
            self.assertEqual(target.read_bytes(), b"fixture")
            third_party.extract(asset, root, check_only=True)

    def test_archive_traversal_is_rejected_without_publishing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = self.archive_asset(root, self.zip_bytes("../outside"))
            with self.assertRaisesRegex(ValueError, "unsafe archive"):
                third_party.extract(asset, root)
            self.assertFalse((root / "outside").exists())
            self.assertFalse((root / "ffmpeg/shared").exists())

    def test_archive_expansion_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = self.archive_asset(root, self.zip_bytes())
            with patch.object(third_party, "MAX_BYTES", 2):
                with self.assertRaisesRegex(ValueError, "size limit"):
                    third_party.extract(asset, root)
            self.assertFalse((root / "ffmpeg/shared").exists())

    def test_check_only_does_not_extract_a_missing_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = self.archive_asset(root, self.zip_bytes())
            with self.assertRaisesRegex(ValueError, "not extracted"):
                third_party.extract(asset, root, check_only=True)
            self.assertFalse((root / "ffmpeg/shared").exists())

    def test_modified_archive_is_refused_before_extracting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            asset = self.archive_asset(root, self.zip_bytes())
            (root / asset["path"]).write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "pinned hash"):
                third_party.extract(asset, root)
            self.assertFalse((root / "ffmpeg/shared").exists())

    def test_tar_links_are_regular_copies_and_cannot_escape(self):
        for link in ["library.so.1", "../../outside"]:
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode="w:xz") as archive:
                file = tarfile.TarInfo("build/library.so.1")
                file.size = 7
                archive.addfile(file, io.BytesIO(b"fixture"))
                alias = tarfile.TarInfo("build/library.so")
                alias.type = tarfile.SYMTYPE
                alias.linkname = link
                archive.addfile(alias)
            with self.subTest(link=link), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                asset = self.archive_asset(root, buffer.getvalue(), "ffmpeg.tar.xz")
                if link.startswith(".."):
                    with self.assertRaisesRegex(ValueError, "link escapes"):
                        third_party.extract(asset, root)
                else:
                    third_party.extract(asset, root)
                    target = root / "ffmpeg/shared/build/library.so"
                    self.assertEqual(target.read_bytes(), b"fixture")
                    self.assertFalse(target.is_symlink())


if __name__ == "__main__":
    unittest.main()

import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pack_cpp_native_provider import FILES, bundle


class ProviderBundle(unittest.TestCase):
    def test_exact_byte_inventory_and_commit_bound_deterministic_archive(self):
        contents = {name: (name+" α\r\n").encode() for name in FILES}
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory)/"a.zip", Path(directory)/"b.zip"
            first = bundle(a, contents, "0.10.2+moritz.6", "a"*40)
            bundle(b, contents, "0.10.2+moritz.6", "a"*40)
            self.assertEqual(a.read_bytes(), b.read_bytes())
            with zipfile.ZipFile(a) as archive:
                self.assertEqual(set(archive.namelist()), {*FILES, "README.txt", "manifest.json"})
                self.assertEqual(json.loads(archive.read("manifest.json")), first)
                for name, data in contents.items():
                    self.assertEqual(archive.read(name), data)
                    self.assertEqual(first["files"][name], {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
                self.assertEqual(first["revision"], "a"*40)
                self.assertNotIn("project.json", archive.namelist())

    def test_rejects_unexpected_or_missing_host_inputs_and_invalid_identity(self):
        contents = {name: b"source" for name in FILES}
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)/"bundle.zip"
            for inputs, version, revision in [({**contents, "project.json": b"private"}, "0.10.2+moritz.6", "a"*40),
                ({k:v for k,v in contents.items() if k!=FILES[0]}, "0.10.2+moritz.6", "a"*40),
                (contents, "0.10.2", "a"*40), (contents, "0.10.2+moritz.6", "HEAD")]:
                with self.assertRaises(ValueError): bundle(out, inputs, version, revision)
                self.assertFalse(out.exists())

    def test_preserves_existing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)/"bundle.zip"
            out.write_bytes(b"existing")
            with self.assertRaises(FileExistsError):
                bundle(out, {name:b"source" for name in FILES}, "0.10.2+moritz.6", "b"*40)
            self.assertEqual(out.read_bytes(), b"existing")


if __name__ == "__main__":
    unittest.main()

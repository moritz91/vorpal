import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cpp_macro_compiler_provider import definition_buffer


class DefinitionBuffer(unittest.TestCase):
    def test_same_bytes_keep_original_unicode_crlf_and_read_on_every_site(self):
        with tempfile.TemporaryDirectory() as directory:
            header = Path(directory)/'header.h'
            data = '#define CHECK(x) { consume(x); } // α\r\n'.encode()
            header.write_bytes(data)
            identity = hashlib.sha256(data).hexdigest()
            definitions, buffers = [], {}
            with patch.object(Path, 'read_bytes', autospec=True, side_effect=Path.read_bytes) as read:
                self.assertEqual(definition_buffer(header, identity.upper(), identity, definitions, buffers), 0)
                self.assertEqual(definition_buffer(header, identity, identity.upper(), definitions, buffers), 0)
                self.assertEqual(read.call_count, 2)
            self.assertEqual(definitions, [{'path': str(header), 'source': data.decode()}])
            self.assertEqual(definitions[0]['source'].encode(), data)

    def test_same_timestamp_edit_and_missing_header_abort_instead_of_reusing_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            header = Path(directory)/'header.h'
            original = b'#define CHECK(x) { before(x); }\n'
            header.write_bytes(original)
            identity = hashlib.sha256(original).hexdigest()
            stamp = header.stat()
            definitions, buffers = [], {}
            definition_buffer(header, identity, identity, definitions, buffers)
            changed = original.replace(b'before', b'edited')
            header.write_bytes(changed)
            os.utime(header, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
            for observed in (identity, hashlib.sha256(changed).hexdigest()):
                with self.assertRaises(AssertionError):
                    definition_buffer(header, observed, observed, definitions, buffers)
            self.assertEqual(definitions[0]['source'].encode(), original)
            header.unlink()
            with self.assertRaises(FileNotFoundError):
                definition_buffer(header, identity, identity, definitions, buffers)

    def test_changed_observation_or_new_capture_requires_its_own_byte_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            header = Path(directory)/'header.h'
            data = b'#define CHECK(x) { consume(x); }\n'
            header.write_bytes(data)
            identity = hashlib.sha256(data).hexdigest()
            definitions, buffers = [], {}
            for first, last in [(identity, '0'*64), ('0'*64, identity)]:
                with self.assertRaises(AssertionError):
                    definition_buffer(header, first, last, definitions, buffers)
                self.assertFalse(definitions)
                self.assertFalse(buffers)
            definition_buffer(header, identity, identity, definitions, buffers)
            with self.assertRaises(AssertionError):
                definition_buffer(header, '0'*64, identity, definitions, buffers)
            changed = data.replace(b'consume', b'updated')
            header.write_bytes(changed)
            fresh, fresh_buffers = [], {}
            identity = hashlib.sha256(changed).hexdigest()
            self.assertEqual(definition_buffer(header, identity, identity, fresh, fresh_buffers), 0)
            self.assertEqual(fresh[0]['source'].encode(), changed)


if __name__ == '__main__':
    unittest.main()

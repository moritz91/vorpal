import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cpp_macro_compiler_provider import (ProofDeclined, ToolUnavailable, main,
                                       proven_projection, provider_exit, run_capture_command)


class ProviderFailure(unittest.TestCase):
    def exit(self, action):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            result = provider_exit(action)
        self.assertNotIn('private-secret', stderr.getvalue())
        return result

    def test_explicit_decline_is_distinct_from_failed_tools_or_provider(self):
        self.assertEqual(self.exit(lambda: None), 0)
        for error, code in [(ProofDeclined('private-secret'), 65),
                            (ToolUnavailable('private-secret'), 69),
                            (FileNotFoundError('private-secret'), 69),
                            (ImportError('private-secret'), 69),
                            (RuntimeError('private-secret'), 1),
                            (ValueError('private-secret'), 1),
                            (AssertionError('private-secret'), 1)]:
            def action():
                raise error
            self.assertEqual(self.exit(action), code)

    def test_missing_native_compiler_at_real_provider_entry_is_unavailable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'probe.cc'
            source.write_bytes(b'void probe() {}\n')
            request, plan, ready = root/'request.json', root/'plan.json', root/'ready'
            request.write_text(json.dumps({'version': 1, 'requestId': 'probe', 'path': str(source),
                                          'source': source.read_text()}))
            plan.write_text(json.dumps([{'source': str(source), 'cwd': directory,
                                         'args': ['/nologo', str(source), '/Zs']}]))
            ready.write_bytes(b'ready')
            with patch.dict('os.environ', {'VORPAL_CPP_COMPILER_READY': str(ready),
                                          'VORPAL_CPP_COMPILER_REQUEST': str(request)}), \
                 patch.object(sys, 'argv', ['provider', '--plan', str(plan), '--observer', 'private-secret',
                     '--resource-dir', 'private-secret', '--libclang', 'private-secret',
                     '--python-bindings', 'private-secret']), \
                 patch('cpp_macro_compiler_provider.shutil.which', return_value=None):
                self.assertEqual(self.exit(main), 69)

    def test_native_nonzero_exit_and_missing_program_do_not_claim_a_proof_decline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            out, err = root/'output', root/'stderr'
            action = lambda: run_capture_command([sys.executable, '-c',
                'import sys; sys.stderr.write("private-secret"); sys.exit(7)'], out, err, root)
            self.assertEqual(self.exit(action), 1)
            self.assertEqual(err.read_text(), 'private-secret')
            self.assertEqual(self.exit(lambda: run_capture_command(
                [str(root/'private-secret-missing')], out, err, root)), 69)

    def test_only_projection_refutation_can_explicitly_decline(self):
        def invalid():
            raise ValueError('private-secret')
        self.assertEqual(self.exit(lambda: proven_projection(invalid)), 65)
        def missing_input():
            raise FileNotFoundError('private-secret')
        self.assertEqual(self.exit(lambda: proven_projection(missing_input)), 69)


if __name__ == '__main__':
    unittest.main()

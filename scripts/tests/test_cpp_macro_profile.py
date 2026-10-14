import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class CaptureProfile(unittest.TestCase):
    def run_profile(self, root, action):
        request_dir = root / 'handshake'
        source_dir = root / 'source'
        tools_dir = root / 'tools'
        for directory in (request_dir, source_dir, tools_dir):
            directory.mkdir()
        source = source_dir / 'private_source.cc'
        source.write_text('private source payload', encoding='utf-8')
        request = request_dir / 'request.json'
        request.write_text(json.dumps({'requestId': 'capture-1', 'path': str(source)}))
        provider = tools_dir / 'fixture_provider.py'
        provider.write_text('import subprocess, sys\ndef main():\n' + action, encoding='utf-8')
        output = root / 'profiles'
        environment = dict(os.environ, VORPAL_CPP_COMPILER_REQUEST=str(request),
                           PRIVATE_PROFILE_SECRET='private environment payload')
        script = Path(__file__).resolve().parents[1] / 'profile_cpp_native_provider.py'
        result = subprocess.run([sys.executable, str(script), '--output-dir', str(output),
                                 str(provider), '--observer', str(tools_dir / 'unused-observer')],
                                env=environment, capture_output=True, timeout=30)
        return result, output, source

    def test_successful_capture_timeline_is_bounded_and_keeps_context_private(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result, output, source = self.run_profile(root,
                '    subprocess.run([sys.executable, "-c", "pass"], check=True)\n'
                '    subprocess.run([sys.executable, "-c", "pass"], check=True)\n')
            self.assertEqual(result.returncode, 0)
            self.assertEqual([p.name for p in output.iterdir()], ['capture-1.json'])
            encoded = (output / 'capture-1.json').read_bytes()
            report = json.loads(encoded)
            self.assertEqual(report['sourceContextSha256'], hashlib.sha256(str(source).encode()).hexdigest())
            start, end = report['startedPerfCounterNs'], report['finishedPerfCounterNs']
            self.assertGreater(end, start)
            self.assertGreaterEqual(report['pythonCpuSeconds'], 0)
            self.assertLessEqual(len(report['functions']), 90)
            self.assertEqual([r['phase'] for r in report['subprocesses']], ['nativeBefore', 'nativeAfter'])
            for process in report['subprocesses']:
                self.assertLessEqual(start, process['startedPerfCounterNs'])
                self.assertLessEqual(process['startedPerfCounterNs'], end)
                self.assertGreaterEqual(process['seconds'], 0)
            for private in (str(root), 'private source payload', 'private environment payload'):
                self.assertNotIn(private.encode(), encoded)

    def test_failed_capture_does_not_publish_a_timing_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            result, output, _ = self.run_profile(Path(temporary),
                '    raise RuntimeError("private failure payload")\n')
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stderr.strip(), b'native C++ capture failed')
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()

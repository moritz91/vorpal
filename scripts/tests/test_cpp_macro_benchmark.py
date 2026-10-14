import importlib.util
from pathlib import Path
import tempfile
import unittest


path = Path(__file__).resolve().parents[1] / 'benchmark_cpp_native_mcp.py'
spec = importlib.util.spec_from_file_location('native_benchmark', path)
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class WorkCounts(unittest.TestCase):
    def summarize(self, lines):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'phases.log'
            log.write_text(lines, encoding='utf-8')
            return benchmark.phase_summary(log)

    def test_distinguishes_migration_from_ordinary_replay(self):
        result = self.summarize(
            '[phase 10.0s] stream: start\n'
            '[phase 17.0s] stream: absorb tail\n'
            '[phase 17.1s] stream: products parsed=1744 replayed=0\n'
            '[phase 30.0s] stream: start\n'
            '[phase 34.0s] stream: absorb tail\n'
            '[phase 34.1s] stream: products parsed=27 replayed=1717\n')
        self.assertEqual(result['streamWorkCounts'], [
            {'parsed': 1744, 'replayed': 0}, {'parsed': 27, 'replayed': 1717}])
        self.assertEqual(result['extractionSeconds'], {'samples': 2, 'median': 5.5})

    def test_older_or_failed_traces_do_not_invent_work_counts(self):
        result = self.summarize(
            'private compiler arguments/environment: do not emit\n'
            '[phase 10.0s] stream: start\n'
            '[phase 11.0s] stream: products parsed=unknown replayed=12\n')
        self.assertEqual(result, {})


if __name__ == '__main__':
    unittest.main()

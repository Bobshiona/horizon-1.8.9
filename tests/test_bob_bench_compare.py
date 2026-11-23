"""Paired benchmark verdicts and fail-closed evidence loading."""
import importlib.util
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location("bob_bench_compare", Path(__file__).resolve().parents[1] / "tools/bob_bench_compare.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


def run(frame=1000, cpu=1000, fps=1000):
    return {p: {**dict.fromkeys(bench.METRICS, 1.0), "p50_us": frame,
                "cpu_p99_us": cpu, "avg_fps": fps, "frames": 100} for p in bench.PHASES}


class PairedBenchmark(unittest.TestCase):
    def test_average_fps_noise_does_not_override_stable_latency(self):
        pairs = [(run(), run(1001, 1005, fps=fps)) for fps in (300, 400, 2000)]
        self.assertEqual(bench.compare(pairs)[1], 0)

    def test_uses_median_of_paired_ratios_not_ratio_of_medians(self):
        pairs = [(run(base), run(base * ratio)) for base, ratio in
                 ((1000, 1.01), (2000, 1.01), (3000, .1))]
        lines, bad = bench.compare(pairs)
        self.assertEqual(bad, 0)
        self.assertTrue(any("lobby.p50_us" in l and "median=+1.00%" in l for l in lines))

    def test_checks_both_phases_and_returns_regressions(self):
        for metric, value in (("p50_us", 1051), ("cpu_p99_us", 1251)):
            pairs = [(run(), run()) for _ in range(3)]
            for _, candidate in pairs:
                candidate["flyover"][metric] = value
            self.assertEqual(bench.compare(pairs)[1], 1)
        self.assertEqual(bench.compare([(run(), run(1050, 1250))] * 3)[1], 0)

    def test_cli_failure_status_blocks_a_push_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, data in (("base", run()), ("new", run(frame=1060))):
                path = Path(directory) / name
                path.write_text("\n".join("BOBTRACE_RESULT phase=" + phase + " " +
                                " ".join(f"{k}={v}" for k, v in fields.items())
                                for phase, fields in data.items()))
                paths.append(str(path))
            command = [sys.executable, spec.origin, "--pair", *paths]
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 2)
            result = subprocess.run(command + ["--pair", *paths] * 2, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn("BENCH_REGRESSION=2", result.stdout)

    def test_invalid_or_incomplete_evidence_fails_closed(self):
        with self.assertRaises(ValueError):
            bench.compare([(run(), run())])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "run.log"
            for text in ("", "BOBTRACE_RESULT phase=lobby frames=0 p50_us=nan cpu_p99_us=1"):
                path.write_text(text)
                with self.assertRaises(ValueError):
                    bench.load(path)
            lines = ["BOBTRACE_RESULT phase=" + phase + " " + " ".join(f"{k}={v}" for k,v in data.items())
                     for phase,data in run().items()]
            path.write_text("\n".join(lines))
            self.assertEqual(bench.load(path), run())
            path.write_text("\n".join(lines).replace("allocs_per_frame=1.0", "allocs_per_frame=n/a"))
            loaded = bench.load(path)
            self.assertIn("allocs_per_frame=n/a", "\n".join(bench.compare([(loaded, loaded)] * 3)[0]))
            path.write_text("\n".join(lines + lines))
            with self.assertRaises(ValueError):
                bench.load(path)


if __name__ == "__main__":
    unittest.main()

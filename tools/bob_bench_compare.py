#!/usr/bin/env python3
"""Fail closed on paired trace regressions; average FPS remains diagnostic.

Pass --pair BASE.log NEW.log at least three times, in execution order. Run
pairs in alternating A/B, B/A order. Inspect flagged changes with bob-profile.sh.
Both phases gate median frame interval (+5%) and CPU submit p99 (+25%).
"""
from __future__ import annotations
import argparse
import math
from pathlib import Path
import statistics

PHASES = ("lobby", "flyover")
RULES = {"p50_us": 0.05, "cpu_p99_us": 0.25}
METRICS = ("avg_fps", "low1_fps", "p50_us", "p99_us", "cpu_p99_us", "gpu_p99_us",
           "draws_per_frame", "allocs_per_frame", "mesh_p99_ms")


def load(path: Path) -> dict[str, dict[str, float]]:
    phases = {}
    for line in path.read_text().splitlines():
        if not line.startswith("BOBTRACE_RESULT "):
            continue
        fields = dict(part.split("=", 1) for part in line.split()[1:] if "=" in part)
        phase = fields.pop("phase")
        if phase in phases:
            raise ValueError(f"duplicate phase {phase}")
        phases[phase] = {k: None if k == "allocs_per_frame" and v == "n/a" else float(v)
                         for k, v in fields.items()}
    for phase in PHASES:
        data = phases.get(phase, {})
        for key in (*METRICS, "frames"):
            value = data.get(key, float("nan"))
            if key == "allocs_per_frame" and key in data and value is None:
                continue
            if not math.isfinite(value) or value < 0:
                raise ValueError(f"missing or invalid {phase}.{key}")
        for key in (*RULES, "frames"):
            if data[key] <= 0:
                raise ValueError(f"zero {phase}.{key}")
    return phases


def compare(pairs: list[tuple[dict, dict]]) -> tuple[list[str], int]:
    if len(pairs) < 3:
        raise ValueError("at least three complete A/B pairs required")
    lines, bad = [], 0
    for phase in PHASES:
        for side, index in (("base", 0), ("new", 1)):
            def median(key):
                values = [p[index][phase][key] for p in pairs]
                return "n/a" if any(v is None for v in values) else f"{statistics.median(values):g}"
            values = " ".join(f"{key}={median(key)}" for key in METRICS)
            lines.append(f"BENCH_AB {side} {phase} {values}")
        for key, limit in RULES.items():
            ratios = [p[1][phase][key] / p[0][phase][key] for p in pairs]
            delta = statistics.median(ratios) - 1
            failed = delta > limit + 1e-12
            bad += failed
            shown = ",".join(f"{r - 1:+.2%}" for r in ratios)
            lines.append(f"BENCH_CHECK {phase}.{key} pairs={shown} median={delta:+.2%} "
                         f"limit=+{limit:.0%}" + (" REGRESSION" if failed else " PASS"))
    lines.append(f"BENCH_REGRESSION={bad}")
    return lines, bad


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pair", nargs=2, action="append", required=True, type=Path)
    args = parser.parse_args()
    try:
        lines, bad = compare([(load(a), load(b)) for a, b in args.pair])
    except (OSError, ValueError, KeyError) as error:
        print(f"BENCH_INVALID {error}")
        return 2
    print("\n".join(lines))
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Summarise an Instruments trace that tools/bob-profile.sh recorded.

    tools/bob-profile-summary.py TRACE.trace [--top N] [--from S] [--to S]

Time Profiler traces: the CPU time per thread, then each busy thread's
functions by self time (the function on top of the stack) and Bob Client's own functions by total
time (anywhere on the stack). Metal System Trace traces add Bob Client's GPU work by
channel and its command buffers by kind and label, with count, total, mean
and p99 in milliseconds. --from/--to keep samples inside a
window of the recording, in seconds, so startup can be left out.
"""
import argparse
import collections
import re
import subprocess
import sys
import xml.etree.ElementTree as ET


def export(trace, xpath=None):
    cmd = ["xcrun", "xctrace", "export", "--input", trace]
    cmd += ["--xpath", xpath] if xpath else ["--toc"]
    return subprocess.run(cmd, check=True, capture_output=True).stdout


def schemas(trace):
    toc = ET.fromstring(export(trace))
    return [t.get("schema") for t in toc.iter("table")]


def table(trace, schema):
    xpath = f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]'
    return ET.fromstring(export(trace, xpath))


class Refs:
    """xctrace writes each value once with an id and refers back to it."""

    def __init__(self):
        self.by_id = {}

    def resolve(self, element):
        ref = element.get("ref")
        if ref is not None:
            return self.by_id[ref]
        if element.get("id") is not None:
            self.by_id[element.get("id")] = element
        for child in element:
            self.resolve(child)
        return element


def demangle(name):
    """Rust's v0 symbols as their path (`crate::module::Type::method`), others unchanged.

    Enough to read a profile, not a full demangler: crate hashes, back
    references and disambiguators are dropped and generic arguments join the path.
    """
    if not name.startswith("_R"):
        return name
    parts, i = [], 2
    while i < len(name):
        c = name[i]
        if c.isdigit():
            j = i
            while j < len(name) and name[j].isdigit():
                j += 1
            n = int(name[i:j])
            if j < len(name) and name[j] == "_":
                j += 1
            if n:
                parts.append(name[j : j + n])
            i = j + n
        elif c in "sB":
            # A disambiguator (crate hash, impl number) or a back reference: up to its `_`.
            end = name.find("_", i)
            i = len(name) if end < 0 else end + 1
        else:
            i += 1
    path = "::".join(p for p in parts if p)
    return path or name


def thread_name(element):
    name = element.get("fmt", "?")
    return re.sub(r"\s*\(0x[0-9a-f]+\).*$", "", name)


def in_allocator(frame, refs):
    """The leaf frame is the allocator: the system heap or mimalloc (linked in)."""
    name = frame.get("name") or ""
    if name.startswith(("mi_", "_mi_", "__rust_alloc", "__rust_dealloc", "__rust_realloc", "__rdl_")):
        return True
    binary = frame.find("binary")
    if binary is None:
        return False
    return (refs.resolve(binary).get("name") or "") == "libsystem_malloc.dylib"


def time_profile(trace, top, start, end):
    root = table(trace, "time-profile")
    refs = Refs()
    per_thread = collections.Counter()
    self_time = collections.defaultdict(collections.Counter)
    total_time = collections.defaultdict(collections.Counter)
    allocator = collections.Counter()
    for row in root.iter("row"):
        cells = {child.tag: refs.resolve(child) for child in row}
        at = int(cells["sample-time"].text) / 1e9
        if at < start or (end is not None and at > end):
            continue
        state = cells.get("thread-state")
        if state is not None and state.get("fmt") != "Running":
            continue
        thread = thread_name(cells["thread"])
        weight = int(cells["weight"].text) / 1e6
        per_thread[thread] += weight
        stack = cells.get("tagged-backtrace")
        if stack is None:
            stack = cells.get("backtrace")
        if stack is None:
            continue
        resolved = [refs.resolve(f) for f in stack.iter("frame")]
        if resolved and in_allocator(resolved[0], refs):
            allocator[thread] += weight
        frames = [f.get("name") or f.get("addr") for f in resolved]
        frames = [demangle(f) for f in frames if f]
        if frames:
            self_time[thread][frames[0]] += weight
        for name in set(frames):
            total_time[thread][name] += weight
    cpu = sum(per_thread.values())
    print(f"PROFILE_CPU_MS total={cpu:.0f}")
    for thread, ms in per_thread.most_common(8):
        print(f"PROFILE_THREAD {ms:9.0f} ms {100 * ms / max(cpu, 1):5.1f}%  {thread}")
    # The allocator's share of each thread's CPU: what mimalloc against the system heap changes.
    total_alloc = sum(allocator.values())
    print(f"PROFILE_ALLOCATOR all {total_alloc:.0f} ms {100 * total_alloc / max(cpu, 1):.2f}%")
    for thread, ms in per_thread.most_common(3):
        print(f"PROFILE_ALLOCATOR {thread} {allocator[thread]:.0f} ms {100 * allocator[thread] / max(ms, 1):.2f}%")
    for thread, ms in per_thread.most_common(3):
        print(f"\n== {thread}: by self time")
        for name, w in self_time[thread].most_common(top):
            print(f"  {w:8.0f} ms {100 * w / ms:5.1f}%  {name[:110]}")
        print(f"== {thread}: Bob Client's own functions by total time")
        own = [(n, w) for n, w in total_time[thread].items() if n.startswith("horizon_")]
        for name, w in sorted(own, key=lambda nw: -nw[1])[:top]:
            print(f"  {w:8.0f} ms {100 * w / ms:5.1f}%  {name[:110]}")


def quantile(values, q):
    values = sorted(values)
    return values[min(len(values) - 1, int(q * len(values)))]


def rows(trace, schema, wanted):
    """Stream a table's rows as {column: (fmt, text)} for the wanted columns."""
    xpath = f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]'
    cmd = ["xcrun", "xctrace", "export", "--input", trace, "--xpath", xpath]
    seen = {}
    columns = []
    with subprocess.Popen(cmd, stdout=subprocess.PIPE) as export_process:
        for event, element in ET.iterparse(export_process.stdout, events=("end",)):
            if element.tag == "mnemonic":
                columns.append(element.text)
            elif element.tag == "row":
                out = {}
                for column, cell in zip(columns, element):
                    ref = cell.get("ref")
                    if ref is not None:
                        value = seen.get(ref, ("", ""))
                    else:
                        value = (cell.get("fmt") or "", cell.text or "")
                        for inner in cell.iter():
                            if inner.get("id") is not None:
                                seen[inner.get("id")] = (inner.get("fmt") or "", inner.text or "")
                    if column in wanted:
                        out[column] = value
                element.clear()
                yield out


def report(title, groups, top):
    print(f"\n== {title}")
    ranked = sorted(groups.items(), key=lambda kv: -sum(kv[1]))
    for label, durations in ranked[:top]:
        print(
            f"  n={len(durations):6d} total={sum(durations):9.1f} ms "
            f"mean={sum(durations) / len(durations):7.3f} p99={quantile(durations, 0.99):7.3f}  {label[:80]}"
        )


def metal(trace, top, start, end):
    def inside(row):
        at = int(row["start"][1] or 0) / 1e9
        return at >= start and (end is None or at <= end) and "horizon-189" in row["process"][0]

    gpu = collections.defaultdict(list)
    for row in rows(trace, "metal-gpu-intervals", {"start", "duration", "channel-name", "process"}):
        if inside(row):
            gpu[row["channel-name"][0]].append(int(row["duration"][1] or 0) / 1e6)
    report("GPU work by channel (Bob Client only)", gpu, top)
    cpu = collections.defaultdict(list)
    columns = {"start", "duration", "process", "event-label", "event-type"}
    for row in rows(trace, "metal-application-intervals", columns):
        if inside(row):
            label = re.sub(r" \(Frame [\d,]+\)", "", row["event-label"][0])
            label = re.split(r" \((?:Main Thread|horizon-189-client|[^()]*\(0x)", label)[0]
            cpu[f"{row['event-type'][0]}: {label}"].append(int(row["duration"][1] or 0) / 1e6)
    report("Metal command buffers on the CPU side, by kind and label", cpu, top)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("trace")
    parser.add_argument("--top", type=int, default=25)
    parser.add_argument("--from", dest="start", type=float, default=0.0)
    parser.add_argument("--to", dest="end", type=float, default=None)
    args = parser.parse_args()
    names = schemas(args.trace)
    if "time-profile" in names:
        time_profile(args.trace, args.top, args.start, args.end)
    if "metal-gpu-intervals" in names:
        metal(args.trace, args.top, args.start, args.end)
    return 0


if __name__ == "__main__":
    sys.exit(main())

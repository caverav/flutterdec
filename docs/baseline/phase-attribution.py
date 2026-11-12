#!/usr/bin/env python3
"""Attribute pipeline cost to phases and cases from the committed A/A artifacts.

Reads the four sample streams under docs/baseline/aa-1 and docs/baseline/aa-2
plus the case manifest, and prints the cost attribution published in
docs/research-ir-cfg-emitter.md section 12.

Usage:
    python3 docs/baseline/phase-attribution.py docs/baseline/aa-1 docs/baseline/aa-2

Reads only. Writes nothing but stdout.
"""

import collections
import csv
import json
import math
import os
import sys

PHASES = ("ir", "cfg", "emission_exclusive", "serialization")
SIDES = ("reference", "candidate")


def median(values):
    ordered = sorted(values)
    n = len(ordered)
    if n == 0:
        raise ValueError("median of empty sequence")
    mid = n // 2
    if n % 2 == 1:
        return float(ordered[mid])
    return (ordered[mid - 1] + ordered[mid]) / 2.0


def load_samples(path):
    """case -> phase -> list of (nanos, alloc_count, alloc_bytes)."""
    table = collections.defaultdict(lambda: collections.defaultdict(list))
    with open(path, newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            table[row["case"]][row["phase"]].append(
                (
                    int(row["nanos"]),
                    int(row["alloc_count"]),
                    int(row["alloc_bytes"]),
                )
            )
    return table


def medians(table):
    """case -> phase -> (nanos, alloc_count, alloc_bytes) medians."""
    out = {}
    for case, phases in table.items():
        out[case] = {}
        for phase, rows in phases.items():
            out[case][phase] = (
                median(r[0] for r in rows),
                median(r[1] for r in rows),
                median(r[2] for r in rows),
            )
    return out


def load_manifest(run_dir):
    with open(os.path.join(run_dir, "manifest-reference.json")) as handle:
        manifest = json.load(handle)
    return {case["case"]: case for case in manifest["cases"]}


def load_warmup(run_dir):
    with open(os.path.join(run_dir, "warmup-reference.json")) as handle:
        warmup = json.load(handle)
    return {case["case"]: case["correctness"] for case in warmup["cases"]}


def workload_shares(med):
    """Time-weighted phase share of the whole 33 case workload."""
    total = sum(med[case]["combined"][0] for case in med)
    shares = {}
    for phase in PHASES:
        shares[phase] = sum(med[case][phase][0] for case in med) / total
    return total, shares


def scaling_exponent(med, manifest, topology, load, phase):
    """log ratio of the 1024 block cost to the 64 block cost, base 16."""
    small = None
    large = None
    for case, meta in manifest.items():
        if meta["topology"] != topology or meta["load"] != load:
            continue
        if meta["blocks"] == 64:
            small = med[case][phase][0]
        elif meta["blocks"] == 1024:
            large = med[case][phase][0]
    if small is None or large is None or small <= 0:
        return None
    return math.log(large / small) / math.log(16.0)


def main(argv):
    if len(argv) != 3:
        sys.stderr.write(
            "usage: phase-attribution.py <run-dir-1> <run-dir-2>\n"
        )
        return 2
    run_dirs = argv[1:]
    manifest = load_manifest(run_dirs[0])
    warmup = load_warmup(run_dirs[0])

    per_binary = {}
    for run_dir in run_dirs:
        for side in SIDES:
            path = os.path.join(run_dir, "samples-%s.tsv" % side)
            label = "%s/%s" % (os.path.basename(run_dir.rstrip("/")), side)
            per_binary[label] = medians(load_samples(path))
    labels = sorted(per_binary)

    print("== 1. phase share of total workload time, per binary ==")
    print("binary\ttotal_ms\t" + "\t".join(PHASES) + "\taccounted")
    pooled = collections.defaultdict(list)
    for label in labels:
        total, shares = workload_shares(per_binary[label])
        accounted = sum(shares.values())
        row = ["%.5f" % shares[p] for p in PHASES]
        for phase in PHASES:
            pooled[phase].append(shares[phase])
        print(
            "%s\t%.1f\t%s\t%.5f" % (label, total / 1e6, "\t".join(row), accounted)
        )
    print("mean\t\t" + "\t".join("%.5f" % (sum(pooled[p]) / 4.0) for p in PHASES))
    print()

    ref = per_binary["aa-1/reference"]

    print("== 2. per case share of total workload time, descending ==")
    total = sum(ref[c]["combined"][0] for c in ref)
    ranked = sorted(ref, key=lambda c: -ref[c]["combined"][0])
    cumulative = 0.0
    print("case\tblocks\tinstr\tcombined_ms\tshare\tcumulative\temission_share")
    for case in ranked:
        share = ref[case]["combined"][0] / total
        cumulative += share
        print(
            "%s\t%d\t%d\t%.3f\t%.5f\t%.5f\t%.5f"
            % (
                case,
                manifest[case]["blocks"],
                manifest[case]["instructions"],
                ref[case]["combined"][0] / 1e6,
                share,
                cumulative,
                ref[case]["emission_exclusive"][0] / ref[case]["combined"][0],
            )
        )
    print()

    print("== 2b. topology rollup (reference, aa-1) ==")
    family_time = collections.defaultdict(float)
    family_emission = collections.defaultdict(float)
    family_allocs = collections.defaultdict(float)
    for case in ref:
        topology = manifest[case]["topology"]
        family_time[topology] += ref[case]["combined"][0]
        family_emission[topology] += ref[case]["emission_exclusive"][0]
        family_allocs[topology] += ref[case]["emission_exclusive"][1]
    emission_total = sum(family_emission.values())
    alloc_total = sum(family_allocs.values())
    print("topology\tcombined_ms\tshare_of_total\tshare_of_emission\tshare_of_allocs")
    for topology in sorted(family_time, key=lambda t: -family_time[t]):
        print(
            "%s\t%.1f\t%.5f\t%.5f\t%.5f"
            % (
                topology,
                family_time[topology] / 1e6,
                family_time[topology] / total,
                family_emission[topology] / emission_total,
                family_allocs[topology] / alloc_total,
            )
        )
    declined = ("irreducible", "multi-exit", "fan-in")
    print(
        "declined-structuring group (%s)\t%.1f\t%.5f\t%.5f\t%.5f"
        % (
            ", ".join(declined),
            sum(family_time[t] for t in declined) / 1e6,
            sum(family_time[t] for t in declined) / total,
            sum(family_emission[t] for t in declined) / emission_total,
            sum(family_allocs[t] for t in declined) / alloc_total,
        )
    )
    print()

    print("== 3. per case phase share spread (reference, aa-1) ==")
    print("phase\tmin_case\tmin\tmax_case\tmax")
    for phase in PHASES:
        pairs = [
            (ref[c][phase][0] / ref[c]["combined"][0], c) for c in ref
        ]
        lo = min(pairs)
        hi = max(pairs)
        print("%s\t%s\t%.5f\t%s\t%.5f" % (phase, lo[1], lo[0], hi[1], hi[0]))
    print()

    print("== 4. size scaling, log ratio 1024/64 base 16 (1.0 is linear) ==")
    print("topology\tload\t" + "\t".join(PHASES) + "\tcombined")
    seen = []
    for meta in manifest.values():
        key = (meta["topology"], meta["load"])
        if key not in seen:
            seen.append(key)
    for topology, load in sorted(seen):
        cells = []
        for phase in list(PHASES) + ["combined"]:
            exponent = scaling_exponent(ref, manifest, topology, load, phase)
            cells.append("n/a" if exponent is None else "%.3f" % exponent)
        print("%s\t%s\t%s" % (topology, load, "\t".join(cells)))
    print()

    print("== 5. emission cost normalised by shape (reference, aa-1) ==")
    print("case\tblocks\tinstr\tns_per_block\tns_per_instr\thelper_refs\tlines")
    for case in sorted(ref, key=lambda c: (manifest[c]["blocks"], c)):
        meta = manifest[case]
        emission = ref[case]["emission_exclusive"][0]
        print(
            "%s\t%d\t%d\t%.1f\t%.1f\t%d\t%d"
            % (
                case,
                meta["blocks"],
                meta["instructions"],
                emission / meta["blocks"],
                emission / meta["instructions"],
                warmup[case]["helper_references"],
                warmup[case]["source_lines"],
            )
        )
    print()

    print("== 6. allocation attribution, summed over 33 cases (reference, aa-1) ==")
    print("phase\talloc_count\talloc_bytes\tcount_share\tbytes_share")
    total_count = sum(ref[c]["combined"][1] for c in ref)
    total_bytes = sum(ref[c]["combined"][2] for c in ref)
    for phase in PHASES:
        count = sum(ref[c][phase][1] for c in ref)
        size = sum(ref[c][phase][2] for c in ref)
        print(
            "%s\t%d\t%d\t%.5f\t%.5f"
            % (phase, count, size, count / total_count, size / total_bytes)
        )
    print("combined\t%d\t%d\t1.00000\t1.00000" % (total_count, total_bytes))
    print()

    print("== 7. emission cost per allocation (reference, aa-1) ==")
    print("case\temit_ms\talloc_count\tns_per_alloc\tbytes_per_alloc")
    rates = []
    for case in sorted(
        ref, key=lambda c: ref[c]["emission_exclusive"][0] / ref[c]["emission_exclusive"][1]
    ):
        nanos, count, size = ref[case]["emission_exclusive"]
        rates.append(nanos / count)
        print(
            "%s\t%.3f\t%d\t%.1f\t%.1f"
            % (case, nanos / 1e6, count, nanos / count, size / count)
        )
    xs = [ref[c]["emission_exclusive"][1] for c in ref]
    ys = [ref[c]["emission_exclusive"][0] for c in ref]
    n = float(len(xs))
    mx = sum(xs) / n
    my = sum(ys) / n
    cov = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    vx = sum((x - mx) ** 2 for x in xs)
    vy = sum((y - my) ** 2 for y in ys)
    print(
        "ns_per_alloc min %.1f max %.1f median %.1f; pearson_r %.6f; "
        "fit_through_origin %.2f ns"
        % (
            min(rates),
            max(rates),
            median(rates),
            cov / math.sqrt(vx * vy),
            sum(x * y for x, y in zip(xs, ys)) / sum(x * x for x in xs),
        )
    )
    print()

    print("== 8. Amdahl round ceiling, whole workload ==")
    _, shares = workload_shares(ref)
    print("target\tshare\t-10pct\t-25pct\t-50pct\tremoved")
    for phase in PHASES:
        share = shares[phase]
        print(
            "%s\t%.5f\t%.5f\t%.5f\t%.5f\t%.5f"
            % (phase, share, share * 0.10, share * 0.25, share * 0.50, share)
        )
    pair = shares["ir"] + shares["cfg"]
    print(
        "ir+cfg\t%.5f\t%.5f\t%.5f\t%.5f\t%.5f"
        % (pair, pair * 0.10, pair * 0.25, pair * 0.50, pair)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

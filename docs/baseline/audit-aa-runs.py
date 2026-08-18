#!/usr/bin/env python3
"""Independent audit of two A/A runs of scripts/bench-pipeline.sh.

Recomputes every published statistic from the raw sample streams, restates the
alternating schedule from scratch, and exits 1 on any reject condition: run
overlap, stale or unpaired samples, binary digest mismatch, workload drift,
correctness failure, a cell clearing its own MDE, or a systematic order or
build-layout bias.

    python3 docs/baseline/audit-aa-runs.py AA1_DIR AA2_DIR [PER_CASE_SUMMARY_TSV]

Each directory is either a live bench-pipeline output directory or a committed
`docs/baseline/aa-N`; the two layouts differ only in where `pair-order.tsv`
lives, which is handled below.
"""
import glob
import hashlib
import json
import os
import statistics
import sys

if len(sys.argv) not in (3, 4):
    print(__doc__, file=sys.stderr)
    sys.exit(2)
RUNS = [os.path.abspath(p) for p in sys.argv[1:3]]
SUMMARY_OUT = sys.argv[3] if len(sys.argv) == 4 else None
PAIRS = 15
PHASES = ["ir", "cfg", "emission_exclusive", "serialization", "combined"]
problems = []


def median(xs):
    return statistics.median(xs)


def mad(xs):
    m = median(xs)
    return median([abs(x - m) for x in xs])


def mde(noise):
    return max(0.05, 3.0 * noise)


def load_samples(path):
    """(run, case, phase) -> nanos"""
    out = {}
    with open(path) as fh:
        header = fh.readline().rstrip("\n").split("\t")
        assert header == ["run", "case", "phase", "nanos", "alloc_count", "alloc_bytes"], header
        for line in fh:
            r, case, phase, nanos, _ac, _ab = line.rstrip("\n").split("\t")
            key = (int(r), case, phase)
            if key in out:
                problems.append(f"duplicate sample {key} in {path}")
            out[key] = float(nanos)
    return out


def read_order(path):
    order = []
    with open(path) as fh:
        for line in fh:
            p, pos, side = line.rstrip("\n").split("\t")
            order.append((int(p), pos, side))
    return order


def expected_order():
    exp = []
    for p in range(PAIRS):
        if p % 2 == 0:
            exp += [(p, "first", "reference"), (p, "second", "candidate")]
        else:
            exp += [(p, "first", "candidate"), (p, "second", "reference")]
    return exp


def order_path(d):
    """Live runs write raw/pair-order.tsv; the committed copy flattens it."""
    live = os.path.join(d, "raw", "pair-order.tsv")
    return live if os.path.exists(live) else os.path.join(d, "pair-order.tsv")


runs = {}
for d in RUNS:
    name = os.path.basename(d)
    bind = dict(
        line.split(None, 1)[0:2] if len(line.split(None, 1)) == 2 else (line.strip(), "")
        for line in open(f"{d}/binding.txt").read().splitlines()
        if line.strip()
    )
    bind = {k: v.strip() for k, v in bind.items()}

    # --- reject: digest mismatch between the two sides
    if bind["reference_binary_sha256"] != bind["candidate_binary_sha256"]:
        problems.append(f"{name}: binary digest mismatch between sides")
    if bind["reference_product_ref"] != bind["candidate_product_ref"]:
        problems.append(f"{name}: not an A/A run")

    # --- reject: schedule not alternating (restated independently here)
    order = read_order(order_path(d))
    if order != expected_order():
        problems.append(f"{name}: pair order log is not the alternating schedule")
    if len(order) != 2 * PAIRS:
        problems.append(f"{name}: expected {2*PAIRS} order rows, got {len(order)}")

    pos_of = {(p, side): pos for p, pos, side in order}

    ref = load_samples(f"{d}/samples-reference.tsv")
    cand = load_samples(f"{d}/samples-candidate.tsv")

    # --- reject: stale samples / unpaired samples
    if set(ref) != set(cand):
        problems.append(f"{name}: reference and candidate sample keys differ")
    cases = sorted({c for (_r, c, _p) in ref})
    for phase in PHASES:
        for case in cases:
            for p in range(PAIRS):
                for label, tbl in (("reference", ref), ("candidate", cand)):
                    if (p, case, phase) not in tbl:
                        problems.append(f"{name}: missing {label} sample pair={p} {case} {phase}")

    # --- reject: runs overlapping in time. Only meaningful against a live output
    # directory: a committed copy carries checkout mtimes, not measurement ones,
    # so the check is skipped rather than faked, and the skip is printed.
    mtimes = [os.path.getmtime(f) for f in glob.glob(f"{d}/raw/*.tsv")]
    win = (min(mtimes), max(mtimes)) if mtimes else None

    warm = {
        side: json.load(open(f"{d}/warmup-{side}.json")) for side in ("reference", "candidate")
    }
    for side, w in warm.items():
        if w["correctness_failures"]:
            problems.append(f"{name}/{side}: correctness failures {w['correctness_failures']}")
        if len(w["cases"]) != 33:
            problems.append(f"{name}/{side}: {len(w['cases'])} cases, expected 33")
        if not w["limits"]["within_memory_limit"]:
            problems.append(f"{name}/{side}: exceeded memory limit")
        if w["limits"]["runs_over_timeout"]:
            problems.append(f"{name}/{side}: runs over timeout")
        if w["timer"]["reconciliation_failures"]:
            problems.append(f"{name}/{side}: reconciliation failures")
        if w["binding"]["warmups"] != 3:
            problems.append(f"{name}/{side}: warmups={w['binding']['warmups']}, expected 3")
        if w["binding"]["measured_runs"] != 0:
            problems.append(f"{name}/{side}: warmup invocation measured runs")

    # Every measured raw document: 0 warmups, 1 run, right binding, and a label
    # carrying the pair index and position it actually ran in. Present only in a
    # live output directory; the committed copy keeps the aggregated sample
    # streams instead, so the check is skipped and the skip is printed.
    raw_docs = os.path.exists(f"{d}/raw/reference-0.json")
    for side in ("reference", "candidate") if raw_docs else ():
        for p in range(PAIRS):
            j = json.load(open(f"{d}/raw/{side}-{p}.json"))
            b = j["binding"]
            if b["warmups"] != 0 or b["measured_runs"] != 1:
                problems.append(f"{name}/{side}-{p}: warmups={b['warmups']} runs={b['measured_runs']}")
            if b["binary_sha256"] != bind[f"{side}_binary_sha256"]:
                problems.append(f"{name}/{side}-{p}: binary digest does not match binding")
            if b["patch_sha256"] != bind["patch_sha256"]:
                problems.append(f"{name}/{side}-{p}: patch digest does not match binding")
            if b["harness_ref"] != bind["harness_ref"]:
                problems.append(f"{name}/{side}-{p}: harness ref does not match binding")
            if f"{p} {pos_of[(p, side)]}" not in b["label"]:
                problems.append(f"{name}/{side}-{p}: label does not carry pair/position")

    if not raw_docs:
        print(f"[skip] {name}: no raw/ per-pair documents, per-document binding check skipped")

    runs[name] = dict(
        dir=d, binding=bind, order=order, pos_of=pos_of, ref=ref, cand=cand,
        cases=cases, window=win, warm=warm,
    )

names = [os.path.basename(d) for d in RUNS]
first, second = runs[names[0]], runs[names[1]]

# --- reject: the two runs overlap in time
print("== windows (raw sample mtimes, epoch seconds)")
if first["window"] and second["window"]:
    w1, w2 = first["window"], second["window"]
    if not w1[1] < w2[0]:
        problems.append("runs overlap: run 1's newest raw sample is not older than run 2's oldest")
    print(f"{names[0]} {w1[0]:.1f} .. {w1[1]:.1f}")
    print(f"{names[1]} {w2[0]:.1f} .. {w2[1]:.1f}")
    print(f"gap between runs: {w2[0] - w1[1]:.1f}s")
else:
    print("[skip] no raw/ sample mtimes, overlap check skipped (committed copy)")

# --- per-cell and per-phase stats, plus per-case summary rows
summary_rows = []
cell = {}
for name, R in runs.items():
    for phase in PHASES:
        for case in R["cases"]:
            deltas, refs, cands = [], [], []
            for p in range(PAIRS):
                r = R["ref"][(p, case, phase)]
                c = R["cand"][(p, case, phase)]
                refs.append(r)
                cands.append(c)
                deltas.append((c - r) / r)
            d, n = median(deltas), mad(deltas)
            cell[(name, case, phase)] = d
            summary_rows.append(
                (name, case, phase, PAIRS, median(refs), mad(refs), median(cands), mad(cands),
                 d, n, mde(n), "yes" if abs(d) >= mde(n) else "no")
            )

print("\n== per-phase pooled (recomputed independently)")
phase_pooled = {}
for name, R in runs.items():
    for phase in PHASES:
        deltas = []
        for case in R["cases"]:
            for p in range(PAIRS):
                r = R["ref"][(p, case, phase)]
                c = R["cand"][(p, case, phase)]
                deltas.append((c - r) / r)
        d, n = median(deltas), mad(deltas)
        phase_pooled[(name, phase)] = (d, n, mde(n))
        print(f"{name} {phase:<20} d={d:+.5f} noise={n:.5f} mde={mde(n):.4f} "
              f"clears={'YES' if abs(d)>=mde(n) else 'no'} pairs={len(deltas)}")

# cross-check against the harness aggregator
print("\n== cross-check vs analysis.json by_phase (tolerance 1e-4)")
for name, R in runs.items():
    a = json.load(open(f"{R['dir']}/analysis.json"))
    for row in a["by_phase"]:
        mine = phase_pooled[(name, row["phase"])]
        if abs(mine[0] - row["median_paired_delta"]) > 1e-4 or abs(mine[1] - row["noise_mad_of_deltas"]) > 1e-4:
            problems.append(f"{name} {row['phase']}: my stats disagree with aggregator")
        print(f"{name} {row['phase']:<20} aggregator d={row['median_paired_delta']:+.5f} mine d={mine[0]:+.5f} OK")

# --- reject: systematic order bias
print("\n== order bias: within one side, (second - first) / first on combined")
order_bias = {}
for name, R in runs.items():
    for side, tbl in (("reference", R["ref"]), ("candidate", R["cand"])):
        firsts = [p for p in range(PAIRS) if R["pos_of"][(p, side)] == "first"]
        seconds = [p for p in range(PAIRS) if R["pos_of"][(p, side)] == "second"]
        rel = []
        for case in R["cases"]:
            f = median([tbl[(p, case, "combined")] for p in firsts])
            s = median([tbl[(p, case, "combined")] for p in seconds])
            rel.append((s - f) / f)
        order_bias[(name, side)] = (median(rel), mad(rel), len(firsts), len(seconds))
        print(f"{name} {side:<10} (second-first)/first={median(rel):+.5f} MAD={mad(rel):.5f} "
              f"firsts={len(firsts)} seconds={len(seconds)} cases={len(rel)}")

print("\n== order bias: candidate vs reference holding position fixed")
held = {}
for name, R in runs.items():
    for phase in ("emission_exclusive", "combined"):
        row = {}
        for slot in ("first", "second"):
            rel = []
            for case in R["cases"]:
                rp = [p for p in range(PAIRS) if R["pos_of"][(p, "reference")] == slot]
                cp = [p for p in range(PAIRS) if R["pos_of"][(p, "candidate")] == slot]
                r = median([R["ref"][(p, case, phase)] for p in rp])
                c = median([R["cand"][(p, case, phase)] for p in cp])
                rel.append((c - r) / r)
            row[slot] = median(rel)
        held[(name, phase)] = row
        print(f"{name} {phase:<20} both-first={row['first']:+.5f} both-second={row['second']:+.5f}")

# Reject a systematic order bias: an A/A run whose position effect is large
# enough to move a phase median more than the 5 percent floor, or whose sign
# split is one-sided beyond chance.
for (name, side), (d, m, nf, ns) in order_bias.items():
    if abs(d) >= 0.05:
        problems.append(f"{name}/{side}: position effect {d:+.5f} reaches the 5 percent floor")
for (name, phase), row in held.items():
    if abs(row["first"]) >= 0.05 or abs(row["second"]) >= 0.05:
        problems.append(f"{name} {phase}: held-position A/A skew reaches the 5 percent floor")
    # a build-layout bias shows up as the SAME sign and magnitude in both slots
    if (row["first"] > 0.01 and row["second"] > 0.01) or (row["first"] < -0.01 and row["second"] < -0.01):
        problems.append(
            f"{name} {phase}: A/A skew of the same sign in both slots "
            f"({row['first']:+.5f}, {row['second']:+.5f}) indicates a build-layout bias, not noise"
        )

print("\n== sign balance and cells clearing MDE")
for name, R in runs.items():
    cells = [(c, p, cell[(name, c, p)]) for p in PHASES for c in R["cases"]]
    slower = sum(1 for _c, _p, d in cells if d > 0)
    big = [(c, p, d) for c, p, d in cells if abs(d) >= 0.02]
    clearing = [r for r in summary_rows if r[0] == name and r[11] == "yes"]
    mx = max(cells, key=lambda t: abs(t[2]))
    print(f"{name}: cells={len(cells)} candidate-slower={slower} faster={len(cells)-slower} "
          f"|delta|>=2%={len(big)} (slower {sum(1 for _c,_p,d in big if d>0)}) "
          f"clearing-own-MDE={len(clearing)} largest|delta|={mx[2]:+.5f} at {mx[0]}/{mx[1]}")
    if clearing:
        problems.append(f"{name}: {len(clearing)} cell(s) clear their own MDE in an A/A run")

# --- run to run
print(f"\n== run to run, same cells ({names[0]} vs {names[1]})")
shifts = [cell[(names[1], c, p)] - cell[(names[0], c, p)]
          for p in PHASES for c in first["cases"]]
print(f"cells={len(shifts)} median shift={median(shifts):+.5f} MAD={mad(shifts):.5f} "
      f"mde={mde(mad(shifts)):.4f}")

# --- workload digests
print("\n== workload digests")
mats = set()
per_case = {}
for name, R in runs.items():
    for side, w in R["warm"].items():
        mats.add(w["binding"]["matrix_sha256"])
        for c in w["cases"]:
            per_case.setdefault(c["case"], set()).add(c["workload_sha256"])
print(f"distinct matrix_sha256 across all four binaries: {len(mats)} -> {sorted(mats)}")
if len(mats) != 1:
    problems.append("matrix digest differs between binaries")
multi = {k: v for k, v in per_case.items() if len(v) != 1}
print(f"cases with a non-unique workload digest: {len(multi)}")
if multi:
    problems.append(f"workload digest differs between binaries for {sorted(multi)}")
manifests = set()
for name, R in runs.items():
    h = hashlib.sha256(open(f"{R['dir']}/manifest-reference.json", "rb").read()).hexdigest()
    manifests.add(h)
    # The pipeline already refuses to measure when the two sides' manifests
    # differ, so the committed copy keeps only the reference one. When both are
    # present, check them here too.
    cand_manifest = f"{R['dir']}/manifest-candidate.json"
    if os.path.exists(cand_manifest):
        if hashlib.sha256(open(cand_manifest, "rb").read()).hexdigest() != h:
            problems.append(f"{name}: manifest digests differ between sides")
    else:
        print(f"[skip] {name}: no manifest-candidate.json, side-vs-side manifest check skipped")
print(f"distinct manifest sha256 across both runs: {len(manifests)} -> {sorted(manifests)}")
if len(manifests) != 1:
    problems.append("manifest digest differs between runs")

if SUMMARY_OUT:
    with open(SUMMARY_OUT, "w") as fh:
        fh.write("run\tcase\tphase\tpairs\treference_median_nanos\treference_mad_nanos\t"
                 "candidate_median_nanos\tcandidate_mad_nanos\tmedian_paired_delta\t"
                 "noise_mad_of_deltas\tmde\tclears_mde\n")
        for r in sorted(summary_rows):
            fh.write("\t".join([r[0], r[1], r[2], str(r[3]),
                                f"{r[4]:.1f}", f"{r[5]:.1f}", f"{r[6]:.1f}", f"{r[7]:.1f}",
                                f"{r[8]:+.6f}", f"{r[9]:.6f}", f"{r[10]:.6f}", r[11]]) + "\n")
    print(f"\nwrote {SUMMARY_OUT} ({len(summary_rows)} rows)")

print("\n== VERDICT")
if problems:
    for p in problems:
        print("REJECT:", p)
    sys.exit(1)
print("no reject condition triggered")

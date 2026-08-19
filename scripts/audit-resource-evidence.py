#!/usr/bin/env python3
"""Fail closed on resource evidence, 5 percent guards, and phase plants."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path

PHASES = {"ir", "cfg", "emission_exclusive", "serialization", "combined"}
METRICS = ("allocation_count", "total_allocated_bytes", "peak_live_bytes")


def read(path: Path):
    rows = {}
    with path.open(newline="", encoding="ascii") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            key = (row["case"], row["phase"])
            if key in rows:
                raise ValueError(f"{path}: duplicate {key}")
            rows[key] = {name: int(row[name]) for name in METRICS}
            rss = int(row["process_peak_rss_bytes"])
            if rss <= 0:
                raise ValueError(f"{path}: non-positive RSS for {key}")
    cases = {case for case, _ in rows}
    if len(cases) != 33 or len(rows) != 165:
        raise ValueError(f"{path}: expected 33 cases and 165 rows, got {len(cases)} and {len(rows)}")
    for case in cases:
        if {phase for row_case, phase in rows if row_case == case} != PHASES:
            raise ValueError(f"{path}: incomplete phases for {case}")
    if any(value <= 0 for metrics in rows.values() for value in metrics.values()):
        raise ValueError(f"{path}: zero resource cell")
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--noop", type=Path, required=True)
    parser.add_argument("--cfg-plant", type=Path, required=True)
    parser.add_argument("--emitter-plant", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    reference = read(args.reference)
    candidate = read(args.candidate)
    noop = read(args.noop)
    cfg = read(args.cfg_plant)
    emitter = read(args.emitter_plant)
    if reference.keys() != candidate.keys() or candidate != noop:
        raise ValueError("reference/candidate keys differ or no-op lifecycle control drifted")

    regressions = []
    max_delta = {metric: float("-inf") for metric in METRICS}
    for key, cand in candidate.items():
        for metric in METRICS:
            delta = (cand[metric] - reference[key][metric]) / reference[key][metric]
            max_delta[metric] = max(max_delta[metric], delta)
            if delta > 0.05:
                regressions.append((key, metric, delta))
    if regressions:
        raise ValueError(f"candidate has {len(regressions)} resource regressions above 5 percent: {regressions[:5]}")

    def plant_check(planted, own_phase):
        over = []
        for key, control in candidate.items():
            if key[1] not in {own_phase, "combined"} and planted[key] != control:
                raise ValueError(f"{own_phase} plant changed wrong phase {key}")
            if key[1] == own_phase:
                delta = (planted[key]["peak_live_bytes"] - control["peak_live_bytes"]) / control["peak_live_bytes"]
                if delta > 0.05:
                    over.append((key[0], delta))
                if planted[key]["allocation_count"] <= control["allocation_count"]:
                    raise ValueError(f"{own_phase} plant did not add an allocation for {key[0]}")
                if planted[key]["total_allocated_bytes"] <= control["total_allocated_bytes"]:
                    raise ValueError(f"{own_phase} plant did not add bytes for {key[0]}")
        if not over:
            raise ValueError(f"{own_phase} plant never exceeded the 5 percent peak guard")
        return max(delta for _, delta in over), len(over)

    cfg_max, cfg_cells = plant_check(cfg, "cfg")
    emitter_max, emitter_cells = plant_check(emitter, "emission_exclusive")
    result = {
        "schema": "flutterdec-resource-audit/1",
        "case_count": 33,
        "row_count_per_binding": 165,
        "candidate_guard": "passed",
        "max_candidate_delta": max_delta,
        "noop_control": "passed",
        "cfg_plant": {"max_peak_delta": cfg_max, "cells_over_5_percent": cfg_cells},
        "emitter_plant": {"max_peak_delta": emitter_max, "cells_over_5_percent": emitter_cells},
        "phase_misattribution": "none",
    }
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="ascii")
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

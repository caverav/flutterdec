#!/usr/bin/env python3
"""Fail-closed audit and final score for frozen timing/resource comparisons."""

from __future__ import annotations

import csv
import hashlib
import json
import statistics
import sys
from pathlib import Path

REFERENCE = "630ec442d951aac5704ae80287367912bfbfc388"
CANDIDATE = "9b82e07fa62f97654aea5153d9fb6a2ef57a377a"
TIMING_HARNESS = "4c127aba4e74fb6f8d486c4cb066586bb0d74846"
RESOURCE_HARNESS = "b0e615785b28e7e58aa06dd1b929dd58acf06e53"
PHASES = ("ir", "cfg", "emission_exclusive", "serialization", "combined")
DISCLOSED_TARGETS = (
    "irreducible/1024/base",
    "irreducible/256/base",
    "irreducible/64/base",
    "multi-exit/1024/base",
)
RESOURCE_METRICS = ("allocation_count", "total_allocated_bytes", "peak_live_bytes")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def median_mad(values: list[float]) -> tuple[float, float]:
    med = statistics.median(values)
    return med, statistics.median(abs(value - med) for value in values)


def timing_rows(path: Path, expected_cases: int):
    rows = list(csv.DictReader(path.open(newline="", encoding="ascii"), delimiter="\t"))
    assert len(rows) == expected_cases * len(PHASES) * 15
    result: dict[tuple[str, str], dict[int, int]] = {}
    for row in rows:
        key = (row["case"], row["phase"])
        result.setdefault(key, {})[int(row["run"])] = int(row["nanos"])
    assert all(len(values) == 15 for values in result.values())
    return result


def resource_rows(path: Path, expected_cases: int):
    rows = list(csv.DictReader(path.open(newline="", encoding="ascii"), delimiter="\t"))
    assert len(rows) == expected_cases * len(PHASES)
    result = {}
    for row in rows:
        key = (row["case"], row["phase"])
        assert key not in result
        result[key] = {metric: int(row[metric]) for metric in RESOURCE_METRICS}
        result[key]["process_peak_rss_bytes"] = int(row["process_peak_rss_bytes"])
        assert all(value > 0 for value in result[key].values())
    return result


def paired(ref: list[int], cand: list[int]):
    deltas = [(c - r) / r for r, c in zip(ref, cand)]
    estimate, noise = median_mad(deltas)
    return {
        "reference_scores": ref,
        "candidate_scores": cand,
        "estimate": estimate,
        "noise_mad": noise,
        "mde": max(0.05, 3 * noise),
    }


def audit_matrix(root: Path, matrix: str):
    timing = root / matrix / "timing"
    resource = root / matrix / "resource"
    manifests = [timing / f"manifest-{side}.json" for side in ("reference", "candidate")]
    assert manifests[0].read_bytes() == manifests[1].read_bytes()
    manifest = json.loads(manifests[0].read_text(encoding="ascii"))
    expected_cases = 33 if matrix == "disclosed" else 6
    assert manifest["matrix"] == matrix and manifest["case_count"] == expected_cases
    cases = [row["case"] for row in manifest["cases"]]
    assert len(set(cases)) == expected_cases
    workloads = {row["case"]: row["workload_sha256"] for row in manifest["cases"]}
    assert len(set(workloads.values())) == expected_cases
    if matrix == "held-out":
        assert manifest["held_out_seed_hex"]
        assert all(row["instructions_per_block"] == 8 for row in manifest["cases"])
        assert all(96 <= row["blocks"] <= 2048 for row in manifest["cases"])
        assert not ({row["blocks"] for row in manifest["cases"]} & {64, 256, 1024})

    warmups = {}
    max_rss = 0
    for side, product in (("reference", REFERENCE), ("candidate", CANDIDATE)):
        document = json.loads((timing / f"warmup-{side}.json").read_text(encoding="ascii"))
        binding = document["binding"]
        assert binding["product_ref"] == product and binding["harness_ref"] == TIMING_HARNESS
        assert binding["matrix"] == matrix and binding["matrix_sha256"] == manifest["matrix_sha256"]
        assert binding["warmups"] == 3 and binding["measured_runs"] == 0
        assert binding["correctness_checked"] and not document["correctness_failures"]
        assert document["limits"]["within_memory_limit"] and not document["limits"]["runs_over_timeout"]
        max_rss = max(max_rss, document["limits"]["peak_rss_bytes"])
        assert len(document["cases"]) == expected_cases
        assert all(row["correctness"]["passed"] for row in document["cases"])
        warmups[side] = {row["case"]: row["correctness"]["artifact_sha256"] for row in document["cases"]}
    assert warmups["reference"] == warmups["candidate"]

    expected_order = [
        f"{pair}\t{position}\t{side}"
        for pair in range(15)
        for position, side in zip(
            ("first", "second"),
            (("reference", "candidate") if pair % 2 == 0 else ("candidate", "reference")),
        )
    ]
    assert (timing / "pair-order.tsv").read_text().splitlines() == expected_order
    assert (timing / "planned-pair-order.tsv").read_text().splitlines() == expected_order

    worst_residue = 0.0
    if (timing / "raw").is_dir():
        for pair in range(15):
            sides = ("reference", "candidate") if pair % 2 == 0 else ("candidate", "reference")
            for position, side in zip(("first", "second"), sides):
                document = json.loads((timing / "raw" / f"{side}-{pair}.json").read_text(encoding="ascii"))
                binding = document["binding"]
                assert binding["matrix_sha256"] == manifest["matrix_sha256"]
                assert binding["warmups"] == 0 and binding["measured_runs"] == 1
                assert binding["label"].endswith(f"pair {pair} {position}")
                assert document["limits"]["within_memory_limit"] and not document["limits"]["runs_over_timeout"]
                assert not document["timer"]["reconciliation_failures"]
                max_rss = max(max_rss, document["limits"]["peak_rss_bytes"])
                assert abs(document["timer"]["worst_unaccounted_fraction"]) <= 0.02

    ref = timing_rows(timing / "samples-reference.tsv", expected_cases)
    cand = timing_rows(timing / "samples-candidate.tsv", expected_cases)
    assert ref.keys() == cand.keys()
    for rows in (ref, cand):
        for case in cases:
            for pair in range(15):
                parts = sum(rows[(case, phase)][pair] for phase in PHASES[:-1])
                combined = rows[(case, "combined")][pair]
                worst_residue = max(worst_residue, abs((combined - parts) / combined))
    assert worst_residue <= 0.02 and max_rss <= 2 * 1024 * 1024 * 1024
    targets = list(DISCLOSED_TARGETS) if matrix == "disclosed" else cases
    scores = {}
    for phase in ("emission_exclusive", "combined"):
        ref_score = [sum(ref[(case, phase)][pair] for case in targets) for pair in range(15)]
        cand_score = [sum(cand[(case, phase)][pair] for case in targets) for pair in range(15)]
        scores[phase] = paired(ref_score, cand_score)
        scores[phase]["pass"] = scores[phase]["estimate"] <= -scores[phase]["mde"]

    cells = []
    for case in cases:
        for phase in PHASES:
            result = paired(
                [ref[(case, phase)][pair] for pair in range(15)],
                [cand[(case, phase)][pair] for pair in range(15)],
            )
            bound = max(0.10, result["mde"])
            summary = {key: value for key, value in result.items() if not key.endswith("_scores")}
            cells.append({"case": case, "phase": phase, **summary, "regression_bound": bound, "pass": result["estimate"] <= bound})
    cell_failures = [
        {key: cell[key] for key in ("case", "phase", "estimate", "noise_mad", "mde", "regression_bound")}
        for cell in cells
        if not cell["pass"]
    ]

    resources = {side: resource_rows(resource / f"{side}.tsv", expected_cases) for side in ("reference", "candidate")}
    assert resources["reference"].keys() == resources["candidate"].keys() == ref.keys()
    resource_regressions = []
    max_resource_delta = {metric: float("-inf") for metric in RESOURCE_METRICS}
    for key in resources["reference"]:
        for metric in RESOURCE_METRICS:
            delta = (resources["candidate"][key][metric] - resources["reference"][key][metric]) / resources["reference"][key][metric]
            max_resource_delta[metric] = max(max_resource_delta[metric], delta)
            if delta > 0.05:
                resource_regressions.append({"case": key[0], "phase": key[1], "metric": metric, "delta": delta})
        max_rss = max(max_rss, resources["reference"][key]["process_peak_rss_bytes"], resources["candidate"][key]["process_peak_rss_bytes"])
    assert not resource_regressions and max_rss <= 2 * 1024 * 1024 * 1024

    candidate_combined = sum(statistics.median(cand[(case, "combined")].values()) for case in cases)
    candidate_emission = sum(statistics.median(cand[(case, "emission_exclusive")].values()) for case in cases)
    emission_share = candidate_emission / candidate_combined
    return {
        "matrix": matrix,
        "case_count": expected_cases,
        "manifest_sha256": digest(manifests[0]),
        "matrix_sha256": manifest["matrix_sha256"],
        "workload_sha256": workloads,
        "correctness": f"{expected_cases}/{expected_cases} both, artifacts byte-identical",
        "pairs": 15,
        "warmups": 3,
        "scores": scores,
        "per_case_phase": cells,
        "per_case_phase_failures": cell_failures,
        "resource_max_delta": max_resource_delta,
        "resource_regressions": resource_regressions,
        "max_process_rss_bytes": max_rss,
        "worst_span_residue": worst_residue,
        "candidate_emission_share": emission_share,
        "candidate_emission_ceiling": 1 / (1 - emission_share),
        "pass": all(score["pass"] for score in scores.values()) and not cell_failures,
    }


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: analyze-final.py RUN_DIR")
    root = Path(sys.argv[1])
    results = {matrix: audit_matrix(root, matrix) for matrix in ("disclosed", "held-out")}
    result = {
        "schema": "flutterdec-final-performance/1",
        "reference_product_ref": REFERENCE,
        "candidate_product_ref": CANDIDATE,
        "accepted_timing_harness_ref": TIMING_HARNESS,
        "resource_harness_ref": RESOURCE_HARNESS,
        "matrices": results,
        "decision": "accepted-win" if all(item["pass"] for item in results.values()) else "honest-no-win",
    }
    (root / "final-audit.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="ascii")
    for matrix, item in results.items():
        for phase, score in item["scores"].items():
            print(f"{matrix}\t{phase}\testimate={score['estimate']:.8f}\tnoise={score['noise_mad']:.8f}\tmde={score['mde']:.8f}\tpass={score['pass']}")
        print(f"{matrix}\tresources\tmax={item['resource_max_delta']}\trss={item['max_process_rss_bytes']}\tresidue={item['worst_span_residue']:.8f}")
    print(f"decision\t{result['decision']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

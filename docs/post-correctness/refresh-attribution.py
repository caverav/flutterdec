#!/usr/bin/env python3
"""Recompute post-correctness phase, case, allocation, and leverage tables."""

import csv
import hashlib
import json
import statistics
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path

PHASES = ("ir", "cfg", "emission_exclusive", "serialization")
PRODUCTS = {
    "reference": "1371e42549472ec388f58bc1fd5dbdf96e8dcdd1",
    "candidate": "5ba4b6d30604606c04b5b742eaf9469adc1c729d",
}
HARNESS = "4c127aba4e74fb6f8d486c4cb066586bb0d74846"
RESOURCE_HARNESS = "b0e615785b28e7e58aa06dd1b929dd58acf06e53"
PATCH = "14413796ca8a89cc1328497b5c87629b1c55f945ec58e73eebb3838df0700460"
MATRIX_SHA256 = "76b617c62858e698710f0ab068c1a8b6d8458feedc83f3738a5f5664cfacbc43"
MANIFEST_SHA256 = "bfb167600ee186d4e360958348cc8892e3dee2620f9dcdeaf9fcd60c20fd3bc7"
HARNESS_TREE = "83e06014b368736c1921a0da7949c7b6a0b76e97"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def key_values(path):
    return {
        fields[0]: fields[1]
        for line in path.read_text().splitlines()
        if len(fields := line.split(None, 1)) == 2
    }


def binding(root):
    return key_values(root / "binding.txt")


def audit_checksums(root):
    checksum_file = root / "SHA256SUMS"
    if not checksum_file.exists():
        return
    listed = {}
    for line in checksum_file.read_text().splitlines():
        digest_value, name = line.split("  ", 1)
        assert name.startswith("./") and name not in listed
        listed[name] = digest_value
    actual = {
        f"./{path.relative_to(root)}"
        for path in root.rglob("*")
        if path.is_file() and path.name not in {"SHA256SUMS", ".lock"}
    }
    assert set(listed) == actual
    for name, digest_value in listed.items():
        assert sha256(root / name[2:]) == digest_value
    print(f"checksums={len(listed)}/{len(actual)}")


def sample_rows(path):
    rows = list(csv.DictReader(path.open(newline=""), delimiter="\t"))
    assert len(rows) == 33 * 5
    keys = {(row["case"], row["phase"]) for row in rows}
    assert len(keys) == len(rows)
    assert {row["phase"] for row in rows} == set(PHASES) | {"combined"}
    assert {row["run"] for row in rows} == {"0"}
    return {(row["case"], row["phase"]): row for row in rows}


def audit_live(root):
    audit_checksums(root)
    bind = binding(root)
    preflight = key_values(root / "workspace-preflight.txt")
    assert preflight["schema"] == "flutterdec-bench/workspace-preflight/1"
    assert preflight["head"] == bind["controlling_repository_head"]
    assert len(preflight["head"]) == 40
    int(preflight["head"], 16)
    assert preflight["porcelain_status"] == "empty"
    assert preflight["porcelain_status_bytes"] == "0"
    assert preflight["porcelain_status_sha256"] == hashlib.sha256(b"").hexdigest()
    assert preflight["command"]
    datetime.fromisoformat(preflight["timestamp"])
    assert bind["harness_ref"] == HARNESS
    assert bind["patch_sha256"] == PATCH
    assert bind["harness_tree_oid"] == HARNESS_TREE
    assert bind["reference_product_ref"] == PRODUCTS["reference"]
    assert bind["candidate_product_ref"] == PRODUCTS["candidate"]
    for side in PRODUCTS:
        assert sha256(root / "bin" / "timing" / side / "flutterdec-bench") == bind[
            f"{side}_binary_sha256"
        ]
        assert sha256(root / "bin" / "resource" / side / "flutterdec-bench") == bind[
            f"resource_{side}_binary_sha256"
        ]

    manifests = [root / "manifest-reference.json", root / "manifest-candidate.json"]
    assert manifests[0].read_bytes() == manifests[1].read_bytes()
    assert sha256(manifests[0]) == MANIFEST_SHA256
    manifest = json.loads(manifests[0].read_text())
    assert manifest["matrix_sha256"] == MATRIX_SHA256
    workloads = {row["case"]: row["workload_sha256"] for row in manifest["cases"]}
    assert len(workloads) == 33 and len(set(workloads.values())) == 33

    expected_order = []
    max_rss = 0
    worst_residue = 0.0
    measured_passes = 0
    for pair in range(15):
        sides = ("reference", "candidate") if pair % 2 == 0 else ("candidate", "reference")
        for position, side in zip(("first", "second"), sides):
            expected_order.append(f"{pair}\t{position}\t{side}")
            document = json.loads((root / "raw" / f"{side}-{pair}.json").read_text())
            b = document["binding"]
            assert b["product_ref"] == PRODUCTS[side]
            assert b["harness_ref"] == HARNESS and b["patch_sha256"] == PATCH
            assert b["binary_sha256"] == bind[f"{side}_binary_sha256"]
            assert b["matrix_sha256"] == MATRIX_SHA256
            assert b["warmups"] == 0 and b["measured_runs"] == 1
            assert b["label"].endswith(f"pair {pair} {position}")
            assert document["limits"]["within_memory_limit"]
            assert not document["limits"]["runs_over_timeout"]
            assert not document["timer"]["reconciliation_failures"]
            assert len(document["runs"]) == 1
            assert {row["case"]: row["workload_sha256"] for row in document["cases"]} == workloads
            samples = sample_rows(root / "raw" / f"{side}-{pair}.tsv")
            cases = document["runs"][0]["cases"]
            assert len(cases) == 33
            for case in cases:
                name = case["case"]
                parts = 0
                for phase in PHASES:
                    field = f"{phase}_nanos"
                    value = int(samples[(name, phase)]["nanos"])
                    assert value == case[field]
                    parts += value
                combined = int(samples[(name, "combined")]["nanos"])
                assert combined == case["combined_nanos"]
                residue = (combined - parts) / combined
                assert 0 <= residue <= document["timer"]["reconciliation_tolerance"]
                assert abs(residue - case["unaccounted_fraction"]) < 5e-7
                worst_residue = max(worst_residue, residue)
                measured_passes += 1
            max_rss = max(max_rss, document["limits"]["peak_rss_bytes"])
    assert (root / "pair-order.tsv").read_text().splitlines() == expected_order
    assert (root / "planned-pair-order.tsv").read_text().splitlines() == expected_order

    chronology = list(csv.DictReader((root / "chronology.tsv").open(newline=""), delimiter="\t"))
    assert len(chronology) == 34
    assert [int(row["sequence"]) for row in chronology] == list(range(34))
    previous_end = 0
    for row in chronology:
        start, end = int(row["start_epoch_ns"]), int(row["end_epoch_ns"])
        assert previous_end <= start < end
        assert (root / row["artifact"]).is_file()
        previous_end = end
    assert [row["kind"] for row in chronology] == ["warmup", "warmup"] + ["measured"] * 30 + ["resource", "resource"]

    resource_rows = 0
    for side, product in PRODUCTS.items():
        document = json.loads((root / "resource" / f"{side}.json").read_text())
        b = document["binding"]
        assert b["product_ref"] == product and b["harness_ref"] == RESOURCE_HARNESS
        assert b["warmups"] == 3 and b["threads"] == 1 and b["plant"] == "none"
        assert b["binary_sha256"] == bind[f"resource_{side}_binary_sha256"]
        tsv = list(csv.DictReader((root / "resource" / f"{side}.tsv").open(newline=""), delimiter="\t"))
        assert len(tsv) == 33 * 5
        actual = {}
        for case in document["cases"]:
            assert case["instrumentation_recursions"] == 0
            for phase in case["phases"]:
                key = (case["case"], phase["phase"])
                actual[key] = {
                    **{name: int(value) for name, value in phase["metrics"].items() if name != "live_bytes_at_snapshot"},
                    "process_peak_rss_bytes": int(case["process_peak_rss_bytes"]),
                }
            actual[(case["case"], "combined")] = {
                **{name: int(value) for name, value in case["combined"].items() if name != "live_bytes_at_snapshot"},
                "process_peak_rss_bytes": int(case["process_peak_rss_bytes"]),
            }
        assert len(actual) == 33 * 5
        for row in tsv:
            key = (row["case"], row["phase"])
            assert key in actual
            for metric in ("allocation_count", "total_allocated_bytes", "peak_live_bytes", "process_peak_rss_bytes"):
                assert int(row[metric]) == actual[key][metric]
            max_rss = max(max_rss, int(row["process_peak_rss_bytes"]))
        assert max_rss <= 2 * 1024 * 1024 * 1024
        resource_rows += len(tsv)

    for side, product in PRODUCTS.items():
        warmup = json.loads((root / f"warmup-{side}.json").read_text())
        b = warmup["binding"]
        assert b["product_ref"] == product and b["harness_ref"] == HARNESS
        assert b["patch_sha256"] == PATCH and b["binary_sha256"] == bind[f"{side}_binary_sha256"]
        assert b["warmups"] == 3 and b["measured_runs"] == 0 and b["correctness_checked"]
        assert not warmup["correctness_failures"]
        assert len(warmup["cases"]) == 33
        assert all(row["correctness"]["passed"] for row in warmup["cases"])
        assert {row["workload_sha256"] for row in warmup["cases"]} == set(workloads.values())
        assert warmup["limits"]["within_memory_limit"]
        assert not warmup["limits"]["runs_over_timeout"]

    print("PASS live raw audit before aggregation")
    print(f"raw_documents=30 sample_streams=30 measured_passes={measured_passes}")
    print(f"pair_order=30/30 alternating correctness=33/33_both workloads=33_unique")
    print(f"chronology=34/34 non_overlapping resource_rows={resource_rows} raw_lanes_skipped=0")
    print(f"max_rss_bytes={max_rss} worst_unaccounted_fraction={worst_residue:.8f}")
    print(f"harness_tree_oid={HARNESS_TREE}")
    return max_rss, worst_residue


def load(path):
    rows = defaultdict(lambda: defaultdict(list))
    with path.open(newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            rows[row["case"]][row["phase"]].append(
                (int(row["nanos"]), int(row["alloc_count"]), int(row["alloc_bytes"]))
            )
    return rows


def medians(rows):
    return {
        case: {
            phase: tuple(statistics.median(v[i] for v in values) for i in range(3))
            for phase, values in phases.items()
        }
        for case, phases in rows.items()
    }


def median_mad(values):
    centre = statistics.median(values)
    return centre, statistics.median(abs(value - centre) for value in values)


def audit_analysis(root, ref_rows, cand_rows):
    analysis = json.loads((root / "analysis.json").read_text())
    assert analysis["schema"] == "flutterdec-bench/analysis/1"
    assert analysis["mde_floor"] == 0.05 and analysis["mde_noise_multiple"] == 3.0
    assert analysis["series_count"] == 33 * 5 and analysis["unpaired_samples"] == 0

    def summary(case, phase, ref, cand):
        ref_med, ref_mad = median_mad(ref)
        cand_med, cand_mad = median_mad(cand)
        deltas = [0.0 if left == 0 else (right - left) / left for left, right in zip(ref, cand)]
        delta, noise = median_mad(deltas)
        mde = max(0.05, 3 * noise)
        return {
            "case": case,
            "phase": phase,
            "pairs": len(deltas),
            "reference_median_nanos": ref_med,
            "reference_mad_nanos": ref_mad,
            "candidate_median_nanos": cand_med,
            "candidate_mad_nanos": cand_mad,
            "median_paired_delta": delta,
            "noise_mad_of_deltas": noise,
            "mde": mde,
            "clears_mde": abs(delta) >= mde,
            "direction": "faster" if delta < 0 else "slower",
        }

    expected = []
    for case in sorted(ref_rows):
        for phase in sorted(ref_rows[case]):
            left = [value[0] for value in ref_rows[case][phase]]
            right = [value[0] for value in cand_rows[case][phase]]
            expected.append(summary(case, phase, left, right))
    pooled = []
    for phase in sorted(PHASES + ("combined",)):
        left, right = [], []
        for case in sorted(ref_rows):
            left.extend(value[0] for value in ref_rows[case][phase])
            right.extend(value[0] for value in cand_rows[case][phase])
        pooled.append(summary("all", phase, left, right))

    def same(actual, wanted):
        assert actual.keys() == wanted.keys()
        for key, value in wanted.items():
            if isinstance(value, float):
                assert abs(actual[key] - value) <= 0.00000051, (key, actual[key], value)
            else:
                assert actual[key] == value, (key, actual[key], value)

    assert len(analysis["by_case_and_phase"]) == len(expected)
    for actual, wanted in zip(analysis["by_case_and_phase"], expected):
        same(actual, wanted)
    for actual, wanted in zip(analysis["by_phase"], pooled):
        same(actual, wanted)
    worst = max(expected, key=lambda row: row["median_paired_delta"])
    assert analysis["worst_regression"]["case"] == worst["case"]
    assert analysis["worst_regression"]["phase"] == worst["phase"]
    assert abs(analysis["worst_regression"]["median_paired_delta"] - worst["median_paired_delta"]) <= 0.00000051


def audit_collected_samples(root):
    for side in PRODUCTS:
        collected = list(csv.DictReader((root / f"samples-{side}.tsv").open(newline=""), delimiter="\t"))
        assert len(collected) == 15 * 33 * 5
        expected = []
        for pair in range(15):
            rows = list(csv.DictReader((root / "raw" / f"{side}-{pair}.tsv").open(newline=""), delimiter="\t"))
            for row in rows:
                row["run"] = str(pair)
                expected.append(row)
        assert collected == expected


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--audit-live":
        audit_live(Path(sys.argv[2]))
        return
    if len(sys.argv) != 2:
        raise SystemExit("usage: refresh-attribution.py [--audit-live] RUN_DIR")
    root = Path(sys.argv[1])
    max_rss, worst_residue = audit_live(root)
    ref_rows = load(root / "samples-reference.tsv")
    cand_rows = load(root / "samples-candidate.tsv")
    audit_collected_samples(root)
    audit_analysis(root, ref_rows, cand_rows)
    ref = medians(ref_rows)
    cand = medians(cand_rows)
    manifest_text = (root / "manifest-reference.json").read_text()
    assert manifest_text == (root / "manifest-candidate.json").read_text()
    manifest = json.loads(manifest_text)
    assert manifest["matrix"] == "disclosed"
    assert manifest["matrix_sha256"] == MATRIX_SHA256
    assert hashlib.sha256(manifest_text.encode()).hexdigest() == MANIFEST_SHA256
    baseline_manifest = Path(__file__).parents[1] / "baseline/aa-1/manifest-reference.json"
    assert manifest_text == baseline_manifest.read_text()
    meta = {row["case"]: row for row in manifest["cases"]}
    assert len(meta) == 33
    for side, rows in (("reference", ref_rows), ("candidate", cand_rows)):
        assert set(rows) == set(meta), side
        for phases in rows.values():
            assert set(phases) == set(PHASES) | {"combined"}
            assert all(len(values) == 15 for values in phases.values())

    for side in ("reference", "candidate"):
        warmup = json.loads((root / f"warmup-{side}.json").read_text())
        assert warmup["binding"]["product_ref"] == PRODUCTS[side]
        assert warmup["binding"]["harness_ref"] == HARNESS
        assert warmup["binding"]["patch_sha256"] == PATCH
        assert warmup["binding"]["warmups"] == 3
        assert warmup["binding"]["measured_runs"] == 0
        assert all(case["correctness"]["passed"] for case in warmup["cases"])
        assert warmup["limits"]["within_memory_limit"]
        assert not warmup["limits"]["runs_over_timeout"]

    order = []
    for pair in range(15):
        expected = (("reference", "candidate") if pair % 2 == 0 else ("candidate", "reference"))
        for position, side in zip(("first", "second"), expected):
            order.append(f"{pair}\t{position}\t{side}")
            document = json.loads((root / "raw" / f"{side}-{pair}.json").read_text())
            assert f"pair {pair} {position}" in document["binding"]["label"]
            assert document["binding"]["product_ref"] == PRODUCTS[side]
            assert document["binding"]["harness_ref"] == HARNESS
            assert document["binding"]["patch_sha256"] == PATCH
            assert document["binding"]["warmups"] == 0
            assert document["binding"]["measured_runs"] == 1
            assert document["limits"]["within_memory_limit"]
            assert not document["limits"]["runs_over_timeout"]
            assert not document["timer"]["reconciliation_failures"]
            max_rss = max(max_rss, document["limits"]["peak_rss_bytes"])
            worst_residue = max(worst_residue, document["timer"]["worst_unaccounted_fraction"])
    assert (root / "pair-order.tsv").read_text().splitlines() == order
    assert (root / "planned-pair-order.tsv").read_text().splitlines() == order

    total = sum(cand[c]["combined"][0] for c in cand)

    print("audit\tvalue")
    print("cases\t33")
    print("pairs\t15")
    print("correctness\t33/33 both sides")
    print(f"max_rss_bytes\t{max_rss}")
    print(f"worst_unaccounted_fraction\t{worst_residue:.8f}")
    print("timeouts\t0")
    print("\nphase\treference_ns\tpost_correctness_ns\treference_share\tpost_correctness_share\ttime_weighted_delta")
    old_total = sum(ref[c]["combined"][0] for c in ref)
    for phase in PHASES:
        old = sum(ref[c][phase][0] for c in ref)
        new = sum(cand[c][phase][0] for c in cand)
        print(f"{phase}\t{old:.0f}\t{new:.0f}\t{old/old_total:.8f}\t{new/total:.8f}\t{(new-old)/old:+.8f}")
    print(f"combined\t{old_total:.0f}\t{total:.0f}\t1.00000000\t1.00000000\t{(total-old_total)/old_total:+.8f}")

    analysis = json.loads((root / "analysis.json").read_text())
    print("\npaired 1371e42-to-post-correctness cost (descriptive only, not candidate MDE)")
    print("phase\tmedian_paired_delta\tMAD")
    for row in analysis["by_phase"]:
        print(f"{row['phase']}\t{row['median_paired_delta']:+.8f}\t{row['noise_mad_of_deltas']:.8f}")

    def target_prefix(phase):
        phase_total = sum(cand[c][phase][0] for c in cand)
        cumulative = 0.0
        targets = []
        print(f"\ntarget cases: shortest descending {phase} prefix >= 0.75")
        print("case\tphase_ns\tphase_share\tcumulative\tcombined_share")
        for case in sorted(cand, key=lambda c: (-cand[c][phase][0], c)):
            if cumulative >= 0.75:
                break
            share = cand[case][phase][0] / phase_total
            cumulative += share
            targets.append(case)
            print(f"{case}\t{cand[case][phase][0]:.0f}\t{share:.8f}\t{cumulative:.8f}\t{cand[case]['combined'][0]/total:.8f}")
        return targets

    targets = target_prefix("emission_exclusive")
    cfg_targets = target_prefix("cfg")

    print("\nallocation shape")
    print("phase\tcount\tbytes\tcount_share\tbytes_share")
    counts = {p: sum(cand[c][p][1] for c in cand) for p in PHASES}
    sizes = {p: sum(cand[c][p][2] for c in cand) for p in PHASES}
    count_total = sum(cand[c]["combined"][1] for c in cand)
    size_total = sum(cand[c]["combined"][2] for c in cand)
    for phase in PHASES:
        print(f"{phase}\t{counts[phase]:.0f}\t{sizes[phase]:.0f}\t{counts[phase]/count_total:.8f}\t{sizes[phase]/size_total:.8f}")

    shares = {p: sum(cand[c][p][0] for c in cand) / total for p in PHASES}
    dominant = max(shares, key=shares.get)
    print("\nsummary")
    print(f"dominant_phase\t{dominant}\t{shares[dominant]:.8f}")
    print(f"amdahl_speedup_if_removed\t{1/(1-shares[dominant]):.8f}")
    print(f"E1_shared_emission_leverage\t{shares['emission_exclusive']:.8f}")
    target_share = sum(cand[c]["emission_exclusive"][0] for c in targets) / total
    print(f"E2_target_emission_leverage\t{target_share:.8f}")
    print(f"E3_cfg_leverage\t{shares['cfg']:.8f}")
    cfg_target_share = sum(cand[c]["cfg"][0] for c in cfg_targets) / total
    print(f"E3_target_cfg_leverage\t{cfg_target_share:.8f}")
    print("trial_order\tE1,E2,E3")
    print("mde_rule\tmax(0.05, 3 * MAD) from each later comparison's own 15 paired deltas")

    print("\nper-case post-correctness shares")
    print("case\tblocks\tinstructions\tcombined_share\tir_share\tcfg_share\temission_share\tserialization_share")
    phase_totals = {p: sum(cand[c][p][0] for c in cand) for p in PHASES}
    for case in sorted(cand, key=lambda c: (-cand[c]["combined"][0], c)):
        phase_shares = "\t".join(f"{cand[case][p][0]/phase_totals[p]:.8f}" for p in PHASES)
        print(f"{case}\t{meta[case]['blocks']}\t{meta[case]['instructions']}\t{cand[case]['combined'][0]/total:.8f}\t{phase_shares}")


if __name__ == "__main__":
    main()

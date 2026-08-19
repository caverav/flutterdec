#!/usr/bin/env python3
"""Recompute post-correctness phase, case, allocation, and leverage tables."""

import csv
import hashlib
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

PHASES = ("ir", "cfg", "emission_exclusive", "serialization")
PRODUCTS = {
    "reference": "1371e42549472ec388f58bc1fd5dbdf96e8dcdd1",
    "candidate": "630ec442d951aac5704ae80287367912bfbfc388",
}
HARNESS = "4c127aba4e74fb6f8d486c4cb066586bb0d74846"
PATCH = "14413796ca8a89cc1328497b5c87629b1c55f945ec58e73eebb3838df0700460"
MATRIX_SHA256 = "76b617c62858e698710f0ab068c1a8b6d8458feedc83f3738a5f5664cfacbc43"
MANIFEST_SHA256 = "bfb167600ee186d4e360958348cc8892e3dee2620f9dcdeaf9fcd60c20fd3bc7"


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


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: refresh-attribution.py RUN_DIR")
    root = Path(sys.argv[1])
    ref_rows = load(root / "samples-reference.tsv")
    cand_rows = load(root / "samples-candidate.tsv")
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

    max_rss = 0
    worst_residue = 0.0
    order = []
    for pair in range(15):
        expected = (("reference", "candidate") if pair % 2 == 0 else ("candidate", "reference"))
        for position, side in zip(("first", "second"), expected):
            document = json.loads((root / "raw" / f"{side}-{pair}.json").read_text())
            order.append(f"{pair}\t{position}\t{side}")
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

#!/usr/bin/env python3
"""Build the per-loss-site coverage ledger, and reconcile its `omitted_by_cap`.

One row per (sample, loss site). Figures are per sample and never averaged: the
two samples are two SDK generations, and a mean over them describes neither.

`omitted_by_cap` is the field a cap is not allowed to hide behind, so it is
reconciled two ways that fail the build and reported two more ways that do not.

Failing checks:

  rows vs corpus   - the site's emitted-annotation rows are counted against what
                     the corpus scan found for that site's literals. The audit
                     places a record by locating its span in the finished
                     source, so a record that never got placed would otherwise
                     leave the ledger quietly short.
  rows vs counter  - the ledger's detail rows are counted against
                     `omitted_at_insertion`, which the emitter counts at the
                     drop and publishes per function in a `cap_summary` record.
                     A row lost between the emitter and the audit file shows up
                     here instead of as a smaller, still plausible total.

Whether a drop was whole rather than a truncation is *not* asked here. It is a
property of the artifact and the corpus scan is what answers it: a span cut down
to size is either left without its terminator, which the scan reports as
`unclosed`, or longer than a budget, which it reports as `over_span` and
`over_cap`. A prefix heuristic over the span inventory would be a weaker
restatement of those three, so it is not run.

Reported, not failed, because both have benign causes that the shipped budgets
never reach and a lowered-budget control does:

  also emitted elsewhere - a dropped span's *text* may be emitted at some other
                     site. Spans are not unique; identical values at two joins
                     render identical bytes. Text presence is not evidence that
                     the drop was false.
  not in the uncapped run - the same tree with both budgets raised to
                     `usize::MAX` should carry the dropped span, and usually
                     does. It will not when two anchors compete for one output
                     coordinate: at most one annotation per register spelling
                     survives, so raising the budgets lets the first one take
                     the coordinate the second would have taken. Both counts and
                     per-span presence are affected, which is why this is a
                     figure and not a verdict.
"""

import argparse
import json
import pathlib
import sys
from collections import defaultdict

SITES = ("join", "loop_entry", "call")
BUDGETS = ("annotation", "line")


def read_spans(path):
    """`count<TAB>span` inventory, as written by the safety scan."""
    spans = {}
    for line in pathlib.Path(path).read_text().splitlines():
        count, _, span = line.partition("\t")
        spans[span] = int(count)
    return spans


def read_audit(path):
    for line in pathlib.Path(path).read_text().splitlines():
        if line.strip():
            yield json.loads(line)


def scanned_by_site(scan):
    """The corpus scan's annotation counts, folded onto the three loss sites.

    The join site owns two literals - a covered join and one missing an arm say
    different things and must not be one label - so the fold is by literal, in
    the order the emitter defines them.
    """
    counts = scan["annotations_by_literal"]
    openers = scan["openers"]
    return {
        "join": counts[openers[0]] + counts[openers[1]],
        "loop_entry": counts[openers[2]],
        "call": counts[openers[3]],
    }


def sample_ledger(audit, a_spans, b_spans, scanned):
    sites = {
        site: {
            "annotations": 0,
            "candidate_elements": 0,
            "omitted_by_cap": 0,
            "omitted_by_cap_annotation_budget": 0,
            "omitted_by_cap_line_budget": 0,
            "omitted_by_unsafe_span": 0,
            "omitted_at_insertion": 0,
            "annotations_in_corpus": scanned[site],
        }
        for site in SITES
    }
    unknown_sites = defaultdict(int)
    dropped = []

    for record in audit:
        kind = record.get("record")
        if kind == "annotation":
            site = record["loss_site"]
            if site not in sites:
                unknown_sites[site] += 1
                continue
            sites[site]["annotations"] += 1
            sites[site]["candidate_elements"] += len(record["candidates"])
        elif kind == "cap_omission":
            site = record["loss_site"]
            if site not in sites:
                unknown_sites[site] += 1
                continue
            if record["budget"] in BUDGETS:
                sites[site]["omitted_by_cap"] += 1
                sites[site][f"omitted_by_cap_{record['budget']}_budget"] += 1
            else:
                sites[site]["omitted_by_unsafe_span"] += 1
            dropped.append(record)
        elif kind == "cap_summary":
            site = record["loss_site"]
            if site not in sites:
                unknown_sites[site] += 1
                continue
            sites[site]["omitted_at_insertion"] += record["omitted_at_insertion"]

    checks = {
        "audit_rows_vs_corpus_scan": [],
        "rows_vs_counter": [],
    }
    reported = {
        "dropped_span_also_emitted_elsewhere": [],
        "dropped_span_not_in_uncapped_corpus": [],
    }
    for site, row in sites.items():
        # The audit places a record by finding its span in the finished source,
        # so a record that never got placed is a row the ledger would be missing
        # without ever saying so. The corpus is the arbiter.
        if row["annotations"] != row["annotations_in_corpus"]:
            checks["audit_rows_vs_corpus_scan"].append(
                {
                    "loss_site": site,
                    "audit_rows": row["annotations"],
                    "in_corpus": row["annotations_in_corpus"],
                }
            )
        counted = row["omitted_by_cap"] + row["omitted_by_unsafe_span"]
        if counted != row["omitted_at_insertion"]:
            checks["rows_vs_counter"].append(
                {"loss_site": site, "rows": counted, "counter": row["omitted_at_insertion"]}
            )

    for record in dropped:
        span = record["rendered"]
        where = {
            "loss_site": record["loss_site"],
            "function_id": record["function_id"],
            "register": record["register"],
            "budget": record["budget"],
            "annotation_len": record["annotation_len"],
        }
        if span in a_spans:
            reported["dropped_span_also_emitted_elsewhere"].append(where)
        if span not in b_spans:
            reported["dropped_span_not_in_uncapped_corpus"].append(where)

    return {
        "loss_sites": sites,
        "totals": {
            key: sum(row[key] for row in sites.values()) for key in next(iter(sites.values()))
        },
        "dropped_sites_scanned": len(dropped),
        "unknown_loss_sites": dict(unknown_sites),
        "reconciliation": {name: len(hits) for name, hits in checks.items()},
        "reconciliation_detail": {name: hits[:20] for name, hits in checks.items() if hits},
        "reported_not_failed": {name: len(hits) for name, hits in reported.items()},
        "reconciled": all(not hits for hits in checks.values()) and not unknown_sites,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--sample",
        action="append",
        required=True,
        metavar="NAME=AUDIT,A_SPANS,B_SPANS,A_SCAN",
        help="one per sample; figures are never combined across them",
    )
    parser.add_argument("--candidate-sha256", required=True)
    parser.add_argument("--reference-sha256", required=True)
    parser.add_argument("--uncapped-sha256", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    ledger = {
        "schema_version": 1,
        "fields": {
            "annotations": "annotations emitted at this loss site",
            "candidate_elements": "rendered candidate values across those annotations",
            "omitted_by_cap": "annotations dropped whole by a budget",
            "omitted_by_cap_annotation_budget": "of those, dropped by MAX_JOIN_ANNOTATION",
            "omitted_by_cap_line_budget": "of those, dropped by MAX_JOIN_ANNOTATED_LINE",
            "omitted_by_unsafe_span": "dropped by the structural gate, not by a budget",
            "omitted_at_insertion": "the emitter's own count of every drop, for reconciliation",
            "annotations_in_corpus": "annotations the corpus scan found for this site's literals",
        },
        "binaries": {
            "candidate_sha256": args.candidate_sha256,
            "reference_sha256": args.reference_sha256,
            "uncapped_control_sha256": args.uncapped_sha256,
        },
        "samples": {},
    }

    for spec in args.sample:
        name, _, paths = spec.partition("=")
        audit, a_spans, b_spans, a_scan = paths.split(",")
        ledger["samples"][name] = sample_ledger(
            read_audit(audit),
            read_spans(a_spans),
            read_spans(b_spans),
            scanned_by_site(json.loads(pathlib.Path(a_scan).read_text())),
        )

    pathlib.Path(args.out).write_text(json.dumps(ledger, indent=2))
    json.dump(ledger, sys.stdout, indent=2)
    print()
    return 0 if all(s["reconciled"] for s in ledger["samples"].values()) else 1


if __name__ == "__main__":
    sys.exit(main())

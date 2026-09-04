#!/usr/bin/env bash
# Pre-measurement identity gate for the phase benchmark.
#
# Binary identity is checked in both directions, because both directions of
# disagreement void the run:
#
#   equal product commits    => must be the same machine code. Otherwise every
#                               delta is build layout, and an A/A run publishes
#                               a fabricated noise floor.
#   different product commits => must be different machine code. Identical bytes
#                               from two different revisions means the product
#                               delta never reached the binary, so there is
#                               nothing to compare and any number the run prints
#                               is pure measurement error labelled as a product
#                               effect.
#
# Either way, refuse before warmup rather than after 15 pairs.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: scripts/bench-identity-gate.sh REF_COMMIT CAND_COMMIT REF_SHA256 CAND_SHA256" >&2
  exit 2
fi

reference_commit="$1"
candidate_commit="$2"
reference_digest="$3"
candidate_digest="$4"

if [[ "$reference_commit" == "$candidate_commit" ]]; then
  if [[ "$reference_digest" != "$candidate_digest" ]]; then
    cat >&2 <<EOF
[identity-gate] both sides resolve to product commit $reference_commit but their binaries differ:
  reference $reference_digest
  candidate $candidate_digest
One product revision under one harness patch must build to one binary. Aborting
before warmup: any measured delta would be build layout, not product code.
EOF
    exit 1
  fi
  echo "[identity-gate] both sides at $reference_commit and byte-identical: $reference_digest"
  exit 0
fi

if [[ "$reference_digest" == "$candidate_digest" ]]; then
  cat >&2 <<EOF
[identity-gate] product commits differ but both build to the same binary:
  reference $reference_commit
  candidate $candidate_commit
  binary    $reference_digest
Two revisions that compile to identical machine code have nothing to compare.
Aborting before warmup: any measured delta would be measurement error reported
as a product effect.
EOF
  exit 1
fi

echo "[identity-gate] product commits differ ($reference_commit vs $candidate_commit) and so do their binaries: $reference_digest vs $candidate_digest"

#!/usr/bin/env bash
# Pre-measurement identity gate for the phase benchmark.
#
# When both sides of a run resolve to the same product commit under the same
# harness patch, they must also be the same machine code. If they are not, every
# delta the run reports is build layout rather than product behaviour, and an
# A/A run in particular would publish a fabricated noise floor. Refuse before
# warmup rather than after 15 pairs.
#
# Different product commits are the normal A/B case: differing binaries are
# expected there and the gate says nothing about them.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: scripts/bench-identity-gate.sh REF_COMMIT CAND_COMMIT REF_SHA256 CAND_SHA256" >&2
  exit 2
fi

reference_commit="$1"
candidate_commit="$2"
reference_digest="$3"
candidate_digest="$4"

if [[ "$reference_commit" != "$candidate_commit" ]]; then
  echo "[identity-gate] product commits differ ($reference_commit vs $candidate_commit); binaries are expected to differ"
  exit 0
fi

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

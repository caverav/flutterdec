# Real Golden Baselines

This directory is for optional real-binary golden baselines used by `scripts/real-golden.sh`.

Recommended layout:

- `profiles/<name>/quality.json`
- `profiles/<name>/files.txt`
- `profiles/<name>/pseudocode/...`

Example `files.txt` entries:

- `pseudocode/00080_sub_65f850.dartpseudo`
- `pseudocode/00081_sub_65f9ac.dartpseudo`

Record baseline:

```bash
FLUTTERDEC_REAL_GOLDEN_FILES='pseudocode/00080_sub_65f850.dartpseudo,pseudocode/00081_sub_65f9ac.dartpseudo' \
  scripts/real-golden.sh record \
  --input /path/to/sample.apk \
  --baseline testdata/real-golden/profiles/spotube \
  --max-functions 120 \
  --min-disassembly-ratio 0.0
```

Check baseline:

```bash
scripts/real-golden.sh check \
  --input /path/to/sample.apk \
  --baseline testdata/real-golden/profiles/spotube \
  --max-functions 120 \
  --min-disassembly-ratio 0.0
```

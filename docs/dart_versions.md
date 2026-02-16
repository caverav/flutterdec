# Dart Version Adapter Workflow

## v1 strategy

- Detect a snapshot hash from vm/isolate snapshot data.
- Resolve hash/version mapping via `src/core/dartvm/versions/manifest.json`.
- Use `flutterdec setup --dart-hash <hash>` to fetch SDK sources and install adapter executable.

## Add a new version

1. Add entry to `manifest.json` with:
   - `snapshot_hash`
   - `version`
   - `adapter` executable name
2. Run:
   - `python3 scripts/fetch_dart_sdk.py --dart-hash <hash>`
   - `python3 scripts/build_dart_adapter.py --dart-hash <hash>`
3. Verify:
   - `flutterdec info <libapp.so|apk>` reports adapter available.

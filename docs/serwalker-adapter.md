# Serwalker adapter

Serwalker is a separate protocol-v1 executable. Core does not link its crate.
`--adapter-backend internal` is exclusively machine-code-only recovery;
`--adapter-backend serwalker` pins Serwalker and never silently falls back.

An exact registry record must authorize the snapshot hash, kind, architecture and
normalized feature tuple, plus the parser family, profile and executable digests.
Merely placing a recipe in `data/serwalker_profiles` does not authorize parsing.
The checked-in registry is not automatically rewritten by a local build.

## Local experimental setup

On a supported Linux or macOS host, build both executables:

```sh
cargo build -p flutterdec-cli -p flutterdec-serwalker --release
```

For an explicitly chosen real snapshot, profile and local adapter build, prepare a
new isolated data directory. This does not overwrite the production registry or
claim SDK-reference validation; the generated record is experimental.

```sh
cargo run -p flutterdec-serwalker --example prepare_adapter -- \
  --libapp /path/to/libapp.so \
  --profile /path/to/chosen-serwalker-profile.json \
  --artifact target/release/flutterdec-serwalker \
  --output /tmp/my-serwalker-data

export FLUTTERDEC_DATA_DIR=/tmp/my-serwalker-data
export FLUTTERDEC_ADAPTER_STORE=/tmp/my-serwalker-store
target/release/flutterdec adapter install --dart-hash FULL_HASH_PRINTED_ABOVE \
  --from target/release/flutterdec-serwalker
target/release/flutterdec info /path/to/libapp.so --adapter-backend serwalker --json
```

`auto` also uses this adapter when that exact record is selected and installed.
Rebuilding the adapter changes its digest: prepare a new record and reinstall.
Do not promote an experimental record without real-fixture and SDK-matched
reference comparisons. Multiple exact records can share an executable and profile.

## Invocation

The generic host stages the exact profile bytes it verified at `in/profile.json`
alongside the read-only snapshot handles. The profile is pinned by
`request.compatibility.profile_sha256`; the adapter does not choose another file
by hash prefix or environment variable. Legacy layout metadata remains available
for existing profiles; parser-specific JSON is validated by its parser, not decoded
as a legacy core layout.

The executable accepts `--request` and `--result`, plus the host's optional
`--input-path` and `--libapp-path` arguments. It verifies input sizes/digests and the
isolate header identity, checks the profile digest, and emits ProgramModel v4 with
the host's producer and compatibility binding. A successful result names
`resolved_backend: "serwalker"`; errors produce no heuristic model.

An explicit Serwalker request with an unsupported identity is rejected before
registry/profile lookup. `auto` may instead use honest machine-code recovery.
Once an authorized parser starts, parsing errors remain errors under both modes.

# Serwalker adapter

Serwalker is flutterdec's native snapshot parser and the default backend. It is a
separate protocol-v1 executable; core does not link its crate.
`--adapter-backend auto` runs it for every snapshot a Serwalker record covers.
`--adapter-backend internal` is exclusively machine-code-only recovery;
`--adapter-backend serwalker` pins Serwalker and never silently falls back.

An exact registry record must authorize the snapshot hash, kind, architecture and
normalized feature tuple, plus the parser family, profile and executable digests.
Merely placing a recipe in `data/serwalker_profiles` does not authorize parsing.
Each record also lists the backends it serves, and Serwalker records list only
`serwalker`, so `--adapter-backend blutter` or `r2-flutter` is refused for them before
anything runs.

## Shipped records and packaging

`adapters/registry.json` carries a Serwalker record, `trust_tier: experimental`, for
each exact identity below. Each pins its checked-in profile by digest:

| snapshot hash | SDK alias | profile | evidence |
| --- | --- | --- | --- |
| `78da37fed6bf1489361a312568249f3f` | Dart 3.11.1 | `profile_78da37fe_compr_arm64.json` | the 3.11.1 snapshot hash; a Flutter release arm64 build |
| `80a49c7111088100a233b2ae788e1f48` | Dart 3.5.0 | `profile_80a49c71_compr_arm64.json` | the LocalSend 1.17.0 fixture and its real-snapshot tests |

Neither has been compared against an SDK-matched reference yet, which is why both
stay experimental.

The checked-in records do not pin the executable: those bytes only exist once a
package is built, and they differ per host and toolchain. Packaging fills that in.
`scripts/stage-release-prefix.sh` (release archives) and the Nix package run the
`bundle_registry` example, which copies the built executable to
`share/flutterdec/artifacts/flutterdec-serwalker` and writes its size and SHA-256 as
the host variant of every Serwalker record in the staged registry. `adapter install`
publishes that bundled file into the store:

```sh
flutterdec info /path/to/libapp.so --json      # read snapshot_hash
flutterdec adapter install --dart-hash 78da37fed6bf1489361a312568249f3f
flutterdec decompile /path/to/libapp.so -o out  # auto runs Serwalker
```

A plain `cargo build` checkout has no bundled executable, so `adapter list` reports
its Serwalker records `incompatible` (no artifact variant for this host). To run
the bundled path from source, stage a prefix and use the CLI inside it:

```sh
cargo build -p flutterdec-cli -p flutterdec-serwalker --release --bins \
  --example bundle_registry
scripts/stage-release-prefix.sh target/release/flutterdec /tmp/flutterdec-prefix
/tmp/flutterdec-prefix/bin/flutterdec adapter install --dart-hash FULL_HASH
```

Rebuilding the adapter changes its digest: stage again and reinstall.

The same holds for every flutterdec upgrade. Each release archive and each Nix
rebuild ships a Serwalker build with a new digest. Until
`flutterdec adapter install --dart-hash <hash>` runs again for each Serwalker hash
that was installed, `adapter list` reports it `corrupt` (exit 2) and `info` and
`decompile` refuse it with `registry_artifact_rejected`. An install of
`80a49c7111088100a233b2ae788e1f48` made by an earlier release, which served that
hash with the reference producer, needs the same reinstall.

## Local experimental setup

For a snapshot no shipped record covers, prepare a new isolated data directory for
an explicitly chosen real snapshot, profile and local adapter build. This does not
overwrite the production registry or claim SDK-reference validation; the generated
record is experimental.

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

The Nix package installs the same preparation program as
`flutterdec-prepare-adapter`; its arguments are unchanged from the example command
above. Do not promote an experimental record without real-fixture and SDK-matched
reference comparisons. Multiple exact records can share an executable and profile.

## Real snapshot regression tests

Two tests parse a real FullAOT snapshot and stay `#[ignore]` in a normal
`cargo test` run because the fixture is an external app binary. Fetch the pinned
LocalSend 1.17.0 arm64 fixture and run both tests with:

```sh
libapp="$(./scripts/fetch-real-snapshot.sh)"
export SERWALKER_TEST_LIBAPP="$libapp"
export SERWALKER_TEST_PROFILE="$PWD/data/serwalker_profiles/profile_80a49c71_compr_arm64.json"

cargo test -p flutterdec-serwalker --test protocol_adapter -- --ignored
cargo test -p flutterdec-serwalker --lib \
  v4_real_snapshot_round_trips_and_passes_host_validation -- --ignored
```

The `Real Snapshot` CI job runs the same fixture on Linux. The download script
verifies both the APK and the extracted `libapp.so` digest, so a changed release
cannot silently stand in for the profile's snapshot.

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

use anyhow::{Context, Result};
use clap::Parser;
use flutterdec_adapter::model::MODEL_VERSION;
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::PROTOCOL_MAJOR;
use flutterdec_adapter::registry::{
    canonical_feature_fingerprint, CompatibilityRecord, CompatibilityRegistry,
};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Prepare an isolated experimental Serwalker registry for one real snapshot")]
struct Arguments {
    #[arg(long)]
    libapp: PathBuf,
    #[arg(long)]
    profile: PathBuf,
    #[arg(long)]
    artifact: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let bundle = flutterdec_loader::load_snapshot_bundle(&arguments.libapp)?;
    let key = bundle.identity.exact_selection_key()?;
    let profile = fs::read(&arguments.profile).context("read selected profile")?;
    let profile_digest = Sha256Digest::of(&profile);
    flutterdec_loader::dart_profile::load_profile_artifact(
        &arguments.profile,
        "local-serwalker-profile",
        profile_digest.as_str(),
        Vec::new(),
    )
    .map_err(anyhow::Error::msg)?;
    let artifact = fs::read(&arguments.artifact).context("read adapter executable")?;
    let record: CompatibilityRecord = serde_json::from_value(serde_json::json!({
        "snapshot_hash": key.hash,
        "snapshot_kind": bundle.identity.kind,
        "target_arch": key.target_arch,
        "features": key.features,
        "feature_fingerprint": canonical_feature_fingerprint(&key.features),
        "parser_family": {"id": "flutterdec-serwalker", "version": env!("CARGO_PKG_VERSION")},
        "profile": {"id": "local-serwalker-profile", "path": "data/profile.json", "sha256": profile_digest},
        "artifact": {"id": "flutterdec-serwalker", "variants": [{
            "host_os": std::env::consts::OS,
            "host_arch": std::env::consts::ARCH,
            "path": "artifacts/flutterdec-serwalker",
            "size": artifact.len(),
            "sha256": Sha256Digest::of(&artifact),
            "provenance": "operator-selected local build"
        }]},
        "evidence": {
            "source": "operator-selected local fixture",
            "provenance": "experimental; not a production compatibility admission",
            "references": [format!("libapp SHA-256: {}", Sha256Digest::of(&fs::read(&arguments.libapp)?))]
        },
        "trust_tier": "experimental", "protocol_major": PROTOCOL_MAJOR, "model_major": MODEL_VERSION
    }))?;
    let registry = CompatibilityRegistry {
        version: 1,
        records: vec![record],
    };
    registry.validate()?;
    fs::create_dir(&arguments.output).context("output must be a new directory")?;
    fs::create_dir(arguments.output.join("adapters"))?;
    fs::create_dir(arguments.output.join("data"))?;
    fs::write(arguments.output.join("data/profile.json"), profile)?;
    fs::write(
        arguments.output.join("adapters/registry.json"),
        serde_json::to_vec_pretty(&registry)?,
    )?;
    println!(
        "data directory: {}",
        arguments.output.canonicalize()?.display()
    );
    println!("snapshot hash: {}", key.hash);
    println!(
        "artifact source: {}",
        arguments.artifact.canonicalize()?.display()
    );
    println!("Experimental local record only. Validate against an SDK-matched reference before publishing compatibility claims.");
    Ok(())
}

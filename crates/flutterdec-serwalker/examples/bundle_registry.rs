//! Bundle a built Serwalker executable into a staged package data directory.
//!
//! The checked-in registry authorizes Serwalker for exact snapshot identities
//! and pins each profile, but it cannot pin the executable: those bytes only
//! exist once a package is built. Packaging runs this once per host build. It
//! copies the executable to `<data-dir>/artifacts/flutterdec-serwalker`, where
//! `flutterdec adapter install` looks for a bundled artifact, and declares its
//! size and digest as the host variant of every Serwalker record.
use anyhow::{bail, Context, Result};
use clap::Parser;
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::registry::{CompatibilityRegistry, HostArtifactVariant};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const ARTIFACT_ID: &str = "flutterdec-serwalker";
const ARTIFACT_PATH: &str = "artifacts/flutterdec-serwalker";

#[derive(Parser)]
#[command(about = "Bundle a built Serwalker executable into a staged flutterdec data directory")]
struct Arguments {
    /// Staged package data directory holding adapters/registry.json
    #[arg(long)]
    data_dir: PathBuf,
    /// The flutterdec-serwalker executable this package ships
    #[arg(long)]
    artifact: PathBuf,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let registry_path = arguments.data_dir.join("adapters/registry.json");
    let mut registry = CompatibilityRegistry::load(&registry_path)
        .with_context(|| format!("load {}", registry_path.display()))?;
    let bytes = fs::read(&arguments.artifact)
        .with_context(|| format!("read {}", arguments.artifact.display()))?;
    let version = env!("CARGO_PKG_VERSION");
    let variant = HostArtifactVariant {
        host_os: std::env::consts::OS.to_string(),
        host_arch: std::env::consts::ARCH.to_string(),
        path: ARTIFACT_PATH.to_string(),
        size: bytes.len() as u64,
        sha256: Sha256Digest::of(&bytes).to_string(),
        provenance: format!("bundled with flutterdec {version}"),
    };

    let mut bundled = 0;
    for record in registry
        .records
        .iter_mut()
        .filter(|record| record.artifact.id == ARTIFACT_ID)
    {
        // A package holds one build per host, so its variant replaces rather
        // than joins whatever the source registry declared.
        record.parser_family.version = Some(version.to_string());
        record.artifact.variants = vec![variant.clone()];
        bundled += 1;
    }
    if bundled == 0 {
        bail!(
            "{} declares no {ARTIFACT_ID} record to bundle",
            registry_path.display()
        );
    }
    registry.validate()?;

    let destination = arguments.data_dir.join(ARTIFACT_PATH);
    fs::create_dir_all(destination.parent().expect("artifact path has a parent"))?;
    fs::write(&destination, &bytes).with_context(|| format!("write {}", destination.display()))?;
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
    let mut document = serde_json::to_vec_pretty(&registry)?;
    document.push(b'\n');
    fs::write(&registry_path, document)
        .with_context(|| format!("write {}", registry_path.display()))?;

    println!(
        "bundled {} ({} bytes, sha256 {}) for {}/{} into {bundled} record(s)",
        destination.display(),
        variant.size,
        variant.sha256,
        variant.host_os,
        variant.host_arch
    );
    Ok(())
}

use clap::Parser;
use flutterdec_adapter::model::{InputRegionName, ProgramModel};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::{
    AdapterError, AdapterErrorCode, AdapterRequest, AdapterResult, AdapterStatus, BackendId,
    ProtocolError, RequestedBackend, PROFILE_PATH,
};
use flutterdec_adapter::{AdapterRegionInput, Limits};
use flutterdec_loader::identity::SnapshotIdentity;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version, about = "Serwalker snapshot parser adapter (protocol v1)")]
struct Arguments {
    #[arg(long)]
    request: PathBuf,
    #[arg(long)]
    result: PathBuf,
    #[arg(long)]
    input_path: Option<PathBuf>,
    #[arg(long)]
    libapp_path: Option<PathBuf>,
}

fn error(code: AdapterErrorCode, message: impl Into<String>) -> AdapterError {
    AdapterError {
        code,
        message: message.into(),
    }
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "{} is not a regular file within the {limit} byte limit",
                path.display()
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "input grew past its byte limit",
        ));
    }
    Ok(bytes)
}

fn produce(request: &AdapterRequest) -> Result<ProgramModel, AdapterError> {
    let expected = request
        .identity
        .exact_selection_key()
        .map_err(|cause| error(AdapterErrorCode::UnsupportedSnapshot, cause.to_string()))?;
    if !matches!(
        request.requested_backend,
        RequestedBackend::Auto | RequestedBackend::Fixed(BackendId::Serwalker)
    ) {
        return Err(error(
            AdapterErrorCode::UnsupportedSnapshot,
            format!(
                "Serwalker cannot serve backend {}",
                request.requested_backend
            ),
        ));
    }
    let limit = Limits::default().max_region_bytes;
    let mut bytes = Vec::with_capacity(request.inputs.len());
    for handle in &request.inputs {
        let data = read_bounded(Path::new(handle.path.as_str()), limit).map_err(|cause| {
            error(
                AdapterErrorCode::InputMissing,
                format!("read {}: {cause}", handle.region),
            )
        })?;
        if data.len() as u64 != handle.size || Sha256Digest::of(&data) != handle.sha256 {
            return Err(error(
                AdapterErrorCode::InputDigestMismatch,
                format!(
                    "{} does not match its declared size and digest",
                    handle.region
                ),
            ));
        }
        bytes.push(data);
    }
    let regions: Vec<_> = request
        .inputs
        .iter()
        .zip(&bytes)
        .map(|(handle, bytes)| AdapterRegionInput {
            region: handle.region,
            bytes,
            virtual_address: handle.virtual_address,
        })
        .collect();
    let data = regions
        .iter()
        .find(|region| region.region == InputRegionName::IsolateData)
        .ok_or_else(|| error(AdapterErrorCode::InputMissing, "request omits isolate data"))?;
    let (hash, kind, features) =
        flutterdec_loader::parse_snapshot_header(data.bytes).ok_or_else(|| {
            error(
                AdapterErrorCode::IdentityMismatch,
                "isolate snapshot header is invalid",
            )
        })?;
    let observed =
        SnapshotIdentity::from_header(request.identity.target_arch.clone(), &hash, kind, &features);
    if observed.exact_selection_key().ok().as_ref() != Some(&expected) {
        return Err(error(
            AdapterErrorCode::IdentityMismatch,
            "isolate snapshot header does not match the host-selected identity",
        ));
    }
    let profile_path = Path::new(PROFILE_PATH);
    let profile = read_bounded(
        profile_path,
        flutterdec_loader::dart_profile::MAX_PROFILE_BYTES,
    )
    .map_err(|cause| {
        error(
            AdapterErrorCode::InputMissing,
            format!("read authorized profile: {cause}"),
        )
    })?;
    if Sha256Digest::of(&profile) != request.compatibility.profile_sha256 {
        return Err(error(
            AdapterErrorCode::InputDigestMismatch,
            "authorized profile digest mismatch",
        ));
    }
    flutterdec_serwalker::walk_snapshot(&flutterdec_serwalker::SnapshotInput {
        identity: &request.identity,
        regions: &regions,
        producer: &request.producer,
        compatibility: Some(&request.compatibility),
        profile_path,
        profile_sha256: &request.compatibility.profile_sha256,
    })
    .map_err(|cause| error(AdapterErrorCode::ParseFailed, format!("{cause:#}")))
}

fn run(arguments: &Arguments) -> AdapterResult {
    let bytes = match read_bounded(&arguments.request, 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(cause) => {
            return AdapterResult::failed(AdapterErrorCode::InputMissing, cause.to_string())
        }
    };
    let request = match AdapterRequest::from_json(&bytes) {
        Ok(request) => request,
        Err(cause) => {
            let code = match cause {
                ProtocolError::UnsupportedModelMajor(_) => {
                    AdapterErrorCode::UnsupportedModelVersion
                }
                _ => AdapterErrorCode::UnsupportedProtocol,
            };
            return AdapterResult::unsupported(code, cause.to_string());
        }
    };
    let model = match produce(&request) {
        Ok(model) => model,
        Err(cause) => {
            return if matches!(cause.code, AdapterErrorCode::UnsupportedSnapshot) {
                AdapterResult::unsupported(cause.code, cause.message)
            } else {
                AdapterResult::failed(cause.code, cause.message)
            };
        }
    };
    let output = match serde_json::to_vec(&model) {
        Ok(bytes) => bytes,
        Err(cause) => return AdapterResult::failed(AdapterErrorCode::Internal, cause.to_string()),
    };
    if let Err(cause) = fs::write(request.output.as_str(), output) {
        return AdapterResult::failed(AdapterErrorCode::OutputWriteFailed, cause.to_string());
    }
    AdapterResult::ok(request.output, BackendId::Serwalker, None, Vec::new())
}

fn main() -> std::process::ExitCode {
    let arguments = Arguments::parse();
    let result = run(&arguments);
    if let Err(cause) = fs::write(&arguments.result, result.to_json()) {
        eprintln!("write adapter result: {cause}");
        return std::process::ExitCode::from(2);
    }
    if result.status == AdapterStatus::Ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

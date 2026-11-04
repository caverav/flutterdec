mod constants;
mod utils;

mod cluster;
mod instruction_table;
mod program_roots;

mod raw_object;
mod snapshot;

mod info_producer;
mod stream;

mod profile_reader;

use anyhow::Context;
use flutterdec_adapter::model::{CompatibilityBinding, InputRegionName, Producer, ProgramModel};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::{AdapterInput, AdapterRegionInput};
use flutterdec_loader::identity::SnapshotIdentity;
use std::path::Path;

use crate::utils::get_snapshot_region;
use crate::{
    constants::DART_3_11_1_SNAPSHOT_HASH,
    info_producer::{
        finish_model, produce_model_headers, produce_model_object_info, produce_model_object_pool,
    },
    snapshot::{parse_data_snapshot, parse_instr_snapshot},
    stream::Stream,
};

pub const SUPPORTED_SNAPSHOT_HASH: &str = DART_3_11_1_SNAPSHOT_HASH;

pub struct SnapshotInput<'a> {
    pub identity: &'a SnapshotIdentity,
    pub regions: &'a [AdapterRegionInput<'a>],
    pub producer: &'a Producer,
    pub compatibility: Option<&'a CompatibilityBinding>,
    pub profile_path: &'a Path,
    pub profile_sha256: &'a Sha256Digest,
}

pub fn profile_file_name(snapshot_hash: &str, features: &[String]) -> anyhow::Result<String> {
    profile_reader::profile_file_name(snapshot_hash, features)
}

pub fn walk_snapshot_and_produce_model(
    adapter_input: &AdapterInput,
) -> anyhow::Result<ProgramModel> {
    walk_snapshot(&SnapshotInput {
        identity: adapter_input.identity,
        regions: &adapter_input.regions,
        producer: &adapter_input.producer,
        compatibility: Some(&adapter_input.compatibility),
        profile_path: adapter_input.authorization.profile_path,
        profile_sha256: &adapter_input.compatibility.profile_sha256,
    })
}

pub fn walk_snapshot(adapter_input: &SnapshotInput<'_>) -> anyhow::Result<ProgramModel> {
    adapter_input.identity.exact_selection_key()?;
    anyhow::ensure!(
        adapter_input.identity.features.has("compressed-pointers"),
        "Serwalker currently requires a compressed-pointers snapshot"
    );
    let profile_bytes =
        std::fs::read(adapter_input.profile_path).context("read authorized Serwalker profile")?;
    anyhow::ensure!(
        Sha256Digest::of(&profile_bytes) == *adapter_input.profile_sha256,
        "Serwalker profile does not match the authorized profile digest"
    );
    let profile = profile_reader::data_profile_from_bytes(
        &profile_bytes,
        &adapter_input.identity.features.normalized,
    )?;
    let mut program_model = produce_model_headers(adapter_input)?;
    let input_regions = adapter_input.regions;

    let isolate_data_region = get_snapshot_region(input_regions, InputRegionName::IsolateData)?;
    let mut isolate_data_stream = Stream::new(isolate_data_region.bytes);

    let isolate_instr_region =
        get_snapshot_region(input_regions, InputRegionName::IsolateInstructions)?;
    let mut isolate_instr_stream = Stream::new(isolate_instr_region.bytes);

    let mut isolate_data_snapshot = parse_data_snapshot(&mut isolate_data_stream, Some(profile))?;
    let mut isolate_instr_snapshot =
        parse_instr_snapshot(&mut isolate_instr_stream, &mut isolate_data_snapshot)?;
    // parse_instr_snapshot needs to run after parse_data_snapshot, given resolve_entrypoints must have been executed already
    // in order to call resolve_instructions_len_for_code_objects

    // manually set it before passing the instructions produce_model_object_info
    // the core already determined it for us

    //TO-DO: add vm snapshot deserialization
    isolate_instr_snapshot.image_va = isolate_instr_region.virtual_address.ok_or_else(|| {
        anyhow::anyhow!("Core didn't provide a virtual address for the instruction image!")
    })?;

    produce_model_object_info(
        &mut program_model,
        &isolate_data_snapshot,
        &isolate_instr_snapshot,
    )?;
    produce_model_object_pool(
        &mut program_model,
        &isolate_data_snapshot,
        &isolate_instr_snapshot,
    )?;
    finish_model(&mut program_model);

    flutterdec_adapter::validate::validate(
        &program_model,
        &flutterdec_adapter::validate::HostSelectedContext {
            identity: adapter_input.identity.clone(),
            producer: adapter_input.producer.clone(),
            compatibility: adapter_input.compatibility.cloned(),
            regions: program_model.input.regions.clone(),
        },
    )?;

    Ok(program_model)
}

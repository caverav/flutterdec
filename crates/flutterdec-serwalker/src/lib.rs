#![allow(dead_code)]

mod constants;
pub mod utils;

mod cluster;
mod instruction_table;
mod program_roots;

mod raw_object;
mod snapshot;

pub mod info_producer;
mod stream;

use std::collections::BTreeMap;

use flutterdec_adapter::model::{
    Capabilities, CapabilityLevel, InputRegion, InputRegionName, ObservedInput, Producer,
    ProducerTrust, ProgramModel,
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_loader::identity::SnapshotIdentity;

use crate::{
    constants::DART_3_11_1_SNAPSHOT_HASH,
    info_producer::{produce_model_object_info, produce_model_object_pool},
    snapshot::{parse_data_snapshot, parse_instr_snapshot},
    stream::Stream,
};

pub const SUPPORTED_SNAPSHOT_HASH: &str = DART_3_11_1_SNAPSHOT_HASH;

/// Deserialize a Dart AOT snapshot into a ProgramModel v4.
pub fn walk_snapshot_and_produce_model(
    isolate_data: &[u8],
    isolate_instr: &[u8],
    isolate_instr_va: u64,
    identity: &SnapshotIdentity,
) -> anyhow::Result<ProgramModel> {
    let mut isolate_data_stream = Stream::new(isolate_data);
    let mut isolate_instr_stream = Stream::new(isolate_instr);

    let mut isolate_data_snapshot = parse_data_snapshot(&mut isolate_data_stream)?;
    let mut isolate_instr_snapshot = parse_instr_snapshot(
        &mut isolate_instr_stream,
        &mut isolate_data_snapshot.clusters,
    )?;

    // parse_instr_snapshot needs to run after parse_data_snapshot, given
    // resolve_entrypoints must have been executed already in order to call
    // resolve_instructions_len_for_code_objects
    isolate_instr_snapshot.image_va = isolate_instr_va;

    let (libraries, classes, functions) =
        produce_model_object_info(&isolate_data_snapshot, &isolate_instr_snapshot)?;
    let object_pool = produce_model_object_pool(&isolate_data_snapshot)?;

    let regions = vec![
        InputRegion {
            region: InputRegionName::IsolateData,
            size: isolate_data.len() as u64,
            sha256: Sha256Digest::of(isolate_data),
            virtual_address: None,
            executable: false,
        },
        InputRegion {
            region: InputRegionName::IsolateInstructions,
            size: isolate_instr.len() as u64,
            sha256: Sha256Digest::of(isolate_instr),
            virtual_address: Some(isolate_instr_va),
            executable: true,
        },
    ];

    let capabilities = Capabilities {
        libraries: if libraries.is_empty() {
            CapabilityLevel::Unavailable
        } else {
            CapabilityLevel::Complete
        },
        classes: if classes.is_empty() {
            CapabilityLevel::Unavailable
        } else {
            CapabilityLevel::Complete
        },
        class_relationships: if classes.iter().any(|c| c.super_class.is_some()) {
            CapabilityLevel::Complete
        } else {
            CapabilityLevel::Unavailable
        },
        functions: if functions.is_empty() {
            CapabilityLevel::Unavailable
        } else {
            CapabilityLevel::Complete
        },
        function_names: if functions.iter().any(|f| f.name.is_some()) {
            CapabilityLevel::Complete
        } else {
            CapabilityLevel::Unavailable
        },
        object_pool: if object_pool.entries.is_empty() {
            CapabilityLevel::Unavailable
        } else {
            CapabilityLevel::Complete
        },
        pool_index_space: if object_pool.geometry.is_some() {
            CapabilityLevel::Complete
        } else {
            CapabilityLevel::Unavailable
        },
    };

    let model = ProgramModel {
        model_version: flutterdec_adapter::model::MODEL_VERSION,
        producer: Producer {
            id: "flutterdec-serwalker".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            artifact_sha256: Sha256Digest::of(b"flutterdec-serwalker"),
            trust: ProducerTrust::Local,
        },
        input: ObservedInput {
            identity: identity.clone(),
            regions,
        },
        compatibility: None,
        capabilities,
        libraries,
        classes,
        functions,
        object_pool,
        diagnostics: Vec::new(),
        extensions: BTreeMap::new(),
    };

    Ok(model)
}

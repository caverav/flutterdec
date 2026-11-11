mod classes_info;
mod functions_info;
mod libraries_info;
mod object_pool_info;
mod utils;

use crate::SnapshotInput;
use classes_info::produce_classes_info;
use flutterdec_adapter::model::{
    Capabilities, CapabilityLevel, Diagnostic, DiagnosticCode, DiagnosticSeverity, Domain,
    InputRegion, ObjectPool as ModelObjectPool, ObservedInput, PoolIndexSpace, ProgramModel,
    MODEL_VERSION,
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::AdapterRegionInput;
use functions_info::produce_functions_info;
use libraries_info::produce_libraries_info;

use crate::{
    info_producer::{object_pool_info::produce_model_object_pool_info, utils::find_object_by_id},
    raw_object::ObjectPool,
    snapshot::{DataSnapshot, InstructionsSnapshot},
};

pub(super) fn observed_regions(
    regions: &[AdapterRegionInput<'_>],
) -> anyhow::Result<Vec<InputRegion>> {
    let mut observed = Vec::with_capacity(regions.len());
    for region in regions {
        anyhow::ensure!(
            region.region.is_executable() == region.virtual_address.is_some(),
            "region {} has an invalid virtual address",
            region.region
        );
        observed.push(InputRegion {
            region: region.region,
            size: u64::try_from(region.bytes.len())?,
            sha256: Sha256Digest::of(region.bytes),
            virtual_address: region.virtual_address,
            executable: region.region.is_executable(),
        });
    }
    observed.sort_by_key(|region| region.region);
    anyhow::ensure!(
        !observed
            .windows(2)
            .any(|pair| pair[0].region == pair[1].region),
        "duplicate input region"
    );
    Ok(observed)
}

pub fn produce_model_headers(input: &SnapshotInput<'_>) -> anyhow::Result<ProgramModel> {
    Ok(ProgramModel {
        model_version: MODEL_VERSION,
        producer: input.producer.clone(),
        input: ObservedInput {
            identity: input.identity.clone(),
            regions: observed_regions(input.regions)?,
        },
        compatibility: input.compatibility.cloned(),
        capabilities: Capabilities::all_unavailable(),
        libraries: Vec::new(),
        classes: Vec::new(),
        functions: Vec::new(),
        object_pool: ModelObjectPool::unavailable(),
        diagnostics: Vec::new(),
        extensions: std::collections::BTreeMap::from([(
            "serwalker.profile".into(),
            serde_json::json!({"file_name": input.profile_path.file_name().map(|name| name.to_string_lossy()),
                               "sha256": input.profile_sha256}),
        )]),
    })
}

pub fn produce_model_object_info(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
) -> anyhow::Result<()> {
    produce_libraries_info(snapshot, model)?;
    produce_classes_info(snapshot, model)?;
    produce_functions_info(snapshot, instructions, model)?; // has to go AFTER produce_classes_info, given it depends on model.classes to find the ClassId for Function owners

    Ok(())
}

pub fn produce_model_object_pool(
    model: &mut ProgramModel,
    snapshot: &DataSnapshot,
    instructions: &InstructionsSnapshot,
) -> anyhow::Result<()> {
    let global_object_pool_ref_id = snapshot.roots.object_store().global_object_pool()?; // here using any Function object object pool field could work as well
    let global_object_pool = find_object_by_id::<ObjectPool>(snapshot, global_object_pool_ref_id)?;

    produce_model_object_pool_info(snapshot, instructions, global_object_pool, model)?;
    Ok(())
}

pub fn finish_model(model: &mut ProgramModel) {
    use CapabilityLevel::{Complete, Partial, Unavailable};
    let recovered = |present| if present { Partial } else { Unavailable };
    model.capabilities = Capabilities {
        libraries: recovered(!model.libraries.is_empty()),
        classes: recovered(!model.classes.is_empty()),
        class_relationships: recovered(
            model
                .classes
                .iter()
                .any(|class| class.super_class.is_some()),
        ),
        functions: recovered(!model.functions.is_empty()),
        function_names: recovered(
            model
                .functions
                .iter()
                .any(|function| function.name.is_some()),
        ),
        object_pool: recovered(!model.object_pool.entries.is_empty()),
        pool_index_space: if model.object_pool.index_space == PoolIndexSpace::Hardware {
            Complete
        } else {
            Unavailable
        },
    };
    for domain in Domain::ALL {
        match model.capabilities.level(domain) {
            Unavailable => model.diagnostics.push(Diagnostic::unavailable(domain,
                "Serwalker did not recover records for this domain from the root program snapshot")),
            Partial => model.diagnostics.push(Diagnostic {
                code: DiagnosticCode::DomainPartiallyRecovered,
                severity: DiagnosticSeverity::Info,
                subject: Some(domain.to_string()),
                message: "Serwalker reports recovered root-unit records; VM/base objects and deferred units are not resolved. Unresolved references are retained in serwalker.* extensions.".into(),
            }),
            Complete => {}
        }
    }
}

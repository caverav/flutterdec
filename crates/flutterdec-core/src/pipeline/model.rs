#[derive(Debug, Clone)]
struct LoadedModel {
    model: ProgramModel,
    adapter_exec: PathBuf,
    manifest_entry_version: Option<String>,
    manifest_entry_adapter: Option<String>,
}

fn load_model(
    repo_root: &Path,
    bundle: &SnapshotBundle,
    backend: AdapterBackend,
) -> Result<LoadedModel> {
    let manifest = flutterdec_adapter::load_manifest(repo_root)?;
    let manifest_entry = manifest
        .entries
        .iter()
        .find(|entry| entry.snapshot_hash == bundle.snapshot_hash);
    let adapter_exec = resolve_adapter_exec(repo_root, &bundle.snapshot_hash)?;
    let model = run_adapter(
        &adapter_exec,
        &AdapterInput {
            input_path: Some(&bundle.input_path),
            libapp_path: Some(&bundle.libapp_path),
            vm_data: &bundle.vm_data,
            isolate_data: &bundle.isolate_data,
            vm_instr: &bundle.vm_instr,
            isolate_instr: &bundle.isolate_instr,
            vm_instr_va: bundle.vm_instr_va,
            isolate_instr_va: bundle.isolate_instr_va,
            backend: Some(backend.as_str()),
        },
    )?;
    Ok(LoadedModel {
        model,
        adapter_exec,
        manifest_entry_version: manifest_entry.map(|entry| entry.version.clone()),
        manifest_entry_adapter: manifest_entry.map(|entry| entry.adapter.clone()),
    })
}

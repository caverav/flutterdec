fn load_model(
    repo_root: &Path,
    bundle: &SnapshotBundle,
    backend: AdapterBackend,
) -> Result<ProgramModel> {
    let adapter_exec = resolve_adapter_exec(repo_root, &bundle.snapshot_hash)?;
    run_adapter(
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
    )
}

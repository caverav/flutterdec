pub fn run_info(repo_root: &Path, input_path: &Path) -> Result<InfoOutput> {
    let bundle = load_snapshot_bundle(input_path)?;
    let adapter_installed = resolve_adapter_exec(repo_root, &bundle.snapshot_hash).is_ok();

    let mut out = InfoOutput {
        input_path: bundle.input_path.display().to_string(),
        libapp_path: bundle.libapp_path.display().to_string(),
        arch: bundle.arch.clone(),
        snapshot_hash: bundle.snapshot_hash.clone(),
        adapter_installed,
        function_count: None,
        class_count: None,
        object_pool_count: None,
    };

    if adapter_installed {
        if let Ok(model) = load_model(repo_root, &bundle) {
            out.function_count = Some(model.functions.len());
            out.class_count = Some(model.classes.len());
            out.object_pool_count = Some(model.object_pool.len());
        }
    }

    Ok(out)
}

pub fn run_decompile(
    repo_root: &Path,
    input_path: &Path,
    opt: &DecompileOptions,
) -> Result<QualityReport> {
    let bundle = load_snapshot_bundle(input_path)?;
    let model = load_model(repo_root, &bundle)?;

    if model.arch != "arm64" {
        bail!("model arch {} unsupported in v1", model.arch);
    }

    let disasm = disassemble_program(
        &model,
        &bundle.isolate_instr,
        bundle.isolate_instr_va,
        opt.focus.as_deref(),
        opt.max_functions,
    );
    let ir: Vec<FunctionIr> = build_program_ir(&disasm);
    let mut symbol_names: HashMap<u64, String> = HashMap::new();
    for f in &model.functions {
        symbol_names.insert(f.entry_va, f.name.clone());
    }
    for f in &disasm {
        symbol_names
            .entry(f.entry_va)
            .or_insert_with(|| f.function_name.clone());
    }
    let pseudo = emit_program(&ir, &symbol_names);

    let asm_dir = opt.out_dir.join("asm");
    let ir_dir = opt.out_dir.join("ir");
    let pseudo_dir = opt.out_dir.join("pseudocode");
    fs::create_dir_all(&pseudo_dir).context("create pseudocode out dir")?;
    if opt.emit_asm {
        fs::create_dir_all(&asm_dir)?;
    }
    if opt.emit_ir {
        fs::create_dir_all(&ir_dir)?;
    }

    for p in &pseudo {
        let filename = format!(
            "{:05}_{}.dartpseudo",
            p.function_id,
            normalize_file_name(&p.function_name)
        );
        fs::write(pseudo_dir.join(filename), &p.source)?;
    }

    if opt.emit_asm {
        for f in &disasm {
            let mut lines = Vec::new();
            for i in &f.instructions {
                let mut line = format!("0x{:x}: {}", i.va, i.mnemonic);
                if !i.op_str.is_empty() {
                    line.push(' ');
                    line.push_str(&i.op_str);
                }
                if !i.annotation.is_empty() {
                    line.push_str(" ; ");
                    line.push_str(&i.annotation);
                }
                lines.push(line);
            }
            let filename = format!(
                "{:05}_{}.s",
                f.function_id,
                normalize_file_name(&f.function_name)
            );
            fs::write(asm_dir.join(filename), lines.join("\n"))?;
        }
    }

    if opt.emit_ir {
        for f in &ir {
            let filename = format!("{:05}_{}.json", f.function_id, normalize_file_name(&f.name));
            fs::write(ir_dir.join(filename), serde_json::to_vec_pretty(f)?)?;
        }
    }

    let report = quality_from_artifacts(&model, &disasm, &pseudo, opt);
    fs::create_dir_all(&opt.out_dir)?;

    let quality_path = opt.out_dir.join("quality.json");
    fs::write(&quality_path, serde_json::to_vec_pretty(&report)?)?;

    let summary = json!({
        "input": bundle.input_path,
        "libapp": bundle.libapp_path,
        "arch": bundle.arch,
        "snapshot_hash": bundle.snapshot_hash,
        "adapter_kind": model.adapter_kind,
        "dart_version": model.dart_version,
        "counts": {
            "libraries": model.libraries.len(),
            "classes": model.classes.len(),
            "functions": model.functions.len(),
            "object_pool": model.object_pool.len(),
            "disassembled_functions": disasm.len()
        },
        "quality": report,
    });

    fs::write(
        opt.out_dir.join("report.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;

    if !report.passed {
        bail!("quality gate failed. see {}", quality_path.display());
    }

    Ok(report)
}

pub fn available_adapters(repo_root: &Path) -> Result<Vec<(String, String, String, bool)>> {
    let entries = list_adapters(repo_root)?;
    Ok(entries
        .into_iter()
        .map(|(e, installed)| (e.snapshot_hash, e.version, e.adapter, installed))
        .collect())
}

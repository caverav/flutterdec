use std::collections::BTreeSet;

fn function_descriptor(
    func: &flutterdec_adapter::FunctionInfo,
    class_to_library: &HashMap<String, String>,
) -> String {
    let raw_library_uri = class_to_library
        .get(&func.owner_class)
        .map(String::as_str)
        .unwrap_or("");
    let library_uri = canonicalize_library_uri_for_diff(raw_library_uri);
    if library_uri.is_empty() {
        return format!("{}::{}", func.owner_class, func.name);
    }
    format!("{}::{}::{}", library_uri, func.owner_class, func.name)
}

fn canonicalize_library_uri_for_diff(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(marker_index) = trimmed.find("/.dart_tool/flutter_build/") {
        let suffix = &trimmed[(marker_index + 1)..];
        return format!("file:///{suffix}");
    }
    trimmed.to_string()
}

fn collect_function_descriptors(model: &ProgramModel) -> BTreeSet<String> {
    let class_to_library = build_class_library_lookup(model);
    model
        .functions
        .iter()
        .map(|func| function_descriptor(func, &class_to_library))
        .collect::<BTreeSet<_>>()
}

fn descriptor_library_uri(descriptor: &str) -> Option<&str> {
    let (library_uri, _) = descriptor.split_once("::")?;
    if library_uri.is_empty() {
        None
    } else {
        Some(library_uri)
    }
}

fn diff_package_bucket_for_library_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    if trimmed.starts_with("file://") {
        return "file".to_string();
    }
    priority_package_from_library_uri(trimmed)
}

fn collect_diff_package_counts(descriptors: &[String]) -> Vec<PackageCount> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for descriptor in descriptors {
        let Some(uri) = descriptor_library_uri(descriptor) else {
            *counts.entry("unknown".to_string()).or_insert(0) += 1;
            continue;
        };
        let key = diff_package_bucket_for_library_uri(uri);
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut items = counts
        .into_iter()
        .map(|(package, functions)| PackageCount { package, functions })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.functions
            .cmp(&a.functions)
            .then_with(|| a.package.cmp(&b.package))
    });
    items
}

pub fn run_diff(
    repo_root: &Path,
    old_input_path: &Path,
    new_input_path: &Path,
    opt: &DiffOptions,
) -> Result<DiffReport> {
    let old_bundle = load_snapshot_bundle(old_input_path)?;
    let new_bundle = load_snapshot_bundle(new_input_path)?;

    let old_loaded = load_model(repo_root, &old_bundle, opt.adapter_backend)?;
    let new_loaded = load_model(repo_root, &new_bundle, opt.adapter_backend)?;
    let old_snapshot_hash_match = enforce_snapshot_hash_match(
        opt.require_snapshot_hash_match,
        "old input",
        &old_bundle.snapshot_hash,
        &old_loaded.model.snapshot_hash,
    )?;
    let new_snapshot_hash_match = enforce_snapshot_hash_match(
        opt.require_snapshot_hash_match,
        "new input",
        &new_bundle.snapshot_hash,
        &new_loaded.model.snapshot_hash,
    )?;
    let old_model = old_loaded.model;
    let new_model = new_loaded.model;

    let (old_scoped, _) =
        apply_function_scope_filter(&old_model, opt.function_scope, &opt.app_packages);
    let (new_scoped, _) =
        apply_function_scope_filter(&new_model, opt.function_scope, &opt.app_packages);

    let old_descriptors = collect_function_descriptors(&old_scoped);
    let new_descriptors = collect_function_descriptors(&new_scoped);

    let added = new_descriptors
        .difference(&old_descriptors)
        .cloned()
        .collect::<Vec<_>>();
    let removed = old_descriptors
        .difference(&new_descriptors)
        .cloned()
        .collect::<Vec<_>>();
    let common_count = old_descriptors.intersection(&new_descriptors).count();

    fs::create_dir_all(&opt.out_dir)?;
    let report_path = opt.out_dir.join("diff_report.json");

    let report = DiffReport {
        old_input_path: old_bundle.input_path.display().to_string(),
        new_input_path: new_bundle.input_path.display().to_string(),
        old_snapshot_hash: old_bundle.snapshot_hash,
        new_snapshot_hash: new_bundle.snapshot_hash,
        old_snapshot_hash_match,
        new_snapshot_hash_match,
        require_snapshot_hash_match: opt.require_snapshot_hash_match,
        old_dart_version: old_model.dart_version,
        new_dart_version: new_model.dart_version,
        function_scope: opt.function_scope.as_str().to_string(),
        app_packages: opt.app_packages.clone(),
        old_function_count: old_descriptors.len(),
        new_function_count: new_descriptors.len(),
        common_function_count: common_count,
        added_function_count: added.len(),
        removed_function_count: removed.len(),
        added_functions_top: added.iter().take(200).cloned().collect::<Vec<_>>(),
        removed_functions_top: removed.iter().take(200).cloned().collect::<Vec<_>>(),
        added_packages_top: collect_diff_package_counts(&added)
            .into_iter()
            .take(20)
            .collect::<Vec<_>>(),
        removed_packages_top: collect_diff_package_counts(&removed)
            .into_iter()
            .take(20)
            .collect::<Vec<_>>(),
        report_path: report_path.display().to_string(),
    };
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

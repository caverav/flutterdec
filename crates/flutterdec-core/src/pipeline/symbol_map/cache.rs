fn sanitize_local_cache_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn repo_relative_display_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn resolve_manifest_target_path(repo_root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn load_local_symbol_cache_manifest(local_cache_root: &Path) -> Result<LocalSymbolCacheManifest> {
    let manifest_path = local_cache_root.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(LocalSymbolCacheManifest::default());
    }
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("read local symbol cache manifest {}", manifest_path.display()))?;
    let manifest = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parse local symbol cache manifest JSON {}",
            manifest_path.display()
        )
    })?;
    Ok(manifest)
}

fn write_local_symbol_cache_manifest(
    local_cache_root: &Path,
    manifest: &LocalSymbolCacheManifest,
) -> Result<PathBuf> {
    fs::create_dir_all(local_cache_root)
        .with_context(|| format!("create local symbol cache dir {}", local_cache_root.display()))?;
    let manifest_path = local_cache_root.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(manifest)?)
        .with_context(|| format!("write local symbol cache manifest {}", manifest_path.display()))?;
    Ok(manifest_path)
}

fn fingerprint_engine_for_local_cache(path: &Path) -> Result<EngineFingerprintReport> {
    run_engine_fingerprint(
        path,
        &EngineFingerprintOptions {
            out_dir: None,
            max_markers: 24,
        },
    )
}

fn update_local_symbol_cache_entry(
    manifest: &mut LocalSymbolCacheManifest,
    entry: LocalSymbolCacheEntry,
) {
    let build_id_lower = entry.build_id.as_deref().map(str::to_ascii_lowercase);
    let version = entry.flutter_version.clone();
    if let Some(existing) = manifest.entries.iter_mut().find(|candidate| {
        if candidate.arch != entry.arch {
            return false;
        }
        if let (Some(want), Some(have)) = (build_id_lower.as_deref(), candidate.build_id.as_deref()) {
            return have.eq_ignore_ascii_case(want);
        }
        if let (Some(want), Some(have)) = (version.as_deref(), candidate.flutter_version.as_deref()) {
            return have == want;
        }
        false
    }) {
        *existing = entry;
    } else {
        manifest.entries.push(entry);
    }
    manifest.entries.sort_by(|a, b| {
        a.arch
            .cmp(&b.arch)
            .then_with(|| a.build_id.cmp(&b.build_id))
            .then_with(|| a.flutter_version.cmp(&b.flutter_version))
    });
}

fn register_local_symbol_cache(
    local_cache_root: &Path,
    arch: &str,
    stripped_path: &Path,
    unstripped_path: &Path,
    targets_path: &Path,
    report_path: &Path,
    notes: &mut Vec<String>,
) -> Result<LocalSymbolCacheRegistration> {
    let repo_root = local_cache_root.parent().unwrap_or(local_cache_root);
    let stripped_fingerprint = fingerprint_engine_for_local_cache(stripped_path).ok();
    let unstripped_fingerprint = fingerprint_engine_for_local_cache(unstripped_path).ok();

    let build_id = stripped_fingerprint
        .as_ref()
        .and_then(|report| report.build_id.clone())
        .or_else(|| {
            unstripped_fingerprint
                .as_ref()
                .and_then(|report| report.build_id.clone())
        });
    let flutter_version = stripped_fingerprint
        .as_ref()
        .and_then(|report| report.candidate_flutter_version.clone())
        .or_else(|| {
            unstripped_fingerprint
                .as_ref()
                .and_then(|report| report.candidate_flutter_version.clone())
        });
    let dart_version = stripped_fingerprint
        .as_ref()
        .and_then(|report| report.candidate_dart_version.clone())
        .or_else(|| {
            unstripped_fingerprint
                .as_ref()
                .and_then(|report| report.candidate_dart_version.clone())
        });

    if build_id.is_none() && flutter_version.is_none() {
        notes.push("local cache registration skipped: no build id or flutter version recovered".to_string());
        return Ok(LocalSymbolCacheRegistration::default());
    }

    let mut registered_paths = Vec::new();
    let mut build_id_target_summary_path = None;
    let mut version_target_summary_path = None;

    if let Some(build_id_value) = build_id.as_deref() {
        let build_id_dir = local_cache_root
            .join("by-build-id")
            .join(sanitize_local_cache_key(build_id_value));
        fs::create_dir_all(&build_id_dir)
            .with_context(|| format!("create build-id cache dir {}", build_id_dir.display()))?;
        let destination = build_id_dir.join("symbol_target_summary.json");
        fs::copy(targets_path, &destination).with_context(|| {
            format!(
                "copy symbol target summary into build-id cache {}",
                destination.display()
            )
        })?;
        let relative = repo_relative_display_path(repo_root, &destination);
        build_id_target_summary_path = Some(relative.clone());
        registered_paths.push(relative);
    }

    if let Some(version_value) = flutter_version.as_deref() {
        let version_dir = local_cache_root
            .join("by-version")
            .join(sanitize_local_cache_key(version_value));
        fs::create_dir_all(&version_dir)
            .with_context(|| format!("create version cache dir {}", version_dir.display()))?;
        let destination = version_dir.join("symbol_target_summary.json");
        fs::copy(targets_path, &destination).with_context(|| {
            format!(
                "copy symbol target summary into version cache {}",
                destination.display()
            )
        })?;
        let relative = repo_relative_display_path(repo_root, &destination);
        version_target_summary_path = Some(relative.clone());
        registered_paths.push(relative);
    }

    let mut manifest = load_local_symbol_cache_manifest(local_cache_root)?;
    update_local_symbol_cache_entry(
        &mut manifest,
        LocalSymbolCacheEntry {
            arch: arch.to_string(),
            build_id: build_id.clone(),
            flutter_version: flutter_version.clone(),
            dart_version,
            build_id_target_summary_path,
            version_target_summary_path,
            report_path: Some(repo_relative_display_path(repo_root, report_path)),
        },
    );
    let manifest_path = write_local_symbol_cache_manifest(local_cache_root, &manifest)?;
    Ok(LocalSymbolCacheRegistration {
        manifest_path: Some(manifest_path),
        build_id,
        flutter_version,
        registered_paths,
    })
}

fn resolve_local_symbol_cache_paths(
    local_cache_root: &Path,
    arch: &str,
    build_id: Option<&str>,
    flutter_version: Option<&str>,
) -> Result<LocalSymbolCacheResolution> {
    let repo_root = local_cache_root.parent().unwrap_or(local_cache_root);
    let manifest_path = local_cache_root.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(LocalSymbolCacheResolution {
            manifest_path: Some(manifest_path),
            ..LocalSymbolCacheResolution::default()
        });
    }

    let manifest = load_local_symbol_cache_manifest(local_cache_root)?;
    let mut resolution = LocalSymbolCacheResolution {
        manifest_path: Some(manifest_path),
        ..LocalSymbolCacheResolution::default()
    };

    if let Some(build_id_value) = build_id {
        if let Some(entry) = manifest.entries.iter().find(|entry| {
            entry.arch == arch
                && entry
                    .build_id
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(build_id_value))
                && entry.build_id_target_summary_path.is_some()
        }) {
            if let Some(path) = entry.build_id_target_summary_path.as_deref() {
                let resolved = resolve_manifest_target_path(repo_root, path);
                if resolved.exists() {
                    resolution.match_kind = Some("build_id".to_string());
                    resolution.paths.push(resolved);
                } else {
                    resolution.error = Some(format!(
                        "manifest build-id target path does not exist: {}",
                        resolved.display()
                    ));
                }
            }
            return Ok(resolution);
        }
        return Ok(resolution);
    }

    if let Some(version_value) = flutter_version {
        if let Some(entry) = manifest.entries.iter().find(|entry| {
            entry.arch == arch
                && entry.flutter_version.as_deref() == Some(version_value)
                && entry.version_target_summary_path.is_some()
        }) {
            if let Some(path) = entry.version_target_summary_path.as_deref() {
                let resolved = resolve_manifest_target_path(repo_root, path);
                if resolved.exists() {
                    resolution.match_kind = Some("flutter_version".to_string());
                    resolution.paths.push(resolved);
                } else {
                    resolution.error = Some(format!(
                        "manifest version target path does not exist: {}",
                        resolved.display()
                    ));
                }
            }
        }
    }

    Ok(resolution)
}

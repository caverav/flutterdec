use super::*;

#[test]
fn parses_target_from_hex_and_decimal_operands() {
    assert_eq!(parse_target_va("#0x4010"), Some(0x4010));
    assert_eq!(parse_target_va("x0, #0x2008"), Some(0x2008));
    assert_eq!(parse_target_va("1234"), Some(1234));
    assert_eq!(parse_target_va("x17"), None);
}

#[test]
fn resolves_exact_before_nearest() {
    let mut syms = BTreeMap::new();
    syms.insert(0x1000, "foo".to_string());
    syms.insert(0x1100, "bar".to_string());

    let exact = resolve_target(&syms, Some(0x1100), 0x200);
    assert!(matches!(exact.kind, MatchKind::Exact));
    assert_eq!(exact.symbol_name.as_deref(), Some("bar"));

    let near = resolve_target(&syms, Some(0x1120), 0x40);
    assert!(matches!(near.kind, MatchKind::Nearest));
    assert_eq!(near.symbol_name.as_deref(), Some("bar"));
    assert_eq!(near.symbol_offset, Some(0x20));

    let far = resolve_target(&syms, Some(0x2000), 0x10);
    assert!(matches!(far.kind, MatchKind::Unresolved));
}

#[test]
fn loads_symbol_target_symbols_and_filters_match_kind() {
    let tmp = std::env::temp_dir().join(format!(
        "flutterdec_symbol_map_test_{}_{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("t")
    ));

    let data = serde_json::json!([
        {
            "target_va": 4096,
            "call_count": 1,
            "match_kind": "exact",
            "symbol_name": "ExactFn",
            "symbol_va": 4096,
            "symbol_offset": 0
        },
        {
            "target_va": 8192,
            "call_count": 1,
            "match_kind": "nearest",
            "symbol_name": "NearFn",
            "symbol_va": 8160,
            "symbol_offset": 32
        },
        {
            "target_va": 12288,
            "call_count": 1,
            "match_kind": "unresolved",
            "symbol_name": null,
            "symbol_va": null,
            "symbol_offset": null
        }
    ]);
    fs::write(&tmp, serde_json::to_vec(&data).expect("json")).expect("write symbol map fixture");

    let exact_only = load_symbol_target_symbols(&tmp, false).expect("load exact symbols");
    assert_eq!(exact_only.get(&0x1000).map(String::as_str), Some("ExactFn"));
    assert!(!exact_only.contains_key(&0x2000));

    let with_near = load_symbol_target_symbols(&tmp, true).expect("load near symbols");
    assert_eq!(with_near.get(&0x1000).map(String::as_str), Some("ExactFn"));
    assert_eq!(with_near.get(&0x2000).map(String::as_str), Some("NearFn"));
    assert!(!with_near.contains_key(&0x3000));

    let _ = fs::remove_file(tmp);
}

#[test]
fn resolves_local_symbol_cache_by_build_id_before_version() {
    let td = tempfile::tempdir().expect("tempdir");
    let repo_root = td.path();
    let build_id_path = repo_root.join("symbols/by-build-id/abc123/symbol_target_summary.json");
    let version_path = repo_root.join("symbols/by-version/3.24.0/symbol_target_summary.json");
    fs::create_dir_all(build_id_path.parent().expect("build-id dir")).expect("mkdir build-id");
    fs::create_dir_all(version_path.parent().expect("version dir")).expect("mkdir version");
    fs::write(&build_id_path, "[]").expect("write build-id target summary");
    fs::write(&version_path, "[]").expect("write version target summary");
    let manifest = LocalSymbolCacheManifest {
        entries: vec![LocalSymbolCacheEntry {
            arch: "arm64".to_string(),
            build_id: Some("abc123".to_string()),
            flutter_version: Some("3.24.0".to_string()),
            dart_version: Some("3.5.0".to_string()),
            build_id_target_summary_path: Some(
                "symbols/by-build-id/abc123/symbol_target_summary.json".to_string(),
            ),
            version_target_summary_path: Some(
                "symbols/by-version/3.24.0/symbol_target_summary.json".to_string(),
            ),
            report_path: None,
        }],
    };
    write_local_symbol_cache_manifest(&repo_root.join("symbols"), &manifest).expect("manifest");

    let cache_root = repo_root.join("symbols");
    let build_id_resolution = resolve_local_symbol_cache_paths(
        &cache_root,
        "arm64",
        Some("ABC123"),
        Some("3.24.0"),
    )
    .expect("resolve build-id");
    assert_eq!(build_id_resolution.match_kind.as_deref(), Some("build_id"));
    assert_eq!(build_id_resolution.paths, vec![build_id_path.clone()]);

    let no_fallback_resolution = resolve_local_symbol_cache_paths(
        &cache_root,
        "arm64",
        Some("missing-build-id"),
        Some("3.24.0"),
    )
    .expect("resolve missing build-id");
    assert!(no_fallback_resolution.paths.is_empty());
    assert!(no_fallback_resolution.match_kind.is_none());

    let version_resolution =
        resolve_local_symbol_cache_paths(&cache_root, "arm64", None, Some("3.24.0"))
            .expect("resolve version");
    assert_eq!(version_resolution.match_kind.as_deref(), Some("flutter_version"));
    assert_eq!(version_resolution.paths, vec![version_path]);
}

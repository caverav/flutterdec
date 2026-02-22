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

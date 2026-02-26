    use super::*;
    use flutterdec_disasm_arm64::{AsmInstruction, FunctionDisassembly, FunctionPriorityComponent};
    use tempfile::tempdir;

    #[test]
    fn formats_asm_instruction_without_opcode_word() {
        let ins = AsmInstruction {
            va: 0x613468,
            word: 0x9400_0001,
            mnemonic: "bl".to_string(),
            op_str: "0x61346c".to_string(),
            annotation: "call".to_string(),
        };
        let line = format_asm_instruction_line(&ins, false);
        assert_eq!(line, "0x613468: bl 0x61346c ; call");
    }

    #[test]
    fn formats_asm_instruction_with_opcode_word() {
        let ins = AsmInstruction {
            va: 0x613468,
            word: 0x9400_0001,
            mnemonic: "bl".to_string(),
            op_str: "0x61346c".to_string(),
            annotation: "call".to_string(),
        };
        let line = format_asm_instruction_line(&ins, true);
        assert_eq!(line, "0x613468: 94000001 bl 0x61346c ; call");
    }

    #[test]
    fn builds_ghidra_symbol_script_with_sorted_entries_and_escaped_names() {
        let mut symbols = HashMap::new();
        symbols.insert(0x2000, "sub_2000".to_string());
        symbols.insert(0x1000, "RenderErrorBox.\"main\"".to_string());

        let script = build_ghidra_symbol_script(&symbols, &[]).expect("script");
        let first = script.find("(0x1000,").expect("0x1000");
        let second = script.find("(0x2000,").expect("0x2000");
        assert!(first < second, "entries should be sorted by VA");
        assert!(script.contains("RenderErrorBox.\\\"main\\\""));
        assert!(script.contains("createLabel(addr, name, True)"));
    }

    #[test]
    fn builds_ida_symbol_script_with_sorted_entries_and_pool_comments() {
        let mut symbols = HashMap::new();
        symbols.insert(0x2000, "sub_2000".to_string());
        symbols.insert(0x1000, "RenderErrorBox.\"main\"".to_string());
        let pool_comments = vec![(0x1004, "pool[4584] = surface".to_string())];

        let script = build_ida_symbol_script(&symbols, &pool_comments).expect("script");
        let first = script.find("(0x1000,").expect("0x1000");
        let second = script.find("(0x2000,").expect("0x2000");
        assert!(first < second, "entries should be sorted by VA");
        assert!(script.contains("RenderErrorBox.\\\"main\\\""));
        assert!(script.contains("ida_name.set_name"));
        assert!(script.contains("idc.set_cmt(va, text, 0)"));
    }

    #[test]
    fn parses_pool_annotation_indexes() {
        assert_eq!(parse_pool_annotation_index("pool[4584]"), Some(4584));
        assert_eq!(parse_pool_annotation_index(" pool[0] "), Some(0));
        assert_eq!(parse_pool_annotation_index("call"), None);
    }

    #[test]
    fn collects_ghidra_pool_comments_from_disassembly() {
        let disasm = vec![FunctionDisassembly {
            function_id: 1,
            function_name: "main".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x1000,
            size: 8,
            instructions: vec![
                AsmInstruction {
                    va: 0x1000,
                    word: 0,
                    mnemonic: "ldr".to_string(),
                    op_str: "x1, [x27, #4584]".to_string(),
                    annotation: "pool[4584]".to_string(),
                },
                AsmInstruction {
                    va: 0x1004,
                    word: 0,
                    mnemonic: "ldr".to_string(),
                    op_str: "x2, [x27, #4592]".to_string(),
                    annotation: "pool[4592]".to_string(),
                },
            ],
        }];
        let mut pool_hints = HashMap::new();
        pool_hints.insert(4584, "surface".to_string());

        let comments = collect_ghidra_pool_comments(&disasm, &pool_hints);
        assert_eq!(
            comments,
            vec![
                (0x1000, "pool[4584] = surface".to_string()),
                (0x1004, "pool[4592]".to_string()),
            ]
        );
    }

    #[test]
    fn merge_symbol_name_replaces_generic_only() {
        let mut map = HashMap::new();
        map.insert(0x1000, "sub_1000".to_string());
        map.insert(0x2000, "StrongName".to_string());
        let mut quality = HashMap::new();
        quality.insert(0x1000, SymbolNameQuality::Placeholder);
        quality.insert(0x2000, SymbolNameQuality::External);

        let mut stats = SymbolMergeStats::default();

        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "RealSymbol".to_string(),
            Some(SymbolNameQuality::External),
            &mut stats,
        );
        merge_symbol_name(
            &mut map,
            &mut quality,
            0x2000,
            "OtherSymbol".to_string(),
            Some(SymbolNameQuality::External),
            &mut stats,
        );
        merge_symbol_name(
            &mut map,
            &mut quality,
            0x3000,
            "InsertedSymbol".to_string(),
            Some(SymbolNameQuality::Heuristic),
            &mut stats,
        );

        assert_eq!(map.get(&0x1000).map(String::as_str), Some("RealSymbol"));
        assert_eq!(map.get(&0x2000).map(String::as_str), Some("StrongName"));
        assert_eq!(map.get(&0x3000).map(String::as_str), Some("InsertedSymbol"));
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.replaced, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.replaced_to_external, 1);
    }

    #[test]
    fn merge_symbol_name_replaces_heuristic_with_stronger_symbol() {
        let mut map = HashMap::new();
        map.insert(0x1000, "package_spotube_Global_main".to_string());
        let mut quality = HashMap::new();
        quality.insert(0x1000, SymbolNameQuality::Heuristic);
        let mut stats = SymbolMergeStats::default();

        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "RealMainEntry".to_string(),
            Some(SymbolNameQuality::External),
            &mut stats,
        );
        assert_eq!(map.get(&0x1000).map(String::as_str), Some("RealMainEntry"));
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.replaced, 1);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.replaced_to_external, 1);

        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "package_spotube_Global_main".to_string(),
            Some(SymbolNameQuality::Heuristic),
            &mut stats,
        );
        assert_eq!(map.get(&0x1000).map(String::as_str), Some("RealMainEntry"));
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.replaced, 1);
        assert_eq!(stats.skipped, 1);
    }

    #[test]
    fn merge_symbol_name_prefers_exact_over_external_heuristic_and_placeholder() {
        let mut map = HashMap::new();
        map.insert(0x1000, "sub_1000".to_string());
        let mut quality = HashMap::new();
        quality.insert(0x1000, SymbolNameQuality::Placeholder);
        let mut stats = SymbolMergeStats::default();

        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "package_spotube_Global_main".to_string(),
            Some(SymbolNameQuality::Heuristic),
            &mut stats,
        );
        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "RealMainEntry".to_string(),
            Some(SymbolNameQuality::External),
            &mut stats,
        );
        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "SpotubeMain".to_string(),
            Some(SymbolNameQuality::Exact),
            &mut stats,
        );
        merge_symbol_name(
            &mut map,
            &mut quality,
            0x1000,
            "native_libc_printf".to_string(),
            Some(SymbolNameQuality::External),
            &mut stats,
        );

        assert_eq!(map.get(&0x1000).map(String::as_str), Some("SpotubeMain"));
        assert_eq!(quality.get(&0x1000), Some(&SymbolNameQuality::Exact));
        assert_eq!(stats.replaced, 3);
        assert_eq!(stats.replaced_to_heuristic, 1);
        assert_eq!(stats.replaced_to_external, 1);
        assert_eq!(stats.replaced_to_exact, 1);
        assert_eq!(stats.skipped, 1);
    }

    #[test]
    fn resolves_symbol_quality_from_name_kind_values() {
        assert_eq!(
            symbol_name_quality_from_name_kind(Some("exact")),
            Some(SymbolNameQuality::Exact)
        );
        assert_eq!(
            symbol_name_quality_from_name_kind(Some("external")),
            Some(SymbolNameQuality::External)
        );
        assert_eq!(
            symbol_name_quality_from_name_kind(Some("heuristic")),
            Some(SymbolNameQuality::Heuristic)
        );
        assert_eq!(
            symbol_name_quality_from_name_kind(Some("placeholder")),
            Some(SymbolNameQuality::Placeholder)
        );
        assert_eq!(symbol_name_quality_from_name_kind(Some("unknown")), None);
        assert_eq!(symbol_name_quality_from_name_kind(None), None);
    }

    #[test]
    fn collects_function_name_kind_breakdown() {
        let functions = vec![
            flutterdec_adapter::FunctionInfo {
                id: 1,
                name: "main".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1000,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some("exact".to_string()),
            },
            flutterdec_adapter::FunctionInfo {
                id: 2,
                name: "native_libc_printf".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1010,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some("external".to_string()),
            },
            flutterdec_adapter::FunctionInfo {
                id: 3,
                name: "flutter_widgets_State_build".to_string(),
                owner_class: "State".to_string(),
                entry_va: 0x1020,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some("heuristic".to_string()),
            },
            flutterdec_adapter::FunctionInfo {
                id: 4,
                name: "sub_1030".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1030,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some("placeholder".to_string()),
            },
            flutterdec_adapter::FunctionInfo {
                id: 5,
                name: "fun_custom".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1040,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some("mystery".to_string()),
            },
            flutterdec_adapter::FunctionInfo {
                id: 6,
                name: "sub_1050".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1050,
                size: 16,
                code_section_va: 0x1000,
                name_kind: None,
            },
            flutterdec_adapter::FunctionInfo {
                id: 7,
                name: "sub_1060".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1060,
                size: 16,
                code_section_va: 0x1000,
                name_kind: Some(" ".to_string()),
            },
        ];
        let stats = collect_function_name_kind_stats(&functions);
        assert_eq!(stats.exact, 1);
        assert_eq!(stats.external, 1);
        assert_eq!(stats.heuristic, 1);
        assert_eq!(stats.placeholder, 1);
        assert_eq!(stats.unknown, 1);
        assert_eq!(stats.unspecified, 2);
        assert_eq!(stats.tagged(), 5);
    }

    #[test]
    fn collects_function_descriptors_with_library_context() {
        let model = ProgramModel {
            schema_version: 3,
            adapter_kind: "python".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![flutterdec_adapter::LibraryInfo {
                id: 0,
                uri: "package:spotube/main.dart".to_string(),
                name_display: "package:spotube/main.dart".to_string(),
            }],
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:spotube/main.dart".to_string(),
            }],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 0,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 16,
                    code_section_va: 0x1000,
                    name_kind: Some("heuristic".to_string()),
                },
                flutterdec_adapter::FunctionInfo {
                    id: 1,
                    name: "sub_2000".to_string(),
                    owner_class: "UnknownOwner".to_string(),
                    entry_va: 0x2000,
                    size: 16,
                    code_section_va: 0x1000,
                    name_kind: Some("placeholder".to_string()),
                },
            ],
            object_pool: Vec::new(),
        };

        let descriptors = collect_function_descriptors(&model);
        assert!(descriptors.contains("package:spotube/main.dart::Global::main"));
        assert!(descriptors.contains("UnknownOwner::sub_2000"));
    }

    #[test]
    fn canonicalizes_flutter_build_file_uri_in_descriptors() {
        let model = ProgramModel {
            schema_version: 3,
            adapter_kind: "python".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![flutterdec_adapter::LibraryInfo {
                id: 0,
                uri:
                    "file:///tmp/build/app/.dart_tool/flutter_build/dart_plugin_registrant.dart"
                        .to_string(),
                name_display: "generated".to_string(),
            }],
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 0,
                name: "_PluginRegistrant".to_string(),
                super_name: "Object".to_string(),
                library_uri:
                    "file:///tmp/build/app/.dart_tool/flutter_build/dart_plugin_registrant.dart"
                        .to_string(),
            }],
            functions: vec![flutterdec_adapter::FunctionInfo {
                id: 0,
                name: "register".to_string(),
                owner_class: "_PluginRegistrant".to_string(),
                entry_va: 0x3000,
                size: 16,
                code_section_va: 0x3000,
                name_kind: Some("heuristic".to_string()),
            }],
            object_pool: Vec::new(),
        };

        let descriptors = collect_function_descriptors(&model);
        assert!(descriptors.contains(
            "file:///.dart_tool/flutter_build/dart_plugin_registrant.dart::_PluginRegistrant::register"
        ));
    }

    #[test]
    fn collects_diff_package_counts_for_added_removed_sets() {
        let descriptors = vec![
            "package:spotube/main.dart::Global::main".to_string(),
            "package:spotube/router.dart::Global::route".to_string(),
            "dart:core/bool.dart::bool::operator ==".to_string(),
            "file:///.dart_tool/flutter_build/dart_plugin_registrant.dart::_PluginRegistrant::register".to_string(),
            "UnknownOwner::sub_1000".to_string(),
        ];

        let counts = collect_diff_package_counts(&descriptors);
        assert_eq!(counts.first().map(|p| p.package.as_str()), Some("spotube"));
        assert_eq!(counts.first().map(|p| p.functions), Some(2));
        assert!(counts.iter().any(|p| p.package == "dart" && p.functions == 1));
        assert!(counts.iter().any(|p| p.package == "file" && p.functions == 1));
        assert!(counts.iter().any(|p| p.package == "unknown" && p.functions == 1));
    }

    #[test]
    fn snapshot_hash_match_enforcement_behaves_as_expected() {
        let ok = enforce_snapshot_hash_match(false, "test", "aaa", "bbb").expect("not strict");
        assert!(!ok);
        let ok = enforce_snapshot_hash_match(true, "test", "aaa", "aaa").expect("strict match");
        assert!(ok);
        let err = enforce_snapshot_hash_match(true, "test", "aaa", "bbb").expect_err("mismatch");
        assert!(err.to_string().contains("snapshot hash mismatch"));
    }

    #[test]
    fn collects_compatibility_warnings_from_flags() {
        let warnings = collect_compatibility_warnings(false, false, true);
        assert_eq!(warnings.len(), 3);
        assert!(warnings
            .iter()
            .any(|w| w.contains("manifest entry missing")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("snapshot hash differs")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("backend differs")));

        let warnings = collect_compatibility_warnings(true, true, false);
        assert!(warnings.is_empty());
    }

    #[test]
    fn resolves_backend_from_adapter_kind_values() {
        assert_eq!(
            resolved_backend_from_adapter_kind("blutter_bridge_model_v1"),
            Some(AdapterBackend::Blutter)
        );
        assert_eq!(
            resolved_backend_from_adapter_kind("dynamic_snapshot_string_model_v1"),
            Some(AdapterBackend::Internal)
        );
        assert_eq!(resolved_backend_from_adapter_kind("custom_model_v2"), None);
        assert_eq!(backend_label(Some(AdapterBackend::Internal)), "internal");
        assert_eq!(backend_label(Some(AdapterBackend::Blutter)), "blutter");
        assert_eq!(backend_label(None), "unknown");
    }

    #[test]
    fn engine_fingerprint_context_reports_missing_engine_binary() {
        let td = tempdir().expect("tempdir");
        let input = td.path().join("libapp.so");
        std::fs::write(&input, b"dummy").expect("write dummy input");
        let ctx = try_collect_engine_fingerprint(&input, "arm64");
        assert!(!ctx.detected);
        assert!(ctx.source.is_none());
        assert!(ctx.error.is_some());
    }

    #[test]
    fn generic_name_detection_is_strict() {
        assert!(is_generic_symbol_name("sub_1234"));
        assert!(is_generic_symbol_name("fn_0x55"));
        assert!(is_generic_symbol_name("unknown"));
        assert!(is_generic_symbol_name("FUN_0012ABCD"));
        assert!(is_generic_symbol_name("nullsub_12"));
        assert!(is_generic_symbol_name("loc_10"));
        assert!(is_generic_symbol_name("off_20"));
        assert!(!is_generic_symbol_name("Dart_Invoke"));
        assert!(!is_generic_symbol_name("fun_processData"));
    }

    #[test]
    fn classifies_function_scope_from_library_uri() {
        assert_eq!(
            classify_library_uri("package:flutter/src/widgets/framework.dart"),
            ScopedFunctionKind::Framework
        );
        assert_eq!(classify_library_uri("dart:core"), ScopedFunctionKind::Stdlib);
        assert_eq!(
            classify_library_uri("package:spotube/models/connect/load.dart"),
            ScopedFunctionKind::App
        );
        assert_eq!(classify_library_uri(""), ScopedFunctionKind::Unknown);
    }

    #[test]
    fn normalizes_package_names_and_library_uris() {
        assert_eq!(
            normalize_package_name(" package:Spotube/models/connect.dart "),
            Some("spotube".to_string())
        );
        assert_eq!(
            normalize_package_name("provider"),
            Some("provider".to_string())
        );
        assert_eq!(
            package_name_from_library_uri("package:spotube/models/connect/load.dart"),
            Some("spotube".to_string())
        );
        assert_eq!(package_name_from_library_uri("dart:core"), None);
    }

    #[test]
    fn derives_manifest_priority_package_hints() {
        assert_eq!(
            derive_manifest_package_hints(Some("oss.krtirtho.spotube")),
            vec!["spotube".to_string()]
        );
        assert_eq!(
            derive_manifest_package_hints(Some("org.localsend.localsend_app")),
            vec!["localsend".to_string(), "localsend_app".to_string()]
        );
        assert_eq!(
            derive_manifest_package_hints(Some("dev.foo.mobile_flutter")),
            vec!["mobile".to_string(), "mobile_flutter".to_string()]
        );
        assert_eq!(
            derive_manifest_package_hints(Some("com.acme.app")),
            vec!["acme".to_string()]
        );
        assert_eq!(derive_manifest_package_hints(None), Vec::<String>::new());
    }

    #[test]
    fn classifies_priority_package_from_library_uri() {
        assert_eq!(
            priority_package_from_library_uri("package:spotube/main.dart"),
            "spotube".to_string()
        );
        assert_eq!(
            priority_package_from_library_uri("dart:core-patch/core_patch.dart"),
            "dart".to_string()
        );
        assert_eq!(priority_package_from_library_uri(""), "unknown".to_string());
    }

    #[test]
    fn collects_selected_priority_package_counts() {
        let selected = vec![
            FunctionPriorityBreakdown {
                function_id: 1,
                function_name: "main".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:spotube/main.dart".to_string(),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: "init".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:spotube/services/init.dart".to_string(),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: "watch".to_string(),
                owner_class: "Provider".to_string(),
                library_uri: "package:provider/src/provider.dart".to_string(),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: "toString".to_string(),
                owner_class: "Object".to_string(),
                library_uri: "dart:core".to_string(),
                entry_va: 0x1030,
                total_score: 70,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 5,
                function_name: "sub_1040".to_string(),
                owner_class: "Unknown".to_string(),
                library_uri: "".to_string(),
                entry_va: 0x1040,
                total_score: 60,
                components: Vec::new(),
            },
        ];
        let counts = collect_selected_priority_package_counts(&selected);
        assert_eq!(
            counts,
            vec![
                ("spotube".to_string(), 2),
                ("dart".to_string(), 1),
                ("provider".to_string(), 1),
                ("unknown".to_string(), 1),
            ]
        );
    }

    #[test]
    fn collects_selected_priority_scope_mix() {
        let selected = vec![
            FunctionPriorityBreakdown {
                function_id: 1,
                function_name: "main".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:spotube/main.dart".to_string(),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: "setState".to_string(),
                owner_class: "State".to_string(),
                library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: "toString".to_string(),
                owner_class: "Object".to_string(),
                library_uri: "dart:core".to_string(),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: "sub_1030".to_string(),
                owner_class: "Unknown".to_string(),
                library_uri: "".to_string(),
                entry_va: 0x1030,
                total_score: 70,
                components: Vec::new(),
            },
        ];
        let mix = collect_selected_priority_scope_mix(&selected);
        assert_eq!(mix.app, 1);
        assert_eq!(mix.framework, 1);
        assert_eq!(mix.stdlib, 1);
        assert_eq!(mix.unknown, 1);
    }

    #[test]
    fn collects_selected_preferred_package_stats() {
        let selected = vec![
            FunctionPriorityBreakdown {
                function_id: 1,
                function_name: "main".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: "init".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:spotube/main.dart".to_string(),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: "watch".to_string(),
                owner_class: "Provider".to_string(),
                library_uri: "package:provider/src/provider.dart".to_string(),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: "toString".to_string(),
                owner_class: "Object".to_string(),
                library_uri: "dart:core".to_string(),
                entry_va: 0x1030,
                total_score: 70,
                components: Vec::new(),
            },
        ];
        let preferred = HashSet::from(["app".to_string(), "spotube".to_string()]);
        let stats = collect_selected_preferred_package_stats(&selected, &preferred);
        assert_eq!(stats.preferred_app, 2);
        assert_eq!(stats.other_app, 1);
    }

    #[test]
    fn collects_selected_priority_component_totals() {
        let selected = vec![
            FunctionPriorityBreakdown {
                function_id: 1,
                function_name: "main".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                entry_va: 0x1000,
                total_score: 100,
                components: vec![
                    FunctionPriorityComponent {
                        name: "main_name_bonus".to_string(),
                        score: 900,
                    },
                    FunctionPriorityComponent {
                        name: "package_library_bonus".to_string(),
                        score: 220,
                    },
                ],
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: "sub_1010".to_string(),
                owner_class: "Provider".to_string(),
                library_uri: "package:provider/src/provider.dart".to_string(),
                entry_va: 0x1010,
                total_score: 90,
                components: vec![
                    FunctionPriorityComponent {
                        name: "main_name_bonus".to_string(),
                        score: 900,
                    },
                    FunctionPriorityComponent {
                        name: "non_preferred_package_penalty:provider".to_string(),
                        score: -220,
                    },
                ],
            },
        ];
        let totals = collect_selected_priority_component_totals(&selected);
        assert_eq!(
            totals.first(),
            Some(&("main_name_bonus".to_string(), 2, 1800))
        );
        assert!(totals.contains(&("package_library_bonus".to_string(), 1, 220)));
        assert!(totals.contains(&(
            "non_preferred_package_penalty:provider".to_string(),
            1,
            -220,
        )));
    }

    #[test]
    fn collects_selected_bootflow_hits_and_coverage() {
        let selected = vec![
            FunctionPriorityBreakdown {
                function_id: 1,
                function_name: "main".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: "runApp".to_string(),
                owner_class: "Global".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
        ];
        let bootflow = BootflowDiscoverySummary {
            main: vec![
                BootflowDiscoveryEntry {
                    decoded_kind: "pool".to_string(),
                    selector: "main".to_string(),
                    target_va: 0x1000,
                    owner_class: "Global".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                    value: "bootflow:main:main".to_string(),
                },
                BootflowDiscoveryEntry {
                    decoded_kind: "pool".to_string(),
                    selector: "main".to_string(),
                    target_va: 0x2000,
                    owner_class: "Global".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                    value: "bootflow:main:main".to_string(),
                },
            ],
            runapp: vec![BootflowDiscoveryEntry {
                decoded_kind: "pool".to_string(),
                selector: "runApp".to_string(),
                target_va: 0x1010,
                owner_class: "Global".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                value: "bootflow:runapp:runApp".to_string(),
            }],
            deeplink: vec![BootflowDiscoveryEntry {
                decoded_kind: "pool".to_string(),
                selector: "onNewIntent".to_string(),
                target_va: 0x3000,
                owner_class: "MainActivity".to_string(),
                library_uri: "package:app/main.dart".to_string(),
                value: "bootflow:deeplink:onNewIntent".to_string(),
            }],
            activity: Vec::new(),
            bootstrap: vec![BootflowDiscoveryEntry {
                decoded_kind: "pool".to_string(),
                selector: "ensureInitialized".to_string(),
                target_va: 0x1000,
                owner_class: "WidgetsFlutterBinding".to_string(),
                library_uri: "package:flutter/src/widgets/binding.dart".to_string(),
                value: "bootflow:init:ensureInitialized".to_string(),
            }],
        };

        let (stats, hits) = collect_selected_bootflow_hits(&selected, &bootflow);
        assert_eq!(stats.main.discovered, 2);
        assert_eq!(stats.main.selected, 1);
        assert_eq!(stats.runapp.discovered, 1);
        assert_eq!(stats.runapp.selected, 1);
        assert_eq!(stats.deeplink.discovered, 1);
        assert_eq!(stats.deeplink.selected, 0);
        assert_eq!(stats.bootstrap.discovered, 1);
        assert_eq!(stats.bootstrap.selected, 1);
        assert_eq!(stats.any.discovered, 4);
        assert_eq!(stats.any.selected, 2);
        assert!((selected_bootflow_coverage_ratio(stats.any) - 0.5).abs() < f64::EPSILON);

        assert!(hits.iter().any(|hit| {
            hit.category == "main" && hit.target_va == 0x1000 && hit.function_name == "main"
        }));
        assert!(hits.iter().any(|hit| {
            hit.category == "runapp"
                && hit.target_va == 0x1010
                && hit.function_name == "runApp"
        }));
        assert!(hits.iter().any(|hit| {
            hit.category == "bootstrap"
                && hit.target_va == 0x1000
                && hit.function_name == "main"
        }));
    }

    #[test]
    fn applies_app_unknown_scope_filter() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 1,
                    name: "State".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 2,
                    name: "_StringBase".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "dart:core".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 3,
                    name: "ConnectService".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:spotube/models/connect/load.dart".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 10,
                    name: "setState".to_string(),
                    owner_class: "State".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 11,
                    name: "toString".to_string(),
                    owner_class: "_StringBase".to_string(),
                    entry_va: 0x1100,
                    size: 4,
                    code_section_va: 0x1100,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 12,
                    name: "executeCommandAsync".to_string(),
                    owner_class: "ConnectService".to_string(),
                    entry_va: 0x1200,
                    size: 4,
                    code_section_va: 0x1200,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 13,
                    name: "sub_1300".to_string(),
                    owner_class: "UnknownOwner".to_string(),
                    entry_va: 0x1300,
                    size: 4,
                    code_section_va: 0x1300,
                name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };

        let (scoped, stats) = apply_function_scope_filter(&model, FunctionScope::AppUnknown, &[]);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![12, 13]);
        assert_eq!(stats.total_before_filter, 4);
        assert_eq!(stats.total_after_filter, 2);
        assert_eq!(stats.excluded, 2);
        assert_eq!(stats.app, 1);
        assert_eq!(stats.framework, 1);
        assert_eq!(stats.stdlib, 1);
        assert_eq!(stats.unknown, 1);
    }

    #[test]
    fn applies_app_scope_filter() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 3,
                name: "ConnectService".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:spotube/models/connect/load.dart".to_string(),
            }],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 12,
                    name: "executeCommandAsync".to_string(),
                    owner_class: "ConnectService".to_string(),
                    entry_va: 0x1200,
                    size: 4,
                    code_section_va: 0x1200,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 13,
                    name: "sub_1300".to_string(),
                    owner_class: "UnknownOwner".to_string(),
                    entry_va: 0x1300,
                    size: 4,
                    code_section_va: 0x1300,
                name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };

        let (scoped, stats) = apply_function_scope_filter(&model, FunctionScope::App, &[]);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![12]);
        assert_eq!(stats.total_before_filter, 2);
        assert_eq!(stats.total_after_filter, 1);
        assert_eq!(stats.excluded, 1);
    }

    #[test]
    fn applies_app_package_filter_to_scoped_functions() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 3,
                    name: "ConnectService".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:spotube/models/connect/load.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 4,
                    name: "ProviderCore".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:provider/src/provider.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 5,
                    name: "State".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 12,
                    name: "executeCommandAsync".to_string(),
                    owner_class: "ConnectService".to_string(),
                    entry_va: 0x1200,
                    size: 4,
                    code_section_va: 0x1200,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 13,
                    name: "watch".to_string(),
                    owner_class: "ProviderCore".to_string(),
                    entry_va: 0x1300,
                    size: 4,
                    code_section_va: 0x1300,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 14,
                    name: "setState".to_string(),
                    owner_class: "State".to_string(),
                    entry_va: 0x1400,
                    size: 4,
                    code_section_va: 0x1400,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 15,
                    name: "sub_1500".to_string(),
                    owner_class: "UnknownOwner".to_string(),
                    entry_va: 0x1500,
                    size: 4,
                    code_section_va: 0x1500,
                name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };

        let app_packages = vec!["spotube".to_string()];
        let (scoped, stats) =
            apply_function_scope_filter(&model, FunctionScope::AppUnknown, &app_packages);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![12]);
        assert_eq!(stats.total_before_filter, 4);
        assert_eq!(stats.total_after_filter, 1);
        assert_eq!(stats.excluded, 3);
        assert_eq!(stats.excluded_by_app_package, 2);
    }

    #[test]
    fn collects_app_package_function_counts() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 1,
                    name: "AppA".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:spotube/a.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 2,
                    name: "AppB".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:provider/b.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 3,
                    name: "State".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 10,
                    name: "f10".to_string(),
                    owner_class: "AppA".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 11,
                    name: "f11".to_string(),
                    owner_class: "AppA".to_string(),
                    entry_va: 0x1100,
                    size: 4,
                    code_section_va: 0x1100,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 12,
                    name: "f12".to_string(),
                    owner_class: "AppB".to_string(),
                    entry_va: 0x1200,
                    size: 4,
                    code_section_va: 0x1200,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 13,
                    name: "setState".to_string(),
                    owner_class: "State".to_string(),
                    entry_va: 0x1300,
                    size: 4,
                    code_section_va: 0x1300,
                name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };

        let counts = collect_app_package_counts(&model);
        assert_eq!(
            counts,
            vec![("spotube".to_string(), 2), ("provider".to_string(), 1)]
        );
    }

    #[test]
    fn normalizes_known_external_symbols() {
        assert_eq!(
            normalize_external_symbol_name("Dart_Invoke"),
            "vm_runtime_Invoke"
        );
        assert_eq!(
            normalize_external_symbol_name("memcpy@LIBC"),
            "native_libc_memcpy"
        );
        assert_eq!(
            normalize_external_symbol_name("__android_log_print"),
            "native_android_log_print"
        );
        assert_eq!(
            normalize_external_symbol_name("dart:core::print"),
            "dart_core_print"
        );
    }

    #[test]
    fn canonicalizes_standard_model_function_names() {
        let mut class_lib = HashMap::new();
        class_lib.insert("_StringBase".to_string(), "dart:core".to_string());
        class_lib.insert(
            "_BoolPatch".to_string(),
            "dart:core-patch/bool_patch.dart".to_string(),
        );
        class_lib.insert(
            "State".to_string(),
            "package:flutter/src/widgets/framework.dart".to_string(),
        );
        class_lib.insert(
            "RenderObject".to_string(),
            "package:flutter/src/rendering/object.dart".to_string(),
        );

        let dart_fn = flutterdec_adapter::FunctionInfo {
            id: 1,
            name: "toString".to_string(),
            owner_class: "_StringBase".to_string(),
            entry_va: 0x1000,
            size: 4,
            code_section_va: 0x1000,
        name_kind: None,
        };
        assert_eq!(
            canonical_standard_model_name(&dart_fn, &class_lib).as_deref(),
            Some("dart_core_toString")
        );

        let dart_patch_fn = flutterdec_adapter::FunctionInfo {
            id: 5,
            name: "fromEnvironment".to_string(),
            owner_class: "_BoolPatch".to_string(),
            entry_va: 0x1800,
            size: 4,
            code_section_va: 0x1800,
        name_kind: None,
        };
        assert_eq!(
            canonical_standard_model_name(&dart_patch_fn, &class_lib).as_deref(),
            Some("dart_core_patch_bool_patch_fromEnvironment")
        );

        let flutter_fn = flutterdec_adapter::FunctionInfo {
            id: 2,
            name: "setState".to_string(),
            owner_class: "State".to_string(),
            entry_va: 0x2000,
            size: 4,
            code_section_va: 0x2000,
        name_kind: None,
        };
        assert_eq!(
            canonical_standard_model_name(&flutter_fn, &class_lib).as_deref(),
            Some("flutter_widgets_State_setState")
        );

        let render_fn = flutterdec_adapter::FunctionInfo {
            id: 3,
            name: "layout".to_string(),
            owner_class: "RenderObject".to_string(),
            entry_va: 0x3000,
            size: 4,
            code_section_va: 0x3000,
        name_kind: None,
        };
        assert_eq!(
            canonical_standard_model_name(&render_fn, &class_lib).as_deref(),
            Some("flutter_rendering_RenderObject_layout")
        );

        let generic_fn = flutterdec_adapter::FunctionInfo {
            id: 4,
            name: "sub_1234".to_string(),
            owner_class: "State".to_string(),
            entry_va: 0x4000,
            size: 4,
            code_section_va: 0x4000,
        name_kind: None,
        };
        assert!(canonical_standard_model_name(&generic_fn, &class_lib).is_none());
    }

    #[test]
    fn aggregates_semantic_intent_counts_from_pseudocode() {
        let pseudo = vec![
            PseudocodeArtifact {
                function_id: 1,
                function_name: "f1".to_string(),
                source: r#"dynamic f1(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = flutter.widgets.KeyedSubtree.new(arg0, arg1, arg2, arg3); // framework:flutter.widgets.KeyedSubtree.new [selector]
  final t2 = dart.core.List.removeAt(arg0, arg1, arg2, arg3); // stdlib:dart.core.List.removeAt [selector]
  final t3 = vm_runtime_Invoke(arg0, arg1, arg2, arg3); // runtime:dart_vm.invoke
  final t4 = native_libc_memcpy(arg0, arg1, arg2, arg3); // native:libc.memcpy
  return t4;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 4,
                indirect_calls: 0,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 0,
                target_va_symbol_calls: 0,
            },
            PseudocodeArtifact {
                function_id: 2,
                function_name: "f2".to_string(),
                source: r#"dynamic f2(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.invoke(arg0, arg1, arg2, arg3); // indirect via: dispatchTarget
  return t1;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 1,
                indirect_calls: 1,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 0,
                target_va_symbol_calls: 0,
            },
        ];

        let summary = collect_semantic_intent_summary(&pseudo);
        assert_eq!(summary.framework, 1);
        assert_eq!(summary.stdlib, 1);
        assert_eq!(summary.runtime, 1);
        assert_eq!(summary.native, 1);
        assert_eq!(summary.selector_tagged, 2);
        assert_eq!(summary.constructor_calls, 1);
    }

    #[test]
    fn summarizes_selector_fallback_counts_from_pseudocode() {
        let pseudo = vec![
            PseudocodeArtifact {
                function_id: 1,
                function_name: "f1".to_string(),
                source: r#"dynamic f1(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.current(arg0, arg1, arg2, arg3); // selector: current, indirect via: dispatchTarget
  final t2 = dispatch.current(arg0, arg1, arg2, arg3); // selector: current, indirect via: dispatchTarget
  return t2;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 2,
                indirect_calls: 2,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 2,
                target_va_symbol_calls: 0,
            },
            PseudocodeArtifact {
                function_id: 2,
                function_name: "f2".to_string(),
                source: r#"dynamic f2(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.customAction(arg0, arg1, arg2, arg3); // selector: customAction, indirect via: indirectTarget9
  return t1;
}"#
                .to_string(),
                placeholder_ifs: 0,
                unresolved_cf: 0,
                raw_register_calls: 0,
                total_calls: 1,
                indirect_calls: 1,
                semantic_direct_calls: 0,
                semantic_indirect_calls: 0,
                dispatch_selector_calls: 1,
                target_va_symbol_calls: 0,
            },
        ];

        let summary = collect_selector_fallback_summary(&pseudo);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.unique, 2);
        assert_eq!(summary.top.first().map(|v| v.selector.as_str()), Some("current"));
        assert_eq!(summary.top.first().map(|v| v.count), Some(2));
        assert!(
            summary
                .top
                .first()
                .map(|v| v.sample.contains("dispatch.current("))
                .unwrap_or(false)
        );
        assert_eq!(
            summary.top.get(1).map(|v| v.selector.as_str()),
            Some("customAction")
        );
        assert_eq!(summary.top.get(1).map(|v| v.count), Some(1));
    }

    #[test]
    fn summarizes_call_fallback_counts_from_pseudocode() {
        let pseudo = vec![PseudocodeArtifact {
            function_id: 1,
            function_name: "f1".to_string(),
            source: r#"dynamic f1(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {
  final t1 = dispatch.invoke(arg0, arg1, arg2, arg3); // indirect via: dispatchTarget
  final t0 = dispatch.icon(arg0, arg1, arg2, arg3); // selector: icon, indirect via: dispatchTarget, target: dispatchTargetFn
  final t11 = dispatchTargetFn(arg0, arg1, arg2, arg3); // indirect via: dispatchTarget
  final t2 = indirectTarget9(arg0, arg1, arg2, arg3); // indirect via: indirectTarget9
  final t3 = dynamicCall(opaqueTarget, [arg0, arg1, arg2, arg3]); // target: opaqueTarget
  return t3;
}"#
            .to_string(),
            placeholder_ifs: 0,
            unresolved_cf: 0,
            raw_register_calls: 0,
            total_calls: 5,
            indirect_calls: 5,
            semantic_direct_calls: 0,
            semantic_indirect_calls: 0,
            dispatch_selector_calls: 0,
            target_va_symbol_calls: 0,
        }];

        let summary = collect_call_fallback_summary(&pseudo);
        assert_eq!(summary.dynamic_call, 1);
        assert_eq!(summary.dispatch_invoke, 1);
        assert_eq!(summary.dispatch_target_invoke, 1);
        assert_eq!(summary.generic_invoke, 1);
    }

    #[test]
    fn discovers_bootflow_candidates_from_pool_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "bootflow:main:main".to_string(),
                    decoded_kind: Some("BootMainCandidate".to_string()),
                    selector: Some("main".to_string()),
                    target_va: Some(0x1000),
                    owner_class: Some("Global".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 2,
                    kind: "String".to_string(),
                    value: "bootflow:deeplink:onNewIntent".to_string(),
                    decoded_kind: Some("DeepLinkHandlerCandidate".to_string()),
                    selector: Some("onNewIntent".to_string()),
                    target_va: Some(0x1010),
                    owner_class: Some("RouterHost".to_string()),
                    library_uri: Some("package:app/router.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 3,
                    kind: "String".to_string(),
                    value: "bootflow:activity:onResume".to_string(),
                    decoded_kind: Some("ActivityHandlerCandidate".to_string()),
                    selector: Some("onResume".to_string()),
                    target_va: Some(0x1020),
                    owner_class: Some("MainActivityHost".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 4,
                    kind: "String".to_string(),
                    value: "bootflow:init:ensureInitialized".to_string(),
                    decoded_kind: Some("BootstrapInitCandidate".to_string()),
                    selector: Some("ensureInitialized".to_string()),
                    target_va: Some(0x1030),
                    owner_class: Some("Global".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                    confidence: None,
                    source: None,
                },
            ],
        };

        let summary = collect_bootflow_discovery(&model);
        assert_eq!(summary.main.len(), 1);
        assert_eq!(summary.runapp.len(), 0);
        assert_eq!(summary.deeplink.len(), 1);
        assert_eq!(summary.activity.len(), 2);
        assert_eq!(summary.bootstrap.len(), 1);
        assert_eq!(summary.main[0].target_va, 0x1000);
        assert_eq!(summary.deeplink[0].selector, "onNewIntent");
        assert!(
            summary
                .activity
                .iter()
                .any(|entry| entry.selector == "onResume")
        );
        assert!(
            summary
                .activity
                .iter()
                .any(|entry| entry.selector == "onNewIntent")
        );
        assert_eq!(summary.bootstrap[0].selector, "ensureInitialized");
    }

    #[test]
    fn dedupes_bootflow_entries_with_same_target_and_selector() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "entrypoint:main".to_string(),
                    decoded_kind: Some("EntryPointCandidate".to_string()),
                    selector: Some("main".to_string()),
                    target_va: Some(0x2000),
                    owner_class: Some("Global".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 2,
                    kind: "String".to_string(),
                    value: "bootflow:main:main".to_string(),
                    decoded_kind: Some("BootMainCandidate".to_string()),
                    selector: Some("main".to_string()),
                    target_va: Some(0x2000),
                    owner_class: Some("Global".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                    confidence: None,
                    source: None,
                },
            ],
        };

        let summary = collect_bootflow_discovery(&model);
        assert_eq!(summary.main.len(), 1);
        assert_eq!(summary.main[0].target_va, 0x2000);
        assert_eq!(summary.main[0].selector, "main");
    }

    #[test]
    fn enriches_model_with_manifest_synthetic_bootflow_hints() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![flutterdec_adapter::LibraryInfo {
                id: 1,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 1,
                    name: "Global".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 2,
                    name: "MainActivity".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 3,
                    name: "SettingsMapper".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/model/settings.dart".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 2,
                    name: "runApp".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1004,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 3,
                    name: "onNewIntent".to_string(),
                    owner_class: "MainActivity".to_string(),
                    entry_va: 0x1008,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 4,
                    name: "onResume".to_string(),
                    owner_class: "MainActivity".to_string(),
                    entry_va: 0x100c,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
                flutterdec_adapter::FunctionInfo {
                    id: 5,
                    name: "ensureInitialized".to_string(),
                    owner_class: "SettingsMapper".to_string(),
                    entry_va: 0x1010,
                    size: 4,
                    code_section_va: 0x1000,
                name_kind: None,
                },
            ],
            object_pool: Vec::new(),
        };
        let signals = AndroidManifestSignals {
            package_name: Some("com.example.app".to_string()),
            has_main_launcher: true,
            has_view_browsable: true,
            activities: vec!["com.example.app.MainActivity".to_string()],
            deeplink_entries: vec!["myapp://open".to_string()],
        };

        let (enriched, inserted) = enrich_model_with_manifest_bootflow_hints(&model, &signals);
        assert!(inserted >= 4);

        let kinds = enriched
            .object_pool
            .iter()
            .filter_map(|e| e.decoded_kind.as_deref())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"ManifestMainCandidate"));
        assert!(kinds.contains(&"ManifestRunAppCandidate"));
        assert!(kinds.contains(&"ManifestDeepLinkCandidate"));
        assert!(kinds.contains(&"ManifestActivityCandidate"));
        assert!(!kinds.contains(&"ManifestBootstrapCandidate"));
    }

    #[test]
    fn decompile_engine_profile_light_is_minimal() {
        let cfg = DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Light);
        assert!(!cfg.canonical_model_symbols);
        assert!(!cfg.pool_value_hints);
        assert!(!cfg.pool_semantic_hints);
        assert!(!cfg.semantic_reporting);
        assert!(!cfg.bootflow_category_seeds);
    }

    #[test]
    fn decompile_engine_overrides_can_disable_balanced_defaults() {
        let base = DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Balanced);
        assert!(base.bootflow_category_seeds);
        let overrides = DecompileEngineOptionOverrides {
            canonical_model_symbols: Some(false),
            pool_value_hints: None,
            pool_semantic_hints: Some(false),
            semantic_reporting: Some(false),
            bootflow_category_seeds: Some(false),
        };
        let cfg = base.with_overrides(&overrides);
        assert!(!cfg.canonical_model_symbols);
        assert!(cfg.pool_value_hints);
        assert!(!cfg.pool_semantic_hints);
        assert!(!cfg.semantic_reporting);
        assert!(!cfg.bootflow_category_seeds);
    }

    #[test]
    fn builds_pool_semantic_hints_from_adapter_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 7,
                    kind: "String".to_string(),
                    value: "didChangeMetrics".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("didChangeMetrics".to_string()),
                    target_va: Some(0x1234),
                    owner_class: Some("WidgetsBindingObserver".to_string()),
                    library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 8,
                    kind: "Smi".to_string(),
                    value: "42".to_string(),
                    decoded_kind: None,
                    selector: None,
                    target_va: None,
                    owner_class: None,
                    library_uri: None,
                    confidence: None,
                    source: None,
                },
            ],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        assert_eq!(hints.len(), 1);
        let h = hints.get(&7).expect("missing semantic hint entry");
        assert_eq!(h.selector.as_deref(), Some("didChangeMetrics"));
        assert_eq!(h.owner_class.as_deref(), Some("WidgetsBindingObserver"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/binding.dart")
        );
        assert_eq!(h.target_va, Some(0x1234));
    }

    #[test]
    fn collects_pool_metadata_coverage_stats() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "a".to_string(),
                    decoded_kind: None,
                    selector: Some("setState".to_string()),
                    target_va: Some(0x1000),
                    owner_class: Some("State".to_string()),
                    library_uri: Some("package:flutter/src/widgets/framework.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 2,
                    kind: "Smi".to_string(),
                    value: "42".to_string(),
                    decoded_kind: None,
                    selector: None,
                    target_va: None,
                    owner_class: None,
                    library_uri: None,
                    confidence: None,
                    source: None,
                },
            ],
        };

        let stats = collect_pool_metadata_stats(&model);
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.with_target_va, 1);
        assert_eq!(stats.with_selector, 1);
        assert_eq!(stats.with_owner_class, 1);
        assert_eq!(stats.with_library_uri, 1);
    }

    #[test]
    fn builds_pool_target_symbols_from_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: Vec::new(),
            functions: Vec::new(),
            object_pool: vec![
                flutterdec_adapter::ObjectPoolEntry {
                    index: 7,
                    kind: "String".to_string(),
                    value: "didChangeMetrics".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("didChangeMetrics".to_string()),
                    target_va: Some(0x1234),
                    owner_class: Some("WidgetsBindingObserver".to_string()),
                    library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 8,
                    kind: "String".to_string(),
                    value: "Int64List".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("Int64List".to_string()),
                    target_va: Some(0x2234),
                    owner_class: Some("Int64List".to_string()),
                    library_uri: Some("dart:typed_data".to_string()),
                    confidence: None,
                    source: None,
                },
                flutterdec_adapter::ObjectPoolEntry {
                    index: 9,
                    kind: "String".to_string(),
                    value: "executeCommandAsync".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("executeCommandAsync".to_string()),
                    target_va: Some(0x3234),
                    owner_class: Some("ConnectService".to_string()),
                    library_uri: Some("package:spotube/models/connect/load.dart".to_string()),
                    confidence: None,
                    source: None,
                },
            ],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        let values = build_pool_value_hints(&model);
        let map = build_pool_target_symbols(&hints, &values);
        assert_eq!(
            map.get(&0x1234).map(String::as_str),
            Some("flutter_widgets_WidgetsBindingObserver_didChangeMetrics")
        );
        assert_eq!(
            map.get(&0x2234).map(String::as_str),
            Some("dart_typed_data_Int64List_new")
        );
        assert_eq!(
            map.get(&0x3234).map(String::as_str),
            Some("package_spotube_ConnectService_executeCommandAsync")
        );
    }

    #[test]
    fn enriches_pool_semantic_hints_from_function_metadata() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "python".to_string(),
            dart_version: "3.0.0".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 1,
                name: "State".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:flutter/src/widgets/framework.dart".to_string(),
            }],
            functions: vec![flutterdec_adapter::FunctionInfo {
                id: 11,
                name: "setState".to_string(),
                owner_class: "State".to_string(),
                entry_va: 0x4000,
                size: 4,
                code_section_va: 0x4000,
            name_kind: None,
            }],
            object_pool: vec![flutterdec_adapter::ObjectPoolEntry {
                index: 21,
                kind: "Closure".to_string(),
                value: "opaque".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: Some(0x4000),
                owner_class: None,
                library_uri: None,
                confidence: None,
                source: None,
            }],
        };

        let class_to_library = build_class_library_lookup(&model);
        let hints = build_pool_semantic_hints(&model, &class_to_library);
        let h = hints.get(&21).expect("missing enriched semantic hint");
        assert_eq!(h.selector.as_deref(), Some("setState"));
        assert_eq!(h.owner_class.as_deref(), Some("State"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/framework.dart")
        );
        assert_eq!(h.target_va, Some(0x4000));
    }

    #[test]
    fn target_filter_matches_function_id_without_scope_override() {
        let full_model = ProgramModel {
            schema_version: 3,
            adapter_kind: "python".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 1,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![flutterdec_adapter::FunctionInfo {
                id: 42,
                name: "main".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1000,
                size: 4,
                code_section_va: 0x1000,
                name_kind: Some("heuristic".to_string()),
            }],
            object_pool: Vec::new(),
        };
        let scoped_model = full_model.clone();
        let (selected, stats) = apply_target_function_filter(
            &full_model,
            &scoped_model,
            FunctionTarget::FunctionId(42),
        )
        .expect("target filter");

        assert!(stats.enabled);
        assert!(!stats.scope_overridden);
        assert_eq!(stats.matched_count, 1);
        assert_eq!(selected.functions.len(), 1);
        assert_eq!(selected.functions[0].id, 42);
    }

    #[test]
    fn target_filter_can_override_scope_for_entry_va() {
        let full_model = ProgramModel {
            schema_version: 3,
            adapter_kind: "python".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![
                flutterdec_adapter::ClassInfo {
                    id: 1,
                    name: "AppClass".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
                flutterdec_adapter::ClassInfo {
                    id: 2,
                    name: "CoreClass".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "dart:core".to_string(),
                },
            ],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "AppClass".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: Some("heuristic".to_string()),
                },
                flutterdec_adapter::FunctionInfo {
                    id: 2,
                    name: "coreFn".to_string(),
                    owner_class: "CoreClass".to_string(),
                    entry_va: 0x2000,
                    size: 4,
                    code_section_va: 0x2000,
                    name_kind: Some("heuristic".to_string()),
                },
            ],
            object_pool: Vec::new(),
        };
        let (scoped_model, _) = apply_function_scope_filter(&full_model, FunctionScope::App, &[]);
        assert_eq!(scoped_model.functions.len(), 1);

        let (selected, stats) =
            apply_target_function_filter(&full_model, &scoped_model, FunctionTarget::EntryVa(0x2000))
                .expect("target filter");

        assert!(stats.enabled);
        assert!(stats.scope_overridden);
        assert_eq!(stats.matched_count, 1);
        assert_eq!(selected.functions.len(), 1);
        assert_eq!(selected.functions[0].entry_va, 0x2000);
    }

    #[test]
    fn target_filter_rejects_ambiguous_any_selector() {
        let full_model = ProgramModel {
            schema_version: 3,
            adapter_kind: "python".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "deadbeef".to_string(),
            arch: "arm64".to_string(),
            libraries: Vec::new(),
            classes: vec![flutterdec_adapter::ClassInfo {
                id: 1,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                flutterdec_adapter::FunctionInfo {
                    id: 42,
                    name: "fnA".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                    name_kind: Some("heuristic".to_string()),
                },
                flutterdec_adapter::FunctionInfo {
                    id: 7,
                    name: "fnB".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 42,
                    size: 4,
                    code_section_va: 42,
                    name_kind: Some("heuristic".to_string()),
                },
            ],
            object_pool: Vec::new(),
        };
        let scoped_model = full_model.clone();

        let err = apply_target_function_filter(&full_model, &scoped_model, FunctionTarget::Any(42))
            .expect_err("ambiguous");
        assert!(err.to_string().contains("ambiguous"));
    }

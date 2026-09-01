    use super::*;
    use flutterdec_adapter::model::{
        Capabilities, CapabilityLevel, Class, ClassId, CodeRange, CompatibilityBinding, Function,
        Diagnostic, Domain, FunctionId, InputRegion, InputRegionName, Library, LibraryId, Name,
        ObjectPool, ObservedInput, PoolEntry, PoolEntryKind, PoolGeometry, PoolIndexSpace,
        Producer, ProducerTrust, Provenance, MODEL_VERSION,
    };
    use flutterdec_adapter::primitives::Sha256Digest;
    use flutterdec_adapter::validate::{validate, HostSelectedContext};
    use flutterdec_disasm_arm64::{
        AsmInstruction, FunctionDisassembly, FunctionPriorityComponent, Hint, HintKind, HintOrigin,
        HintProvenance, ProgramHints,
    };
    use flutterdec_loader::identity::{SnapshotIdentity, SnapshotKind, TargetArch};
    use tempfile::tempdir;

    const ARM64_POOL_GEOMETRY: PoolGeometry = PoolGeometry {
        entries_offset: 0x10,
        word_size: 8,
    };

    /// Fixture builders for v4 models.
    ///
    /// Nothing here can produce a named function without being told the name, or
    /// a class without a library id that resolves, which is the point: the cases
    /// these tests cover include "the producer recovered nothing".
    fn lib(id: u32, uri: &str) -> Library {
        Library {
            id: LibraryId(id),
            uri: uri.to_string(),
            display_name: None,
            provenance: Provenance::Exact,
        }
    }

    fn cls(id: u32, name: &str, library: Option<u32>) -> Class {
        Class {
            id: ClassId(id),
            name: name.to_string(),
            library: library.map(LibraryId),
            super_class: None,
            provenance: Provenance::Exact,
        }
    }

    fn fun(
        id: u32,
        name: Option<Name>,
        owner: Option<u32>,
        start_va: u64,
        size: u64,
    ) -> Function {
        Function {
            id: FunctionId(id),
            name,
            owner: owner.map(ClassId),
            code: CodeRange { start_va, size },
            code_section_va: start_va,
            provenance: Provenance::Exact,
        }
    }

    fn named(text: &str) -> Option<Name> {
        Some(Name::exact(text))
    }

    fn pool_string(index: u64, value: &str) -> PoolEntry {
        PoolEntry {
            index,
            kind: PoolEntryKind::String,
            value: Some(value.to_string()),
            target_va: None,
            provenance: Provenance::Exact,
            confidence: None,
        }
    }

    fn pool_selector(index: u64, selector: &str, target_va: u64) -> PoolEntry {
        PoolEntry {
            index,
            kind: PoolEntryKind::Selector,
            value: Some(selector.to_string()),
            target_va: Some(target_va),
            provenance: Provenance::Exact,
            confidence: None,
        }
    }

    fn ordinal_pool(entries: Vec<PoolEntry>) -> ObjectPool {
        ObjectPool {
            index_space: PoolIndexSpace::Ordinal,
            geometry: None,
            entries,
        }
    }

    fn hardware_pool(entries: Vec<PoolEntry>) -> ObjectPool {
        ObjectPool {
            index_space: PoolIndexSpace::Hardware,
            geometry: Some(ARM64_POOL_GEOMETRY),
            entries,
        }
    }

    fn hint(
        kind: HintKind,
        origin: HintOrigin,
        selector: &str,
        target_va: Option<u64>,
        owner_class: Option<&str>,
        library_uri: Option<&str>,
    ) -> Hint {
        Hint {
            kind,
            origin,
            provenance: HintProvenance::Derived,
            selector: selector.to_string(),
            target_va,
            owner_class: owner_class.map(str::to_string),
            library_uri: library_uri.map(str::to_string),
            detail: String::new(),
        }
    }

    fn program_hints(entries: Vec<Hint>) -> ProgramHints {
        let mut hints = ProgramHints::new();
        for entry in entries {
            hints.push(entry);
        }
        hints
    }

    fn test_model(
        libraries: Vec<Library>,
        classes: Vec<Class>,
        functions: Vec<Function>,
        object_pool: ObjectPool,
    ) -> ProgramModel {
        let digest = Sha256Digest::of(b"core fixture");
        ProgramModel {
            model_version: MODEL_VERSION,
            producer: Producer {
                id: "core-fixture".to_string(),
                version: "0".to_string(),
                artifact_sha256: digest.clone(),
                trust: ProducerTrust::Untrusted,
            },
            input: ObservedInput {
                identity: SnapshotIdentity::from_header(
                    TargetArch::Arm64,
                    "80a49c7111088100a233b2ae788e1f48",
                    SnapshotKind::FullAot,
                    "product arm64 compressed-pointers",
                ),
                regions: vec![InputRegion {
                    region: InputRegionName::IsolateInstructions,
                    size: u64::MAX / 2,
                    sha256: digest.clone(),
                    virtual_address: Some(0),
                    executable: true,
                }],
            },
            compatibility: Some(CompatibilityBinding {
                record_sha256: digest.clone(),
                parser_family_id: "fixture".to_string(),
                profile_id: "fixture".to_string(),
                profile_sha256: digest,
            }),
            capabilities: Capabilities {
                libraries: CapabilityLevel::Partial,
                classes: CapabilityLevel::Partial,
                class_relationships: CapabilityLevel::Unavailable,
                functions: CapabilityLevel::Partial,
                function_names: CapabilityLevel::Partial,
                object_pool: CapabilityLevel::Partial,
                pool_index_space: CapabilityLevel::Unavailable,
            },
            libraries,
            classes,
            functions,
            object_pool,
            diagnostics: Vec::new(),
            extensions: Default::default(),
        }
    }

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
    fn quality_gate_failure_message_explains_strict_placeholder_rejection() {
        let report = QualityReport {
            mode: "strict".to_string(),
            passed: false,
            failures: vec!["placeholder if-count exceeded threshold".to_string()],
            function_count: 5394,
            disassembled_function_count: 5394,
            disassembly_ratio: 1.0,
            total_calls: 77037,
            indirect_calls: 9674,
            indirect_call_ratio: 0.12557602191154899,
            placeholder_ifs: 1691,
            unresolved_cf: 0,
            raw_register_calls: 9674,
            semantic_direct_calls: 37,
            semantic_indirect_calls: 10,
            dispatch_selector_calls: 1132,
            dispatch_table_calls: 1132,
            repeated_blocks: 0,
            unlifted_instructions: 0,
            target_va_symbol_calls: 0,
            block_helper_refs: 0,
            raw_arg_name_refs: 0,
            raw_register_name_refs: 0,
            placeholder_cond_markers: 1618,
            omitted_path_markers: 827,
            loop_backedge_markers: 1,
        };
        let symbol_quality_counts = SymbolQualityCounts {
            placeholder: 5394,
            heuristic: 0,
            external: 0,
            exact: 0,
        };

        let msg = format_quality_gate_failure_message(
            &report,
            std::path::Path::new("./out/quality.json"),
            std::path::Path::new("./out/report.json"),
            std::path::Path::new("libapp.so"),
            Some(AdapterBackend::Internal),
            None,
            &symbol_quality_counts,
        );

        assert!(msg.contains("quality gate failed after artifact generation"));
        assert!(msg.contains("./out/quality.json"));
        assert!(msg.contains("./out/report.json"));
        assert!(msg.contains("placeholder if-count exceeded threshold"));
        assert!(msg.contains("input is not an APK"));
        assert!(msg.contains("resolved backend is internal"));
        assert!(msg.contains("all recovered function names are still placeholders"));
        assert!(msg.contains("--max-placeholder-ifs 999999"));
        assert!(msg.contains("artifacts were still written"));
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
            function_name: Some("main".to_string()),
            owner_class: None,
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
    fn symbol_quality_follows_the_models_name_provenance() {
        assert_eq!(
            symbol_name_quality_from_provenance(Provenance::Exact),
            SymbolNameQuality::Exact
        );
        assert_eq!(
            symbol_name_quality_from_provenance(Provenance::Derived),
            SymbolNameQuality::External
        );
        assert_eq!(
            symbol_name_quality_from_provenance(Provenance::Heuristic),
            SymbolNameQuality::Heuristic
        );
    }

    /// A function with no name is counted as unnamed, not as a placeholder-named
    /// one. v3 could not express the difference, because every function had to
    /// carry a name string.
    #[test]
    fn counts_function_name_provenance_including_unnamed() {
        let functions = vec![
            fun(1, named("main"), None, 0x1000, 16),
            fun(
                2,
                Some(Name {
                    text: "native_libc_printf".to_string(),
                    provenance: Provenance::Derived,
                    confidence: None,
                }),
                None,
                0x1010,
                16,
            ),
            fun(
                3,
                Some(Name {
                    text: "build".to_string(),
                    provenance: Provenance::Heuristic,
                    confidence: None,
                }),
                None,
                0x1020,
                16,
            ),
            fun(4, None, None, 0x1030, 16),
            fun(5, None, None, 0x1040, 16),
        ];
        let stats = collect_function_name_provenance_stats(&functions);
        assert_eq!(stats.exact, 1);
        assert_eq!(stats.derived, 1);
        assert_eq!(stats.heuristic, 1);
        assert_eq!(stats.unnamed, 2);
        assert_eq!(stats.named(), 3);
    }

    #[test]
    fn collects_function_descriptors_with_library_context() {
        let model = test_model(
            vec![lib(0, "package:spotube/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, named("main"), Some(0), 0x1000, 16),
                // No library, no owner, no name: nothing that survives a
                // rebuild, so it is counted as uncomparable rather than folded
                // into a `::` descriptor that reads as one unchanged function.
                fun(1, None, None, 0x2000, 16),
            ],
            ordinal_pool(Vec::new()),
        );

        let (descriptors, uncomparable) = collect_function_descriptors(&model);
        assert!(descriptors.contains("package:spotube/main.dart::AppRoot::main"));
        assert!(!descriptors.contains("::"));
        assert_eq!(uncomparable, 1);
    }

    /// A class whose library did not parse is a supported v4 state, and the
    /// owner is not a library URI. Dropping the empty library segment would
    /// leave `AppRoot::main`, which reads back as the library `AppRoot` and
    /// files the function under a package derived from the class name.
    #[test]
    fn an_unrecovered_library_is_an_empty_segment_not_the_owner() {
        let model = test_model(
            Vec::new(),
            vec![cls(0, "AppRoot", None)],
            vec![fun(0, named("main"), Some(0), 0x1000, 16)],
            ordinal_pool(Vec::new()),
        );

        let (descriptors, uncomparable) = collect_function_descriptors(&model);
        assert!(descriptors.contains("::AppRoot::main"));
        assert_eq!(uncomparable, 0);

        let counts = collect_diff_package_counts(&descriptors.iter().cloned().collect::<Vec<_>>());
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].package, "unknown");
        assert_eq!(counts[0].functions, 1);
    }

    #[test]
    fn canonicalizes_flutter_build_file_uri_in_descriptors() {
        let model = test_model(
            vec![lib(
                0,
                "file:///tmp/build/app/.dart_tool/flutter_build/dart_plugin_registrant.dart",
            )],
            vec![cls(0, "_PluginRegistrant", Some(0))],
            vec![fun(0, named("register"), Some(0), 0x3000, 16)],
            ordinal_pool(Vec::new()),
        );

        let (descriptors, uncomparable) = collect_function_descriptors(&model);
        assert!(descriptors.contains(
            "file:///.dart_tool/flutter_build/dart_plugin_registrant.dart::_PluginRegistrant::register"
        ));
        assert_eq!(uncomparable, 0);
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
    fn collects_compatibility_warnings_from_flags() {
        let warnings = collect_compatibility_warnings(false, false, true, None);
        assert_eq!(warnings.len(), 3);
        assert!(warnings
            .iter()
            .any(|w| w.contains("compatibility registry record missing")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("not header-derived")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("backend differs")));

        let warnings = collect_compatibility_warnings(true, true, false, None);
        assert!(warnings.is_empty());

        // A core fallback is the loudest of the four: it says nothing parsed
        // the snapshot, and what that costs.
        let warnings = collect_compatibility_warnings(
            true,
            true,
            false,
            Some(CoreFallbackReason::NoCompatibilityRecord),
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no compatibility record matches"));
        assert!(warnings[0].contains("no authoritative ObjectPool index space"));
    }

    /// The resolved backend is whatever the protocol result named, mapped
    /// through a closed enum. No string is consulted, which is what makes a
    /// producer calling itself `serwalker` or `r2flutter_snapshot_v1` inert.
    #[test]
    fn resolved_backend_comes_from_the_typed_protocol_result() {
        assert_eq!(
            backend_from_id(BackendId::Blutter),
            AdapterBackend::Blutter
        );
        assert_eq!(
            backend_from_id(BackendId::R2Flutter),
            AdapterBackend::R2Flutter
        );
        assert_eq!(
            backend_from_id(BackendId::Internal),
            AdapterBackend::Internal
        );
    }

    /// A pinned backend maps to a `Fixed` request, which the protocol refuses to
    /// let a producer substitute. `auto` is the only request that may fall back.
    #[test]
    fn requested_backend_pins_everything_except_auto() {
        assert_eq!(
            requested_backend(AdapterBackend::Auto),
            RequestedBackend::Auto
        );
        assert_eq!(
            requested_backend(AdapterBackend::Blutter),
            RequestedBackend::Fixed(BackendId::Blutter)
        );
        assert_eq!(
            requested_backend(AdapterBackend::R2Flutter),
            RequestedBackend::Fixed(BackendId::R2Flutter)
        );
        assert_eq!(
            requested_backend(AdapterBackend::Internal),
            RequestedBackend::Fixed(BackendId::Internal)
        );
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
    fn resolves_local_engine_symbol_targets_only_for_apk_inputs() {
        let td = tempdir().expect("tempdir");
        let repo_root = td.path();
        let build_id_path = repo_root.join("symbols/by-build-id/abc123/symbol_target_summary.json");
        std::fs::create_dir_all(build_id_path.parent().expect("parent")).expect("mkdir cache");
        std::fs::write(&build_id_path, "[]").expect("write target summary");
        let manifest = LocalSymbolCacheManifest {
            entries: vec![LocalSymbolCacheEntry {
                arch: "arm64".to_string(),
                build_id: Some("abc123".to_string()),
                flutter_version: Some("3.24.0".to_string()),
                dart_version: Some("3.5.0".to_string()),
                build_id_target_summary_path: Some(
                    "symbols/by-build-id/abc123/symbol_target_summary.json".to_string(),
                ),
                version_target_summary_path: None,
                report_path: None,
            }],
        };
        write_local_symbol_cache_manifest(&repo_root.join("symbols"), &manifest)
            .expect("write manifest");
        let engine_context = EngineFingerprintContext {
            detected: true,
            build_id: Some("abc123".to_string()),
            candidate_flutter_version: Some("3.24.0".to_string()),
            ..EngineFingerprintContext::default()
        };

        let apk_resolution = resolve_local_engine_symbol_targets(
            &repo_root.join("symbols"),
            Path::new("sample.apk"),
            "arm64",
            &engine_context,
        );
        assert!(apk_resolution.enabled);
        assert_eq!(apk_resolution.match_kind.as_deref(), Some("build_id"));
        assert_eq!(apk_resolution.loaded_paths, vec![build_id_path]);

        let so_resolution = resolve_local_engine_symbol_targets(
            &repo_root.join("symbols"),
            Path::new("libapp.so"),
            "arm64",
            &engine_context,
        );
        assert!(!so_resolution.enabled);
        assert!(so_resolution.loaded_paths.is_empty());
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
                function_name: Some("main".to_string()),
                owner_class: None,
                library_uri: Some("package:spotube/main.dart".to_string()),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: Some("init".to_string()),
                owner_class: None,
                library_uri: Some("package:spotube/services/init.dart".to_string()),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: Some("watch".to_string()),
                owner_class: Some("Provider".to_string()),
                library_uri: Some("package:provider/src/provider.dart".to_string()),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: Some("toString".to_string()),
                owner_class: Some("Object".to_string()),
                library_uri: Some("dart:core".to_string()),
                entry_va: 0x1030,
                total_score: 70,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 5,
                function_name: Some("sub_1040".to_string()),
                owner_class: Some("Unknown".to_string()),
                library_uri: None,
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
                function_name: Some("main".to_string()),
                owner_class: None,
                library_uri: Some("package:spotube/main.dart".to_string()),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: Some("setState".to_string()),
                owner_class: Some("State".to_string()),
                library_uri: Some("package:flutter/src/widgets/framework.dart".to_string()),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: Some("toString".to_string()),
                owner_class: Some("Object".to_string()),
                library_uri: Some("dart:core".to_string()),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: Some("sub_1030".to_string()),
                owner_class: Some("Unknown".to_string()),
                library_uri: None,
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
                function_name: Some("main".to_string()),
                owner_class: None,
                library_uri: Some("package:app/main.dart".to_string()),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: Some("init".to_string()),
                owner_class: None,
                library_uri: Some("package:spotube/main.dart".to_string()),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 3,
                function_name: Some("watch".to_string()),
                owner_class: Some("Provider".to_string()),
                library_uri: Some("package:provider/src/provider.dart".to_string()),
                entry_va: 0x1020,
                total_score: 80,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 4,
                function_name: Some("toString".to_string()),
                owner_class: Some("Object".to_string()),
                library_uri: Some("dart:core".to_string()),
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
                function_name: Some("main".to_string()),
                owner_class: None,
                library_uri: Some("package:app/main.dart".to_string()),
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
                function_name: Some("sub_1010".to_string()),
                owner_class: Some("Provider".to_string()),
                library_uri: Some("package:provider/src/provider.dart".to_string()),
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
                function_name: Some("main".to_string()),
                owner_class: None,
                library_uri: Some("package:app/main.dart".to_string()),
                entry_va: 0x1000,
                total_score: 100,
                components: Vec::new(),
            },
            FunctionPriorityBreakdown {
                function_id: 2,
                function_name: Some("runApp".to_string()),
                owner_class: None,
                library_uri: Some("package:app/main.dart".to_string()),
                entry_va: 0x1010,
                total_score: 90,
                components: Vec::new(),
            },
        ];
        let bootflow = BootflowDiscoverySummary {
            main: vec![
                BootflowDiscoveryEntry {
                    kind: "pool".to_string(),
                    provenance: "derived".to_string(),
                    source: "adapter".to_string(),
                    selector: "main".to_string(),
                    target_va: Some(0x1000),
                    owner_class: None,
                    library_uri: Some("package:app/main.dart".to_string()),
                    detail: "bootflow:main:main".to_string(),
                },
                BootflowDiscoveryEntry {
                    kind: "pool".to_string(),
                    provenance: "derived".to_string(),
                    source: "adapter".to_string(),
                    selector: "main".to_string(),
                    target_va: Some(0x2000),
                    owner_class: None,
                    library_uri: Some("package:app/main.dart".to_string()),
                    detail: "bootflow:main:main".to_string(),
                },
            ],
            runapp: vec![BootflowDiscoveryEntry {
                kind: "pool".to_string(),
                    provenance: "derived".to_string(),
                source: "adapter".to_string(),
                selector: "runApp".to_string(),
                target_va: Some(0x1010),
                owner_class: None,
                library_uri: Some("package:app/main.dart".to_string()),
                detail: "bootflow:runapp:runApp".to_string(),
            }],
            deeplink: vec![BootflowDiscoveryEntry {
                kind: "pool".to_string(),
                    provenance: "derived".to_string(),
                source: "adapter".to_string(),
                selector: "onNewIntent".to_string(),
                target_va: Some(0x3000),
                owner_class: Some("MainActivity".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
                detail: "bootflow:deeplink:onNewIntent".to_string(),
            }],
            activity: Vec::new(),
            bootstrap: vec![BootflowDiscoveryEntry {
                kind: "pool".to_string(),
                    provenance: "derived".to_string(),
                source: "adapter".to_string(),
                selector: "ensureInitialized".to_string(),
                target_va: Some(0x1000),
                owner_class: Some("WidgetsFlutterBinding".to_string()),
                library_uri: Some("package:flutter/src/widgets/binding.dart".to_string()),
                detail: "bootflow:init:ensureInitialized".to_string(),
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
            hit.category == "main" && hit.target_va == 0x1000 && hit.function_name.as_deref() == Some("main")
        }));
        assert!(hits.iter().any(|hit| {
            hit.category == "runapp"
                && hit.target_va == 0x1010
                && hit.function_name.as_deref() == Some("runApp")
        }));
        assert!(hits.iter().any(|hit| {
            hit.category == "bootstrap"
                && hit.target_va == 0x1000
                && hit.function_name.as_deref() == Some("main")
        }));
    }

    #[test]
    fn applies_app_unknown_scope_filter() {
        let model = test_model(
            vec![
                lib(0, "package:flutter/src/widgets/framework.dart"),
                lib(1, "dart:core"),
                lib(2, "package:spotube/models/connect/load.dart"),
            ],
            vec![
                cls(1, "State", Some(0)),
                cls(2, "_StringBase", Some(1)),
                cls(3, "ConnectService", Some(2)),
            ],
            vec![
                fun(10, named("setState"), Some(1), 0x1000, 4),
                fun(11, named("toString"), Some(2), 0x1100, 4),
                fun(12, named("executeCommandAsync"), Some(3), 0x1200, 4),
                fun(13, None, None, 0x1300, 4),
            ],
            ordinal_pool(Vec::new()),
        );

        let (scoped, stats) = apply_function_scope_filter(&model, FunctionScope::AppUnknown, &[]);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![FunctionId(12), FunctionId(13)]);
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
        let model = test_model(
            vec![lib(0, "package:spotube/models/connect/load.dart")],
            vec![cls(3, "ConnectService", Some(0))],
            vec![
                fun(12, named("executeCommandAsync"), Some(3), 0x1200, 4),
                fun(13, None, None, 0x1300, 4),
            ],
            ordinal_pool(Vec::new()),
        );

        let (scoped, stats) = apply_function_scope_filter(&model, FunctionScope::App, &[]);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![FunctionId(12)]);
        assert_eq!(stats.total_before_filter, 2);
        assert_eq!(stats.total_after_filter, 1);
        assert_eq!(stats.excluded, 1);
    }

    #[test]
    fn applies_app_package_filter_to_scoped_functions() {
        let model = test_model(
            vec![
                lib(0, "package:spotube/models/connect/load.dart"),
                lib(1, "package:provider/src/provider.dart"),
                lib(2, "package:flutter/src/widgets/framework.dart"),
            ],
            vec![
                cls(3, "ConnectService", Some(0)),
                cls(4, "ProviderCore", Some(1)),
                cls(5, "State", Some(2)),
            ],
            vec![
                fun(12, named("executeCommandAsync"), Some(3), 0x1200, 4),
                fun(13, named("watch"), Some(4), 0x1300, 4),
                fun(14, named("setState"), Some(5), 0x1400, 4),
                fun(15, None, None, 0x1500, 4),
            ],
            ordinal_pool(Vec::new()),
        );

        let app_packages = vec!["spotube".to_string()];
        let (scoped, stats) =
            apply_function_scope_filter(&model, FunctionScope::AppUnknown, &app_packages);
        let ids = scoped.functions.iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![FunctionId(12)]);
        assert_eq!(stats.total_before_filter, 4);
        assert_eq!(stats.total_after_filter, 1);
        assert_eq!(stats.excluded, 3);
        assert_eq!(stats.excluded_by_app_package, 2);
    }

    #[test]
    fn collects_app_package_function_counts() {
        let model = test_model(
            vec![
                lib(0, "package:spotube/a.dart"),
                lib(1, "package:provider/b.dart"),
                lib(2, "package:flutter/src/widgets/framework.dart"),
            ],
            vec![
                cls(1, "AppA", Some(0)),
                cls(2, "AppB", Some(1)),
                cls(3, "State", Some(2)),
            ],
            vec![
                fun(10, named("f10"), Some(1), 0x1000, 4),
                fun(11, named("f11"), Some(1), 0x1100, 4),
                fun(12, named("f12"), Some(2), 0x1200, 4),
                fun(13, named("setState"), Some(3), 0x1300, 4),
            ],
            ordinal_pool(Vec::new()),
        );

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
        let model = test_model(
            vec![
                lib(0, "dart:core"),
                lib(1, "dart:core-patch/bool_patch.dart"),
                lib(2, "package:flutter/src/widgets/framework.dart"),
                lib(3, "package:flutter/src/rendering/object.dart"),
            ],
            vec![
                cls(0, "_StringBase", Some(0)),
                cls(1, "_BoolPatch", Some(1)),
                cls(2, "State", Some(2)),
                cls(3, "RenderObject", Some(3)),
            ],
            vec![
                fun(1, named("toString"), Some(0), 0x1000, 4),
                fun(5, named("fromEnvironment"), Some(1), 0x1800, 4),
                fun(2, named("setState"), Some(2), 0x2000, 4),
                fun(3, named("layout"), Some(3), 0x3000, 4),
                // No name at all: there is nothing to canonicalize, which is the
                // case v3 expressed as the fabricated name `sub_1234`.
                fun(4, None, Some(2), 0x4000, 4),
            ],
            ordinal_pool(Vec::new()),
        );
        let canonical = |id: u32| {
            let f = model
                .functions
                .iter()
                .find(|f| f.id == FunctionId(id))
                .expect("fixture function");
            canonical_standard_model_name(&model, f)
        };

        assert_eq!(canonical(1).as_deref(), Some("dart_core_toString"));
        assert_eq!(
            canonical(5).as_deref(),
            Some("dart_core_patch_bool_patch_fromEnvironment")
        );
        assert_eq!(canonical(2).as_deref(), Some("flutter_widgets_State_setState"));
        assert_eq!(
            canonical(3).as_deref(),
            Some("flutter_rendering_RenderObject_layout")
        );
        assert!(canonical(4).is_none());
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
                dispatch_table_calls: 0,
                repeated_blocks: 0,
                unlifted_instructions: 0,
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
                dispatch_table_calls: 0,
                repeated_blocks: 0,
                unlifted_instructions: 0,
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
                dispatch_table_calls: 2,
                repeated_blocks: 0,
                unlifted_instructions: 0,
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
                dispatch_table_calls: 1,
                repeated_blocks: 0,
                unlifted_instructions: 0,
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
            dispatch_table_calls: 0,
            repeated_blocks: 0,
            unlifted_instructions: 0,
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
        let hints = program_hints(vec![hint(HintKind::BootMain, HintOrigin::ModelNamePattern, "main", Some(0x1000), Some("AppRoot"), Some("package:app/main.dart")), hint(HintKind::DeepLinkHandler, HintOrigin::ModelNamePattern, "onNewIntent", Some(0x1010), Some("RouterHost"), Some("package:app/router.dart")), hint(HintKind::ActivityHandler, HintOrigin::ModelNamePattern, "onResume", Some(0x1020), Some("MainActivityHost"), Some("package:app/main.dart")), hint(HintKind::BootstrapInit, HintOrigin::ModelNamePattern, "ensureInitialized", Some(0x1030), Some("AppRoot"), Some("package:app/main.dart"))]);

        let summary = collect_bootflow_discovery(&hints);
        assert_eq!(summary.main.len(), 1);
        assert_eq!(summary.runapp.len(), 0);
        assert_eq!(summary.deeplink.len(), 1);
        assert_eq!(summary.activity.len(), 1);
        assert_eq!(summary.bootstrap.len(), 1);
        assert_eq!(summary.main[0].target_va, Some(0x1000));
        assert_eq!(summary.main[0].source, "model_name_pattern");
        // Provenance rides along on every reported entry: nothing here is exact.
        assert_eq!(summary.main[0].provenance, "derived");
        assert_eq!(summary.deeplink[0].selector, "onNewIntent");
        assert_eq!(summary.activity[0].selector, "onResume");
        assert_eq!(summary.bootstrap[0].selector, "ensureInitialized");
    }

    #[test]
    fn dedupes_bootflow_entries_with_same_target_and_selector() {
        let hints = program_hints(vec![hint(HintKind::EntryPoint, HintOrigin::ModelNamePattern, "main", Some(0x2000), Some("AppRoot"), Some("package:app/main.dart")), hint(HintKind::BootMain, HintOrigin::ModelNamePattern, "main", Some(0x2000), Some("AppRoot"), Some("package:app/main.dart"))]);

        let summary = collect_bootflow_discovery(&hints);
        assert_eq!(summary.main.len(), 1);
        assert_eq!(summary.main[0].target_va, Some(0x2000));
        assert_eq!(summary.main[0].selector, "main");
    }

    #[test]
    fn keeps_bootflow_entries_with_same_target_and_selector_when_source_differs() {
        let hints = program_hints(vec![hint(HintKind::BootMain, HintOrigin::AndroidManifest, "main", Some(0x2000), Some("AppRoot"), Some("package:app/main.dart")), hint(HintKind::BootMain, HintOrigin::ApkStartup, "main", Some(0x2000), Some("AppRoot"), Some("package:app/main.dart"))]);

        let summary = collect_bootflow_discovery(&hints);
        assert_eq!(summary.main.len(), 2);
        assert!(summary
            .main
            .iter()
            .any(|entry| entry.source == "android_manifest"));
        assert!(summary.main.iter().any(|entry| entry.source == "apk_startup"));
    }

    /// A model that both parses and passes semantic validation, so enrichment
    /// can be checked against the real invariant rather than against a fixture
    /// that was never valid to begin with.
    fn validatable_model(
        libraries: Vec<Library>,
        classes: Vec<Class>,
        functions: Vec<Function>,
        object_pool: ObjectPool,
        capabilities: Capabilities,
        diagnostics: Vec<Diagnostic>,
    ) -> (ProgramModel, HostSelectedContext) {
        let digest = Sha256Digest::of(b"enrichment fixture");
        let identity = SnapshotIdentity::from_header(
            TargetArch::Arm64,
            "80a49c7111088100a233b2ae788e1f48",
            SnapshotKind::FullAot,
            "product arm64 compressed-pointers",
        );
        let producer = Producer {
            id: "enrichment-fixture".to_string(),
            version: "0".to_string(),
            artifact_sha256: digest.clone(),
            trust: ProducerTrust::Untrusted,
        };
        let compatibility = CompatibilityBinding {
            record_sha256: digest.clone(),
            parser_family_id: "fixture".to_string(),
            profile_id: "fixture".to_string(),
            profile_sha256: digest.clone(),
        };
        // Region order is the enum's declaration order, which is the canonical
        // order validation requires.
        let regions = vec![
            InputRegion {
                region: InputRegionName::VmData,
                size: 64,
                sha256: digest.clone(),
                virtual_address: None,
                executable: false,
            },
            InputRegion {
                region: InputRegionName::IsolateData,
                size: 64,
                sha256: digest.clone(),
                virtual_address: None,
                executable: false,
            },
            InputRegion {
                region: InputRegionName::VmInstructions,
                size: 0x100,
                sha256: digest.clone(),
                virtual_address: Some(0x1000),
                executable: true,
            },
            InputRegion {
                region: InputRegionName::IsolateInstructions,
                size: 0x100,
                sha256: digest,
                virtual_address: Some(0x2000),
                executable: true,
            },
        ];
        let model = ProgramModel {
            model_version: MODEL_VERSION,
            producer: producer.clone(),
            input: ObservedInput {
                identity: identity.clone(),
                regions: regions.clone(),
            },
            compatibility: Some(compatibility.clone()),
            capabilities,
            libraries,
            classes,
            functions,
            object_pool,
            diagnostics,
            extensions: Default::default(),
        };
        let host = HostSelectedContext {
            identity,
            producer,
            compatibility: Some(compatibility),
            regions,
        };
        (model, host)
    }

    fn manifest_signals() -> AndroidManifestSignals {
        AndroidManifestSignals {
            package_name: Some("com.example.app".to_string()),
            application_name: Some("com.example.app.App".to_string()),
            has_main_launcher: true,
            has_view_browsable: true,
            activities: vec!["com.example.app.MainActivity".to_string()],
            launcher_activities: vec!["com.example.app.MainActivity".to_string()],
            deeplink_activities: vec!["com.example.app.MainActivity".to_string()],
            deeplink_entries: vec!["myapp://open".to_string()],
        }
    }

    fn startup_evidence() -> AndroidStartupEvidence {
        AndroidStartupEvidence {
            present: true,
            confidence: "high".to_string(),
            dex_files: vec!["classes.dex".to_string()],
            dart_entrypoints: vec![DartEntrypointEvidence {
                source_dex: "classes.dex".to_string(),
                class_descriptor: "Lcom/example/MainActivity;".to_string(),
                class_name: "com.example.MainActivity".to_string(),
                method_name: "configureFlutterEngine".to_string(),
                target_method: "executeDartEntrypoint".to_string(),
                function_name: Some("main".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
                initial_route: None,
                app_bundle_path: Some("flutter_assets".to_string()),
                confidence: "high".to_string(),
            }],
            ..AndroidStartupEvidence::default()
        }
    }

    /// Run every enrichment pass and prove the model came out the other side
    /// byte-identical and still valid.
    ///
    /// This is the invariant that matters: no synthetic pool entry, no invented
    /// class or function, no index collision, no capability contradiction, and
    /// no authority upgrade. Checking it by re-validating rather than by
    /// eyeballing fields means a future pass that writes into the model fails
    /// here even if nobody thought to assert on the field it touched.
    fn assert_enrichment_preserves(model: &ProgramModel, host: &HostSelectedContext) {
        validate(model, host).expect("fixture must be valid before enrichment");
        let before = model.to_canonical_json();

        let mut hints = ProgramHints::new();
        collect_model_name_hints(model, &mut hints);
        collect_manifest_bootflow_hints(model, &manifest_signals(), &mut hints);
        collect_apk_startup_bootflow_hints(model, &startup_evidence(), &mut hints);

        assert_eq!(
            before,
            model.to_canonical_json(),
            "enrichment mutated the model"
        );
        validate(model, host).expect("model must still be valid after enrichment");

        // Whatever the passes produced, none of it can claim to be exact.
        assert!(hints
            .iter()
            .all(|h| h.provenance != HintProvenance::Derived
                || h.origin != HintOrigin::ModelNamePattern));
        for hint in hints.iter() {
            if let Some(va) = hint.target_va {
                assert!(
                    model.functions.iter().any(|f| f.code.start_va == va),
                    "a hint points at {va:#x}, which is not a recovered code range"
                );
            }
        }
    }

    #[test]
    fn enrichment_preserves_an_authoritative_model() {
        let (model, host) = validatable_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "MainActivity", Some(0))],
            vec![
                fun(0, named("main"), Some(0), 0x2000, 0x10),
                Function {
                    id: FunctionId(1),
                    name: named("onNewIntent"),
                    owner: Some(ClassId(0)),
                    code: CodeRange {
                        start_va: 0x2010,
                        size: 0x10,
                    },
                    // The section base is the region's, not the function's.
                    code_section_va: 0x2000,
                    provenance: Provenance::Exact,
                },
            ],
            ObjectPool {
                index_space: PoolIndexSpace::Hardware,
                geometry: Some(ARM64_POOL_GEOMETRY),
                entries: vec![pool_selector(3, "build", 0x2000)],
            },
            Capabilities {
                libraries: CapabilityLevel::Complete,
                classes: CapabilityLevel::Complete,
                class_relationships: CapabilityLevel::Unavailable,
                functions: CapabilityLevel::Complete,
                function_names: CapabilityLevel::Complete,
                object_pool: CapabilityLevel::Complete,
                pool_index_space: CapabilityLevel::Complete,
            },
            vec![Diagnostic::unavailable(
                Domain::ClassRelationships,
                "no superclass edges in this snapshot",
            )],
        );
        assert_enrichment_preserves(&model, &host);
    }

    #[test]
    fn enrichment_preserves_a_model_that_recovered_nothing() {
        let (model, host) = validatable_model(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ObjectPool::unavailable(),
            Capabilities::all_unavailable(),
            Domain::ALL
                .iter()
                .map(|domain| Diagnostic::unavailable(*domain, "no parser for this identity"))
                .collect(),
        );
        assert_enrichment_preserves(&model, &host);
    }

    #[test]
    fn enrichment_preserves_a_heuristic_only_model() {
        let (model, host) = validatable_model(
            vec![Library {
                id: LibraryId(0),
                uri: "package:app/main.dart".to_string(),
                display_name: None,
                provenance: Provenance::Heuristic,
            }],
            Vec::new(),
            vec![Function {
                id: FunctionId(0),
                name: None,
                owner: None,
                code: CodeRange {
                    start_va: 0x2000,
                    size: 0x10,
                },
                code_section_va: 0x2000,
                provenance: Provenance::Heuristic,
            }],
            ObjectPool {
                index_space: PoolIndexSpace::Ordinal,
                geometry: None,
                entries: vec![PoolEntry {
                    index: 0,
                    kind: PoolEntryKind::String,
                    value: Some("a carved string".to_string()),
                    target_va: None,
                    provenance: Provenance::Heuristic,
                    confidence: None,
                }],
            },
            Capabilities {
                libraries: CapabilityLevel::Partial,
                classes: CapabilityLevel::Unavailable,
                class_relationships: CapabilityLevel::Unavailable,
                functions: CapabilityLevel::Partial,
                function_names: CapabilityLevel::Unavailable,
                object_pool: CapabilityLevel::Partial,
                pool_index_space: CapabilityLevel::Unavailable,
            },
            vec![
                Diagnostic::unavailable(Domain::Classes, "no class table"),
                Diagnostic::unavailable(Domain::ClassRelationships, "no class table"),
                Diagnostic::unavailable(Domain::FunctionNames, "no names in instruction bytes"),
                Diagnostic::unavailable(Domain::PoolIndexSpace, "carve order is not an index space"),
            ],
        );
        assert_enrichment_preserves(&model, &host);
    }

    /// Manifest enrichment produces hints and leaves the model alone. In v3 this
    /// pushed synthetic `ObjectPoolEntry` records at `index = object_pool.len()`,
    /// which both invented pool slots and grew the pool index space.
    #[test]
    fn manifest_enrichment_produces_hints_and_leaves_the_model_untouched() {
        let model = test_model(
            vec![lib(1, "package:app/main.dart")],
            vec![
                cls(1, "AppRoot", Some(1)),
                cls(2, "MainActivity", Some(1)),
                cls(3, "SettingsMapper", None),
            ],
            vec![
                fun(1, named("main"), Some(1), 0x1000, 4),
                fun(2, named("runApp"), Some(1), 0x1004, 4),
                fun(3, named("onNewIntent"), Some(2), 0x1008, 4),
                fun(4, named("onResume"), Some(2), 0x100c, 4),
                fun(5, named("ensureInitialized"), Some(3), 0x1010, 4),
            ],
            ordinal_pool(vec![pool_string(0, "an adapter-authored value")]),
        );
        let before = model.clone();
        let signals = AndroidManifestSignals {
            package_name: Some("com.example.app".to_string()),
            application_name: Some("com.example.app.App".to_string()),
            has_main_launcher: true,
            has_view_browsable: true,
            activities: vec!["com.example.app.MainActivity".to_string()],
            launcher_activities: vec!["com.example.app.MainActivity".to_string()],
            deeplink_activities: vec!["com.example.app.MainActivity".to_string()],
            deeplink_entries: vec!["myapp://open".to_string()],
        };

        let mut hints = ProgramHints::new();
        let inserted = collect_manifest_bootflow_hints(&model, &signals, &mut hints);
        assert!(inserted >= 4, "expected at least four hints, got {inserted}");

        // No new pool entry, no new class, no new function, no index collision.
        assert_eq!(model, before);
        assert_eq!(model.object_pool.entries.len(), 1);
        assert_eq!(model.object_pool.entries[0].index, 0);

        let kinds = hints.iter().map(|h| h.kind).collect::<Vec<_>>();
        assert!(kinds.contains(&HintKind::BootMain));
        assert!(kinds.contains(&HintKind::BootRunApp));
        assert!(kinds.contains(&HintKind::DeepLinkHandler));
        assert!(kinds.contains(&HintKind::ActivityHandler));
        // `SettingsMapper.ensureInitialized` is not in a bootstrap context, so
        // the selector shape alone does not license the hint.
        assert!(!kinds.contains(&HintKind::BootstrapInit));
        assert!(hints
            .iter()
            .all(|h| h.origin == HintOrigin::AndroidManifest));
        assert!(hints
            .iter()
            .all(|h| h.provenance == HintProvenance::Derived));
    }

    #[test]
    fn decompile_engine_profile_light_is_minimal() {
        let cfg = DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Light);
        assert!(!cfg.canonical_model_symbols);
        assert!(!cfg.pool_value_hints);
        assert!(!cfg.pool_semantic_hints);
        assert!(!cfg.semantic_reporting);
        assert!(!cfg.bootflow_category_seeds);
        assert!(!cfg.apk_startup_analysis);
    }

    #[test]
    fn decompile_engine_overrides_can_disable_balanced_defaults() {
        let base = DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Balanced);
        assert!(base.bootflow_category_seeds);
        assert!(base.apk_startup_analysis);
        let overrides = DecompileEngineOptionOverrides {
            canonical_model_symbols: Some(false),
            pool_value_hints: None,
            pool_semantic_hints: Some(false),
            semantic_reporting: Some(false),
            bootflow_category_seeds: Some(false),
            apk_startup_analysis: Some(false),
        };
        let cfg = base.with_overrides(&overrides);
        assert!(!cfg.canonical_model_symbols);
        assert!(cfg.pool_value_hints);
        assert!(!cfg.pool_semantic_hints);
        assert!(!cfg.semantic_reporting);
        assert!(!cfg.bootflow_category_seeds);
        assert!(!cfg.apk_startup_analysis);
    }

    /// Pool semantic metadata now comes from the function an entry points at,
    /// plus host hints. v3 read `owner_class`/`library_uri` off the pool entry
    /// itself, which let a producer attach any class to any slot.
    #[test]
    fn builds_pool_semantic_hints_from_the_function_an_entry_points_at() {
        let model = test_model(
            vec![lib(0, "package:flutter/src/widgets/binding.dart")],
            vec![cls(0, "WidgetsBindingObserver", Some(0))],
            vec![fun(0, named("didChangeMetrics"), Some(0), 0x1234, 4)],
            hardware_pool(vec![
                pool_selector(7, "didChangeMetrics", 0x1234),
                pool_string(8, "42"),
            ]),
        );
        let hints = program_hints(vec![]);

        let semantic = build_pool_semantic_hints(&model, &hints);
        let h = semantic.get(&7).expect("missing semantic hint entry");
        assert_eq!(h.selector.as_deref(), Some("didChangeMetrics"));
        assert_eq!(h.owner_class.as_deref(), Some("WidgetsBindingObserver"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/binding.dart")
        );
        assert_eq!(h.target_va, Some(0x1234));
    }

    /// An ordinal pool index is a position in the producer's list. Joining
    /// disassembly against it would attach unrelated strings, so the join is
    /// refused outright rather than filtered.
    #[test]
    fn ordinal_pool_indexes_produce_no_hints_at_all() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![fun(0, named("didChangeMetrics"), Some(0), 0x1234, 4)],
            ordinal_pool(vec![pool_selector(7, "didChangeMetrics", 0x1234)]),
        );
        let hints = program_hints(vec![]);

        assert!(build_pool_semantic_hints(&model, &hints).is_empty());
        assert!(build_pool_value_hints(&model).is_empty());
        assert!(!collect_pool_metadata_stats(&model).addressable);
    }

    #[test]
    fn collects_pool_metadata_coverage_stats() {
        let model = test_model(
            vec![],
            vec![],
            vec![],
            hardware_pool(vec![
                pool_selector(1, "setState", 0x1000),
                pool_string(2, "42"),
            ]),
        );

        let stats = collect_pool_metadata_stats(&model);
        assert_eq!(stats.total_entries, 2);
        assert!(stats.addressable);
        assert_eq!(stats.with_target_va, 1);
        assert_eq!(stats.with_selector, 1);
        assert_eq!(stats.with_value, 2);
        assert_eq!(stats.heuristic, 0);
    }

    #[test]
    fn builds_pool_target_symbols_from_metadata() {
        let model = test_model(
            vec![
                lib(0, "package:flutter/src/widgets/binding.dart"),
                lib(1, "dart:typed_data"),
                lib(2, "package:spotube/services/connect.dart"),
            ],
            vec![
                cls(0, "WidgetsBindingObserver", Some(0)),
                cls(1, "Int64List", Some(1)),
                cls(2, "ConnectService", Some(2)),
            ],
            vec![
                fun(0, named("didChangeMetrics"), Some(0), 0x1234, 4),
                fun(1, named("Int64List"), Some(1), 0x2234, 4),
                fun(2, named("executeCommandAsync"), Some(2), 0x3234, 4),
            ],
            hardware_pool(vec![
                pool_selector(7, "didChangeMetrics", 0x1234),
                pool_selector(8, "Int64List", 0x2234),
                pool_selector(9, "executeCommandAsync", 0x3234),
            ]),
        );
        let hints = program_hints(vec![]);

        let semantic = build_pool_semantic_hints(&model, &hints);
        let values = build_pool_value_hints(&model);
        let map = build_pool_target_symbols(&semantic, &values);
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

    /// A host hint fills in a selector the model never had, and only that: it
    /// cannot displace an owner or library the model did recover.
    #[test]
    fn host_hints_fill_gaps_without_overriding_model_facts() {
        let model = test_model(
            vec![lib(0, "package:flutter/src/widgets/framework.dart")],
            vec![cls(1, "State", Some(0))],
            vec![fun(11, named("setState"), Some(1), 0x4000, 4)],
            hardware_pool(vec![pool_string(21, "opaque")]),
        );
        let hints = program_hints(vec![hint(
            HintKind::ActivityHandler,
            HintOrigin::AndroidManifest,
            "onNewIntent",
            Some(0x4000),
            Some("ManifestActivity"),
            Some("apk:classes.dex"),
        )]);

        // Nothing points at 0x4000 from the pool, so the entry has no target and
        // no fallback: the hint has nothing to attach to and does not invent one.
        assert!(build_pool_semantic_hints(&model, &hints).is_empty());

        let anchored = test_model(
            vec![lib(0, "package:flutter/src/widgets/framework.dart")],
            vec![cls(1, "State", Some(0))],
            vec![fun(11, named("setState"), Some(1), 0x4000, 4)],
            hardware_pool(vec![pool_selector(21, "opaque", 0x4000)]),
        );
        let semantic = build_pool_semantic_hints(&anchored, &hints);
        let h = semantic.get(&21).expect("missing enriched semantic hint");
        assert_eq!(h.selector.as_deref(), Some("opaque"));
        // Owner and library come from the model's own function, not the hint.
        assert_eq!(h.owner_class.as_deref(), Some("State"));
        assert_eq!(
            h.library_uri.as_deref(),
            Some("package:flutter/src/widgets/framework.dart")
        );
        assert_eq!(h.target_va, Some(0x4000));
    }

    #[test]
    fn target_filter_matches_function_id_without_scope_override() {
        let full_model = test_model(vec![], vec![cls(1, "AppRoot", None)], vec![fun(42, Some(Name { text: "main".to_string(), provenance: Provenance::Heuristic, confidence: None }), Some(1), 0x1000, 4)], ordinal_pool(vec![]));
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
        assert_eq!(selected.functions[0].id, FunctionId(42));
    }

    #[test]
    fn target_filter_can_override_scope_for_entry_va() {
        let full_model = test_model(
            vec![lib(0, "package:app/main.dart"), lib(1, "dart:core")],
            vec![cls(1, "AppClass", Some(0)), cls(2, "CoreClass", Some(1))],
            vec![
                fun(1, named("main"), Some(1), 0x1000, 4),
                fun(2, named("coreFn"), Some(2), 0x2000, 4),
            ],
            ordinal_pool(Vec::new()),
        );
        let (scoped_model, _) = apply_function_scope_filter(&full_model, FunctionScope::App, &[]);
        assert_eq!(scoped_model.functions.len(), 1);

        let (selected, stats) =
            apply_target_function_filter(&full_model, &scoped_model, FunctionTarget::EntryVa(0x2000))
                .expect("target filter");

        assert!(stats.enabled);
        assert!(stats.scope_overridden);
        assert_eq!(stats.matched_count, 1);
        assert_eq!(selected.functions.len(), 1);
        assert_eq!(selected.functions[0].code.start_va, 0x2000);
    }

    #[test]
    fn target_filter_rejects_ambiguous_any_selector() {
        let full_model = test_model(vec![], vec![cls(1, "AppRoot", None)], vec![fun(42, Some(Name { text: "fnA".to_string(), provenance: Provenance::Heuristic, confidence: None }), Some(1), 0x1000, 4), fun(7, Some(Name { text: "fnB".to_string(), provenance: Provenance::Heuristic, confidence: None }), Some(1), 42, 4)], ordinal_pool(vec![]));
        let scoped_model = full_model.clone();

        let err = apply_target_function_filter(&full_model, &scoped_model, FunctionTarget::Any(42))
            .expect_err("ambiguous");
        assert!(err.to_string().contains("ambiguous"));
    }

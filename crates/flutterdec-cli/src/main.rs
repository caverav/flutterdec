use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use flutterdec_adapter::store::{self, EntryState};
use flutterdec_core::{
    available_adapters, run_decompile, run_diff, run_engine_fingerprint, run_info, run_symbol_map,
    AdapterBackend, DecompileAnalysisProfile, DecompileEngineOptionOverrides,
    DecompileEngineOptions, DecompileOptions, DiffOptions, EngineFingerprintOptions, FunctionScope,
    FunctionTarget, SymbolMapOptions,
};
use flutterdec_loader::layout::Layout;
use flutterdec_loader::registry::CompatibilityRegistry;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "flutterdec")]
#[command(version)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
#[command(about = "Static Flutter AOT decompiler research CLI")]
#[command(long_about = "\
Static analysis and decompilation of Flutter AOT Android ARM64 binaries.

Accepts an APK or a bare libapp.so. All analysis is static; the target is
never executed.

Typical flow:
  1. flutterdec info <APK>                    inspect the target
  2. flutterdec adapter install --dart-hash <HASH>  install the matching adapter
  3. flutterdec decompile <APK> -o <DIR>      recover pseudocode")]
#[command(after_help = "\
Examples:
  flutterdec info ./sample.apk --json
  flutterdec decompile ./sample.apk -o ./out
  flutterdec decompile ./sample.apk -o ./out --emit-asm --emit-ir
  flutterdec diff --old ./old.apk --new ./new.apk -o ./out-diff --json

Full reference: https://github.com/caverav/flutterdec/blob/main/docs/cli-reference.md")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Inspect a target and report snapshot, arch, and adapter status
    Info(InfoCmd),
    /// Recover Dart pseudocode from a Flutter AOT snapshot
    Decompile(DecompileCmd),
    /// Compare two builds at recovered-function level
    Diff(DiffCmd),
    /// Identify the Flutter engine build from an ELF
    EngineFingerprint(EngineFingerprintCmd),
    /// Derive engine symbol names from a stripped/unstripped pair
    MapSymbols(MapSymbolsCmd),
    /// Manage Dart snapshot adapters
    Adapter(AdapterCmd),
}

#[derive(Args, Debug)]
struct InfoCmd {
    /// APK or libapp.so to inspect
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Print the full report as JSON instead of plain text
    #[arg(long)]
    json: bool,
    /// Which snapshot adapter backend to use
    #[arg(long, value_enum, default_value_t = AdapterBackendArg::Auto, value_name = "BACKEND")]
    adapter_backend: AdapterBackendArg,
}

#[derive(Args, Debug)]
struct DecompileCmd {
    /// APK or libapp.so to decompile
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Directory to write pseudocode, reports, and artifacts into
    #[arg(short = 'o', long = "out", value_name = "DIR")]
    out_dir: PathBuf,

    /// Write per-function ARM64 disassembly to asm/*.s
    #[arg(long, help_heading = "Emitted artifacts")]
    emit_asm: bool,
    /// Prefix asm lines with raw 32-bit opcode words (requires --emit-asm)
    #[arg(long, help_heading = "Emitted artifacts")]
    emit_asm_opcodes: bool,
    /// Write ghidra_apply_symbols.py to apply recovered symbols in Ghidra
    #[arg(long, help_heading = "Emitted artifacts")]
    emit_ghidra_script: bool,
    /// Write ida_apply_symbols.py to apply recovered symbols in IDA
    #[arg(long, help_heading = "Emitted artifacts")]
    emit_ida_script: bool,
    /// Write the intermediate representation to ir/*.json
    #[arg(long, help_heading = "Emitted artifacts")]
    emit_ir: bool,
    /// Split a function record that spans more than one real function.
    ///
    /// The adapter sizes a record as the gap to the next start it recovered, so a
    /// function it missed is swallowed by its predecessor and never emitted. On two
    /// release samples that hides roughly three quarters of the decoded blocks.
    /// Off by default: it multiplies the emitted function count, which moves every
    /// absolute quality counter and makes `--max-functions` and `--function-scope`
    /// apply to records rather than to what is emitted.
    #[arg(long)]
    split_records: bool,

    /// Unstripped engine ELF to harvest symbol names from (repeatable)
    #[arg(
        long = "extra-symbol-elf",
        value_name = "PATH",
        help_heading = "Symbol ingestion"
    )]
    extra_symbol_elfs: Vec<PathBuf>,
    /// Target summary produced by `map-symbols` to ingest (repeatable)
    #[arg(
        long = "extra-symbol-map-target",
        alias = "extra-symbol-map-targets",
        value_name = "PATH",
        help_heading = "Symbol ingestion"
    )]
    extra_symbol_map_targets: Vec<PathBuf>,
    /// Fall back to the nearest preceding symbol when no exact match exists
    #[arg(long, help_heading = "Symbol ingestion")]
    include_nearest_symbol_map: bool,

    /// Only process functions whose name contains this substring
    #[arg(long, value_name = "SUBSTRING", help_heading = "Function selection")]
    focus: Option<String>,
    /// Select one function: id:<N>, va:0x<ADDR>, 0x<ADDR>, or <N>
    #[arg(
        long,
        value_name = "SELECTOR",
        help_heading = "Function selection",
        long_help = "\
Restrict output to a single function.

Accepted selectors:
  id:<N>        function id, e.g. id:42
  va:0x<ADDR>   entry virtual address, e.g. va:0x613468
  0x<ADDR>      bare hex address
  <N>           bare number; fails if it matches both an id and an address

If the match falls outside the current --function-scope filter, target mode
overrides the scope to keep the explicit match. Selection diagnostics are
written to report.json under target_selection."
    )]
    target: Option<String>,
    /// Stop after this many functions
    #[arg(long, value_name = "N", help_heading = "Function selection")]
    max_functions: Option<usize>,
    /// Which functions to include
    #[arg(
        long,
        value_enum,
        default_value_t = FunctionScopeArg::AppUnknown,
        value_name = "SCOPE",
        help_heading = "Function selection",
        long_help = "\
Which functions to include.

Scope filters apply to every emitted artifact. Use --app-package to narrow
further within a scope."
    )]
    function_scope: FunctionScopeArg,
    /// Restrict to this Dart package, as in package:<NAME>/... (repeatable)
    #[arg(
        long = "app-package",
        value_name = "NAME",
        help_heading = "Function selection"
    )]
    app_packages: Vec<String>,

    /// Which snapshot adapter backend to use
    #[arg(
        long,
        value_enum,
        default_value_t = AdapterBackendArg::Auto,
        value_name = "BACKEND",
        help_heading = "Analysis engine",
        long_help = "\
Which snapshot adapter backend to use.

Backends are located through the environment: FLUTTERDEC_R2FLUTTER_BIN or
FLUTTERDEC_R2FLUTTER_CMD for r2flutter, and FLUTTERDEC_BLUTTER_CMD or
FLUTTERDEC_BLUTTER_PY for the Blutter bridge."
    )]
    adapter_backend: AdapterBackendArg,
    /// Fail if the adapter and loader disagree on the snapshot hash
    #[arg(long, help_heading = "Analysis engine")]
    require_snapshot_hash_match: bool,
    /// Analysis depth versus throughput
    #[arg(
        long,
        value_enum,
        default_value_t = AnalysisProfileArg::Balanced,
        value_name = "PROFILE",
        help_heading = "Analysis engine",
        long_help = "\
Analysis depth versus throughput.

Individual passes can be forced on or off with the --with-*/--no-* flags,
which override whichever profile is selected."
    )]
    analysis_profile: AnalysisProfileArg,

    /// Fail if more than N placeholder `if` statements are emitted
    #[arg(
        long,
        default_value_t = 0,
        value_name = "N",
        help_heading = "Quality gates"
    )]
    max_placeholder_ifs: usize,
    /// Fail if more than N control-flow edges stay unresolved
    #[arg(
        long,
        default_value_t = 0,
        value_name = "N",
        help_heading = "Quality gates"
    )]
    max_unresolved_cf: usize,
    /// Fail if the indirect-call ratio exceeds this fraction
    #[arg(
        long,
        default_value_t = 0.30,
        value_name = "RATIO",
        help_heading = "Quality gates"
    )]
    max_indirect_call_ratio: f64,
    /// Fail if the successfully disassembled fraction falls below this
    #[arg(
        long,
        default_value_t = 0.80,
        value_name = "RATIO",
        help_heading = "Quality gates"
    )]
    min_disassembly_ratio: f64,

    /// Force canonical model symbol naming on
    #[arg(
        long,
        conflicts_with = "no_canonical_model_symbols",
        help_heading = "Analysis passes"
    )]
    with_canonical_model_symbols: bool,
    /// Force canonical model symbol naming off
    #[arg(long, help_heading = "Analysis passes")]
    no_canonical_model_symbols: bool,
    /// Force object-pool value hints on
    #[arg(
        long,
        conflicts_with = "no_pool_value_hints",
        help_heading = "Analysis passes"
    )]
    with_pool_value_hints: bool,
    /// Force object-pool value hints off
    #[arg(long, help_heading = "Analysis passes")]
    no_pool_value_hints: bool,
    /// Force object-pool semantic hints on
    #[arg(
        long,
        conflicts_with = "no_pool_semantic_hints",
        help_heading = "Analysis passes"
    )]
    with_pool_semantic_hints: bool,
    /// Force object-pool semantic hints off
    #[arg(long, help_heading = "Analysis passes")]
    no_pool_semantic_hints: bool,
    /// Force semantic reporting on
    #[arg(
        long,
        conflicts_with = "no_semantic_reporting",
        help_heading = "Analysis passes"
    )]
    with_semantic_reporting: bool,
    /// Force semantic reporting off
    #[arg(long, help_heading = "Analysis passes")]
    no_semantic_reporting: bool,
    /// Force boot-flow category seeding on
    #[arg(
        long,
        conflicts_with = "no_bootflow_category_seeds",
        help_heading = "Analysis passes"
    )]
    with_bootflow_category_seeds: bool,
    /// Force boot-flow category seeding off
    #[arg(long, help_heading = "Analysis passes")]
    no_bootflow_category_seeds: bool,
    /// Force APK startup analysis on
    #[arg(
        long,
        conflicts_with = "no_apk_startup_analysis",
        help_heading = "Analysis passes"
    )]
    with_apk_startup_analysis: bool,
    /// Force APK startup analysis off
    #[arg(long, help_heading = "Analysis passes")]
    no_apk_startup_analysis: bool,
}

#[derive(Args, Debug)]
struct DiffCmd {
    /// Baseline APK or libapp.so
    #[arg(long = "old", value_name = "INPUT")]
    old_input: PathBuf,
    /// Candidate APK or libapp.so to compare against the baseline
    #[arg(long = "new", value_name = "INPUT")]
    new_input: PathBuf,
    /// Directory to write diff_report.json into
    #[arg(short = 'o', long = "out", value_name = "DIR")]
    out_dir: PathBuf,
    /// Which functions to compare [app-unknown, app, all]
    #[arg(long, value_enum, default_value_t = FunctionScopeArg::AppUnknown, value_name = "SCOPE")]
    function_scope: FunctionScopeArg,
    /// Restrict the compare set to this Dart package (repeatable)
    #[arg(long = "app-package", value_name = "NAME")]
    app_packages: Vec<String>,
    /// Which snapshot adapter backend to use
    #[arg(long, value_enum, default_value_t = AdapterBackendArg::Auto, value_name = "BACKEND")]
    adapter_backend: AdapterBackendArg,
    /// Fail if either side has an adapter/loader snapshot hash mismatch
    #[arg(long)]
    require_snapshot_hash_match: bool,
    /// Print the diff summary as JSON on stdout
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AnalysisProfileArg {
    /// Reduced analysis, for faster large-scale runs
    Light,
    /// Best readability and semantic recovery
    Balanced,
}

impl AnalysisProfileArg {
    fn to_core(self) -> DecompileAnalysisProfile {
        match self {
            Self::Light => DecompileAnalysisProfile::Light,
            Self::Balanced => DecompileAnalysisProfile::Balanced,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FunctionScopeArg {
    /// App (package:*) plus functions of unknown ownership
    #[value(name = "app-unknown")]
    AppUnknown,
    /// Only app (package:*) functions
    #[value(name = "app")]
    App,
    /// Also include Flutter, Dart runtime, and framework internals
    #[value(name = "all")]
    All,
}

impl FunctionScopeArg {
    fn to_core(self) -> FunctionScope {
        match self {
            Self::AppUnknown => FunctionScope::AppUnknown,
            Self::App => FunctionScope::App,
            Self::All => FunctionScope::All,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AdapterBackendArg {
    /// Try r2flutter, then the Blutter bridge, then the internal adapter
    Auto,
    /// Force the internal adapter
    Internal,
    /// Require the Blutter bridge, with no fallback
    Blutter,
    // Clap's derive spells this value `r2-flutter`; the alias accepts the tool's own
    // spelling, which is what the docs, `report.json` and the env vars all use.
    /// Require the r2flutter backend, with no fallback
    #[value(alias = "r2flutter")]
    R2Flutter,
}

impl AdapterBackendArg {
    fn to_core(self) -> AdapterBackend {
        match self {
            Self::Auto => AdapterBackend::Auto,
            Self::Internal => AdapterBackend::Internal,
            Self::Blutter => AdapterBackend::Blutter,
            Self::R2Flutter => AdapterBackend::R2Flutter,
        }
    }
}

#[derive(Args, Debug)]
struct EngineFingerprintCmd {
    /// Engine ELF to fingerprint, usually libflutter.so
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Directory to write the fingerprint report into
    #[arg(short = 'o', long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// Maximum number of version markers to report
    #[arg(long, default_value_t = 24, value_name = "N")]
    max_markers: usize,
    /// Print the fingerprint as JSON on stdout
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct MapSymbolsCmd {
    /// Stripped engine ELF, matching the one shipped in the APK
    #[arg(long = "stripped", value_name = "PATH")]
    stripped_path: PathBuf,
    /// Unstripped build of the same engine, carrying the symbol names
    #[arg(long = "unstripped", value_name = "PATH")]
    unstripped_path: PathBuf,
    /// Directory to write the symbol map into
    #[arg(short = 'o', long = "out", value_name = "DIR")]
    out_dir: PathBuf,
    /// Also map branch targets, not just call targets
    #[arg(long)]
    include_branches: bool,
    /// Maximum byte distance when falling back to the nearest symbol
    #[arg(long, default_value_t = 8192, value_name = "BYTES")]
    nearest_max_distance: u64,
    /// Require executable-section layout to match between the two ELFs
    #[arg(long)]
    require_exec_match: bool,
    /// Register the result in symbols/manifest.json for later auto-ingestion
    #[arg(long = "register-local-cache")]
    register_local_cache: bool,
    /// Print the mapping summary as JSON on stdout
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum AdapterSubcommand {
    /// Install the adapter a compatibility record authorizes for a snapshot hash
    Install(AdapterInstallCmd),
    /// Report the verified state of every authorized adapter
    List(AdapterListCmd),
}

#[derive(Args, Debug)]
struct AdapterInstallCmd {
    /// Dart snapshot hash, as reported by `flutterdec info`
    #[arg(long = "dart-hash", value_name = "HASH")]
    dart_hash: String,
    /// Target architecture, when one hash has records for more than one
    #[arg(long = "target-arch", value_name = "ARCH")]
    target_arch: Option<String>,
    /// Artifact to publish instead of the packaged producer. Must match the
    /// digest and size the compatibility record declares.
    #[arg(long = "from", value_name = "PATH")]
    from: Option<PathBuf>,
    /// Print the installation record as JSON on stdout
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AdapterListCmd {
    /// Print the store report as JSON on stdout
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AdapterCmd {
    #[command(subcommand)]
    subcommand: AdapterSubcommand,
}

/// Exit status for a store whose content is broken rather than merely absent.
const STORE_STATE_FAILURE: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    // Resolved once, from the executable and the environment. Nothing below
    // this line may consult the current directory for a repository root: that
    // is what made a packaged binary behave differently per working directory.
    let layout = Layout::resolve().context("resolve flutterdec data and store locations")?;

    match cli.command {
        Command::Info(cmd) => handle_info(&layout, cmd)?,
        Command::Decompile(cmd) => handle_decompile(&layout, cmd)?,
        Command::Diff(cmd) => handle_diff(&layout, cmd)?,
        Command::EngineFingerprint(cmd) => handle_engine_fingerprint(cmd)?,
        Command::MapSymbols(cmd) => handle_map_symbols(&layout, cmd)?,
        Command::Adapter(cmd) => return handle_adapter(&layout, cmd),
    }

    Ok(ExitCode::SUCCESS)
}

fn handle_info(layout: &Layout, cmd: InfoCmd) -> Result<()> {
    let out = run_info(layout, &cmd.input, cmd.adapter_backend.to_core())?;
    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("input: {}", out.input_path);
        println!("libapp: {}", out.libapp_path);
        println!("arch: {}", out.arch);
        println!("snapshot hash: {}", out.snapshot_hash);
        if let Some(aliases) = out
            .dart_aliases
            .as_ref()
            .filter(|aliases| !aliases.is_empty())
        {
            println!("dart aliases: {}", serde_json::to_string(aliases)?);
        }
        if let Some(tag_style) = out.dart_tag_style.as_deref() {
            println!("dart tag style: {}", tag_style);
        }
        if let Some(compressed) = out.compressed_pointers {
            println!("compressed pointers: {}", compressed);
        }
        println!("adapter installed: {}", out.adapter_installed);
        // Requested, resolved, and fallback are printed separately because they
        // are separate facts. Collapsing them into one "adapter kind" line is
        // what made a filename look like a decision.
        if let Some(requested) = out.requested_backend.as_deref() {
            println!("requested backend: {}", requested);
        }
        if let Some(resolved) = out.resolved_backend.as_deref() {
            println!("resolved backend: {}", resolved);
        }
        if let Some(reason) = out.backend_fallback_reason.as_deref() {
            println!("backend fallback reason: {}", reason);
        }
        if let Some(id) = out.producer_id.as_deref() {
            println!("producer: {}", id);
        }
        if let Some(trust) = out.producer_trust.as_deref() {
            println!("producer trust: {}", trust);
        }
        if let Some(digest) = out.compatibility_record_sha256.as_deref() {
            println!("compatibility record: {}", digest);
        }
        if let Some(present) = out.registry_record_present {
            println!("registry record present: {}", present);
        }
        if let Some(exact) = out.snapshot_identity_is_exact {
            println!("snapshot identity header-derived: {}", exact);
        }
        if let Some(rejection) = out.identity_rejection.as_deref() {
            println!("adapter selection refused: {}", rejection);
        }
        if let Some(capabilities) = out.model_capabilities.as_ref() {
            println!("model capabilities:");
            for (domain, level) in capabilities {
                println!("  {}: {}", domain, level);
            }
        }
        if let Some(warnings) = out.compatibility_warnings.as_ref() {
            if !warnings.is_empty() {
                println!("compatibility warnings:");
                for warning in warnings {
                    println!("  - {}", warning);
                }
            }
        }
        if let Some(n) = out.function_count {
            println!("functions: {}", n);
        }
        if let Some(present) = out.android_startup_present {
            println!("android startup present: {}", present);
        }
        if let Some(confidence) = out.android_startup_confidence.as_deref() {
            println!("android startup confidence: {}", confidence);
        }
        if let Some(count) = out.android_startup_entrypoint_count {
            println!("android startup entrypoints: {}", count);
        }
        if let Some(count) = out.android_startup_flutter_activity_count {
            println!("android startup flutter activities: {}", count);
        }
        if let Some(total) = out.app_package_count_total {
            println!("app packages: {}", total);
            if let Some(top) = out.app_package_counts_top.as_ref() {
                for item in top.iter().take(8) {
                    println!("  - {} ({})", item.package, item.functions);
                }
            }
        }
    }
    Ok(())
}

fn handle_decompile(layout: &Layout, cmd: DecompileCmd) -> Result<()> {
    let input = cmd.input.clone();
    let opt = build_decompile_options(cmd)?;
    let quality = run_decompile(layout, &input, &opt)?;
    println!("{}", serde_json::to_string_pretty(&quality)?);
    Ok(())
}

fn handle_diff(layout: &Layout, cmd: DiffCmd) -> Result<()> {
    let old_input = cmd.old_input.clone();
    let new_input = cmd.new_input.clone();
    let json = cmd.json;
    let opt = DiffOptions {
        out_dir: cmd.out_dir,
        adapter_backend: cmd.adapter_backend.to_core(),
        function_scope: cmd.function_scope.to_core(),
        app_packages: cmd.app_packages,
        require_snapshot_hash_match: cmd.require_snapshot_hash_match,
    };
    let report = run_diff(layout, &old_input, &new_input, &opt)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("old input: {}", report.old_input_path);
        println!("new input: {}", report.new_input_path);
        println!(
            "snapshot hash: old={} new={}",
            report.old_snapshot_hash, report.new_snapshot_hash
        );
        println!(
            "snapshot hash match: old={} new={} required={}",
            report.old_snapshot_hash_match,
            report.new_snapshot_hash_match,
            report.require_snapshot_hash_match
        );
        println!(
            "dart aliases: old={} new={}",
            serde_json::to_string(&report.old_dart_aliases)?,
            serde_json::to_string(&report.new_dart_aliases)?
        );
        println!(
            "functions: old={} new={} common={} added={} removed={}",
            report.old_function_count,
            report.new_function_count,
            report.common_function_count,
            report.added_function_count,
            report.removed_function_count
        );
        if !report.added_packages_top.is_empty() {
            println!("top added packages:");
            for item in report.added_packages_top.iter().take(5) {
                println!("  + {} ({})", item.package, item.functions);
            }
        }
        if !report.removed_packages_top.is_empty() {
            println!("top removed packages:");
            for item in report.removed_packages_top.iter().take(5) {
                println!("  - {} ({})", item.package, item.functions);
            }
        }
        println!("report: {}", report.report_path);
    }
    Ok(())
}

fn build_decompile_options(cmd: DecompileCmd) -> Result<DecompileOptions> {
    if cmd.emit_asm_opcodes && !cmd.emit_asm {
        bail!("--emit-asm-opcodes requires --emit-asm");
    }
    let function_target = cmd
        .target
        .as_deref()
        .map(parse_function_target)
        .transpose()?;
    let profile = cmd.analysis_profile.to_core();
    let overrides = resolve_decompile_overrides(&cmd)?;
    let engine_options = DecompileEngineOptions::for_profile(profile).with_overrides(&overrides);
    Ok(DecompileOptions {
        out_dir: cmd.out_dir,
        emit_asm: cmd.emit_asm,
        emit_asm_opcodes: cmd.emit_asm_opcodes,
        emit_ghidra_script: cmd.emit_ghidra_script,
        emit_ida_script: cmd.emit_ida_script,
        emit_ir: cmd.emit_ir,
        split_records: cmd.split_records,
        extra_symbol_elfs: cmd.extra_symbol_elfs,
        extra_symbol_map_targets: cmd.extra_symbol_map_targets,
        include_nearest_symbol_map: cmd.include_nearest_symbol_map,
        focus: cmd.focus,
        function_target,
        max_functions: cmd.max_functions,
        max_placeholder_ifs: cmd.max_placeholder_ifs,
        max_unresolved_cf: cmd.max_unresolved_cf,
        max_indirect_call_ratio: cmd.max_indirect_call_ratio,
        min_disassembly_ratio: cmd.min_disassembly_ratio,
        function_scope: cmd.function_scope.to_core(),
        app_packages: cmd.app_packages,
        adapter_backend: cmd.adapter_backend.to_core(),
        require_snapshot_hash_match: cmd.require_snapshot_hash_match,
        analysis_profile: profile,
        engine_options,
    })
}

fn parse_target_value(raw: &str) -> Result<u64> {
    let value = raw.trim();
    if value.is_empty() {
        bail!("target value cannot be empty");
    }
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let parsed = u64::from_str_radix(hex, 16)
            .with_context(|| format!("parse hex target value: {}", value))?;
        return Ok(parsed);
    }
    let parsed = value
        .parse::<u64>()
        .with_context(|| format!("parse target value: {}", value))?;
    Ok(parsed)
}

fn parse_function_target(raw: &str) -> Result<FunctionTarget> {
    let value = raw.trim();
    if let Some(id_value) = value.strip_prefix("id:") {
        return Ok(FunctionTarget::FunctionId(parse_target_value(id_value)?));
    }
    if let Some(va_value) = value.strip_prefix("va:") {
        return Ok(FunctionTarget::EntryVa(parse_target_value(va_value)?));
    }
    if value.starts_with("0x") || value.starts_with("0X") {
        return Ok(FunctionTarget::EntryVa(parse_target_value(value)?));
    }
    Ok(FunctionTarget::Any(parse_target_value(value)?))
}

fn resolve_decompile_overrides(cmd: &DecompileCmd) -> Result<DecompileEngineOptionOverrides> {
    let canonical_model_symbols = resolve_toggle(
        cmd.with_canonical_model_symbols,
        cmd.no_canonical_model_symbols,
        "--with-canonical-model-symbols/--no-canonical-model-symbols",
    )?;
    let pool_value_hints = resolve_toggle(
        cmd.with_pool_value_hints,
        cmd.no_pool_value_hints,
        "--with-pool-value-hints/--no-pool-value-hints",
    )?;
    let pool_semantic_hints = resolve_toggle(
        cmd.with_pool_semantic_hints,
        cmd.no_pool_semantic_hints,
        "--with-pool-semantic-hints/--no-pool-semantic-hints",
    )?;
    let semantic_reporting = resolve_toggle(
        cmd.with_semantic_reporting,
        cmd.no_semantic_reporting,
        "--with-semantic-reporting/--no-semantic-reporting",
    )?;
    let bootflow_category_seeds = resolve_toggle(
        cmd.with_bootflow_category_seeds,
        cmd.no_bootflow_category_seeds,
        "--with-bootflow-category-seeds/--no-bootflow-category-seeds",
    )?;
    let apk_startup_analysis = resolve_toggle(
        cmd.with_apk_startup_analysis,
        cmd.no_apk_startup_analysis,
        "--with-apk-startup-analysis/--no-apk-startup-analysis",
    )?;
    Ok(DecompileEngineOptionOverrides {
        canonical_model_symbols,
        pool_value_hints,
        pool_semantic_hints,
        semantic_reporting,
        bootflow_category_seeds,
        apk_startup_analysis,
    })
}

fn handle_engine_fingerprint(cmd: EngineFingerprintCmd) -> Result<()> {
    let opt = EngineFingerprintOptions {
        out_dir: cmd.out_dir,
        max_markers: cmd.max_markers,
    };
    let report = run_engine_fingerprint(&cmd.input, &opt)?;
    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("input: {}", report.input_path);
        println!("machine: {} ({})", report.machine, report.machine_id);
        println!("build id: {}", report.build_id.as_deref().unwrap_or("-"));
        println!(
            "candidates: flutter={} dart={}",
            report.candidate_flutter_version.as_deref().unwrap_or("-"),
            report.candidate_dart_version.as_deref().unwrap_or("-")
        );
        println!("confidence: {}", report.confidence);
        println!(
            "symbols: symtab={} dynsym={}",
            report.symbol_count, report.dyn_symbol_count
        );
        println!(
            "exec sections: count={} total_size=0x{:x}",
            report.exec_section_count, report.exec_section_total_size
        );
        if let Some(path) = report.report_path.as_deref() {
            println!("report: {}", path);
        }
    }
    Ok(())
}

fn handle_map_symbols(layout: &Layout, cmd: MapSymbolsCmd) -> Result<()> {
    let opt = SymbolMapOptions {
        out_dir: cmd.out_dir,
        include_branches: cmd.include_branches,
        nearest_max_distance: cmd.nearest_max_distance,
        require_exec_match: cmd.require_exec_match,
        local_cache_root: cmd
            .register_local_cache
            .then(|| layout.symbols_dir().to_path_buf()),
    };
    let report = run_symbol_map(&cmd.stripped_path, &cmd.unstripped_path, &opt)?;
    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("stripped: {}", report.stripped_path);
        println!("unstripped: {}", report.unstripped_path);
        println!("arch: {}", report.arch);
        println!(
            "exec match: layout={} bytes={}",
            report.exec_layout_match, report.exec_bytes_match
        );
        println!(
            "calls: total={} exact={} nearest={} unresolved={}",
            report.total_direct_calls,
            report.exact_symbol_hits,
            report.nearest_symbol_hits,
            report.unresolved_calls
        );
        println!("unique targets: {}", report.unique_call_targets);
        println!("report: {}", report.report_path);
        println!("targets: {}", report.targets_path);
        println!("callsites: {}", report.callsites_path);
        if let Some(path) = report.local_cache_manifest_path.as_deref() {
            println!("local cache manifest: {}", path);
        }
        if let Some(build_id) = report.local_cache_build_id.as_deref() {
            println!("local cache build id: {}", build_id);
        }
        if let Some(version) = report.local_cache_flutter_version.as_deref() {
            println!("local cache flutter version: {}", version);
        }
        if !report.local_cache_registered_paths.is_empty() {
            println!("local cache targets:");
            for path in &report.local_cache_registered_paths {
                println!("  - {}", path);
            }
        }
        if !report.notes.is_empty() {
            println!("notes:");
            for n in &report.notes {
                println!("  - {}", n);
            }
        }
    }
    Ok(())
}

fn handle_adapter(layout: &Layout, cmd: AdapterCmd) -> Result<ExitCode> {
    match cmd.subcommand {
        AdapterSubcommand::Install(cmd) => {
            let registry = CompatibilityRegistry::load(&layout.registry_path())
                .map_err(|err| anyhow!("read compatibility registry: {}", err))?;
            let installation = store::install(
                layout,
                &registry,
                &cmd.dart_hash,
                cmd.target_arch.as_deref(),
                cmd.from.as_deref(),
            )
            .map_err(|err| anyhow!("{}", err))?;
            if cmd.json {
                println!("{}", serde_json::to_string_pretty(&installation)?);
            } else {
                let record = &installation.record;
                println!(
                    "result: {}",
                    if installation.idempotent {
                        "already-installed"
                    } else {
                        "installed"
                    }
                );
                println!("store: {}", installation.store_dir.display());
                println!("artifact: {}", installation.artifact_path.display());
                println!("snapshot hash: {}", record.snapshot_hash);
                println!("target: {}", record.target_arch);
                println!("host: {}/{}", record.host_os, record.host_arch);
                println!("artifact digest: {} ({} bytes)", record.sha256, record.size);
                println!("artifact id: {}", record.artifact_id);
                println!("artifact source: {}", record.source);
                println!("profile: {} {}", record.profile_id, record.profile_sha256);
                println!("profile path: {}", installation.profile_path.display());
                println!(
                    "compatibility record: {}",
                    record.compatibility_record_sha256
                );
                println!(
                    "protocol/model majors: {}/{}",
                    record.protocol_major, record.model_major
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        AdapterSubcommand::List(cmd) => {
            let rows = available_adapters(layout)?;
            if cmd.json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if rows.is_empty() {
                println!("no compatibility records");
            } else {
                for row in &rows {
                    print!(
                        "hash={} state={} adapter={} target={} host={}/{}",
                        row.snapshot_hash,
                        row.state,
                        row.artifact_id,
                        row.target_arch,
                        row.host_os,
                        row.host_arch
                    );
                    if let Some(path) = row.artifact_path.as_deref() {
                        print!(" artifact={}", path);
                    }
                    if let Some(digest) = row.expected_sha256.as_deref() {
                        print!(" sha256={}", digest);
                    }
                    if let Some(detail) = row.detail.as_deref() {
                        print!(" detail={:?}", detail);
                    }
                    println!();
                }
            }
            // A store that claims installs it cannot back is a failure, not a
            // report: exiting 0 here is how "installed" came to mean "a file
            // with the right name exists".
            let broken = rows.iter().filter(|row| row.state.is_failure()).count();
            if broken > 0 {
                eprintln!(
                    "error: {broken} adapter store entries are {} or {}",
                    EntryState::Missing,
                    EntryState::Corrupt
                );
                return Ok(ExitCode::from(STORE_STATE_FAILURE));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn resolve_toggle(with: bool, without: bool, name: &str) -> Result<Option<bool>> {
    if with && without {
        bail!("conflicting options for {name}");
    }
    if with {
        return Ok(Some(true));
    }
    if without {
        return Ok(Some(false));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompile_scope_defaults_to_app_unknown() {
        let cli = Cli::try_parse_from(["flutterdec", "decompile", "sample.apk", "-o", "out"])
            .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(matches!(cmd.function_scope, FunctionScopeArg::AppUnknown));
    }

    #[test]
    fn decompile_scope_accepts_all() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--function-scope",
            "all",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(matches!(cmd.function_scope, FunctionScopeArg::All));
    }

    #[test]
    fn decompile_accepts_repeated_app_package_filter() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--app-package",
            "spotube",
            "--app-package",
            "provider",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert_eq!(cmd.app_packages, vec!["spotube", "provider"]);
    }

    #[test]
    fn decompile_accepts_emit_asm_opcodes() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--emit-asm",
            "--emit-asm-opcodes",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(cmd.emit_asm);
        assert!(cmd.emit_asm_opcodes);
    }

    #[test]
    fn decompile_rejects_emit_asm_opcodes_without_emit_asm() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--emit-asm-opcodes",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        let err = build_decompile_options(cmd).expect_err("requires --emit-asm");
        assert!(err
            .to_string()
            .contains("--emit-asm-opcodes requires --emit-asm"));
    }

    #[test]
    fn decompile_accepts_emit_ghidra_script() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--emit-ghidra-script",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(cmd.emit_ghidra_script);
    }

    #[test]
    fn decompile_accepts_emit_ida_script() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--emit-ida-script",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(cmd.emit_ida_script);
    }

    #[test]
    fn decompile_adapter_backend_defaults_to_auto() {
        let cli = Cli::try_parse_from(["flutterdec", "decompile", "sample.apk", "-o", "out"])
            .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::Auto));
    }
    #[test]
    fn decompile_adapter_backend_accepts_blutter() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "in.apk",
            "-o",
            "out",
            "--adapter-backend",
            "blutter",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::Blutter));
    }

    #[test]
    fn decompile_adapter_backend_accepts_r2flutter() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "in.apk",
            "-o",
            "out",
            "--adapter-backend",
            "r2-flutter",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::R2Flutter));
        assert_eq!(cmd.adapter_backend.to_core().as_str(), "r2flutter");
    }

    #[test]
    fn info_adapter_backend_defaults_to_auto() {
        let cli = Cli::try_parse_from(["flutterdec", "info", "sample.apk"]).expect("parse");
        let Command::Info(cmd) = cli.command else {
            panic!("expected info command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::Auto));
    }

    #[test]
    fn info_adapter_backend_accepts_r2flutter() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "info",
            "in.apk",
            "--adapter-backend",
            "r2-flutter",
        ])
        .expect("parse");
        let Command::Info(cmd) = cli.command else {
            panic!("expected info command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::R2Flutter));
        assert_eq!(cmd.adapter_backend.to_core().as_str(), "r2flutter");
    }

    #[test]
    fn decompile_accepts_require_snapshot_hash_match() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--require-snapshot-hash-match",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        assert!(cmd.require_snapshot_hash_match);
    }

    #[test]
    fn decompile_target_accepts_entry_va_hex() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--target",
            "0x613468",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        let opt = build_decompile_options(cmd).expect("options");
        assert!(matches!(
            opt.function_target,
            Some(FunctionTarget::EntryVa(0x613468))
        ));
    }

    #[test]
    fn decompile_target_accepts_function_id_prefix() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--target",
            "id:42",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        let opt = build_decompile_options(cmd).expect("options");
        assert!(matches!(
            opt.function_target,
            Some(FunctionTarget::FunctionId(42))
        ));
    }

    #[test]
    fn decompile_accepts_no_apk_startup_analysis() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "decompile",
            "sample.apk",
            "-o",
            "out",
            "--no-apk-startup-analysis",
        ])
        .expect("parse");
        let Command::Decompile(cmd) = cli.command else {
            panic!("expected decompile command");
        };
        let opt = build_decompile_options(cmd).expect("options");
        assert!(!opt.engine_options.apk_startup_analysis);
    }

    #[test]
    fn diff_scope_defaults_to_app_unknown() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "diff",
            "--old",
            "old.apk",
            "--new",
            "new.apk",
            "-o",
            "out",
        ])
        .expect("parse");
        let Command::Diff(cmd) = cli.command else {
            panic!("expected diff command");
        };
        assert!(matches!(cmd.function_scope, FunctionScopeArg::AppUnknown));
    }

    #[test]
    fn diff_accepts_backend_and_package_filters() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "diff",
            "--old",
            "old.apk",
            "--new",
            "new.apk",
            "-o",
            "out",
            "--adapter-backend",
            "blutter",
            "--app-package",
            "spotube",
            "--app-package",
            "provider",
            "--function-scope",
            "all",
        ])
        .expect("parse");
        let Command::Diff(cmd) = cli.command else {
            panic!("expected diff command");
        };
        assert!(matches!(cmd.adapter_backend, AdapterBackendArg::Blutter));
        assert!(matches!(cmd.function_scope, FunctionScopeArg::All));
        assert_eq!(cmd.app_packages, vec!["spotube", "provider"]);
    }

    #[test]
    fn diff_accepts_require_snapshot_hash_match() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "diff",
            "--old",
            "old.apk",
            "--new",
            "new.apk",
            "-o",
            "out",
            "--require-snapshot-hash-match",
        ])
        .expect("parse");
        let Command::Diff(cmd) = cli.command else {
            panic!("expected diff command");
        };
        assert!(cmd.require_snapshot_hash_match);
    }

    #[test]
    fn map_symbols_accepts_register_local_cache() {
        let cli = Cli::try_parse_from([
            "flutterdec",
            "map-symbols",
            "--stripped",
            "libflutter.so",
            "--unstripped",
            "libflutter-unstripped.so",
            "-o",
            "out",
            "--register-local-cache",
        ])
        .expect("parse");
        let Command::MapSymbols(cmd) = cli.command else {
            panic!("expected map-symbols command");
        };
        assert!(cmd.register_local_cache);
    }
}

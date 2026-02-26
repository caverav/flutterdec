use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use flutterdec_adapter::install_adapter;
use flutterdec_core::{
    available_adapters, run_decompile, run_engine_fingerprint, run_info, run_symbol_map,
    AdapterBackend, DecompileAnalysisProfile, DecompileEngineOptionOverrides,
    DecompileEngineOptions, DecompileOptions, EngineFingerprintOptions, FunctionScope,
    SymbolMapOptions,
};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "flutterdec")]
#[command(about = "Static Flutter AOT decompiler research CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Info(InfoCmd),
    Decompile(DecompileCmd),
    EngineFingerprint(EngineFingerprintCmd),
    MapSymbols(MapSymbolsCmd),
    Adapter(AdapterCmd),
}

#[derive(Args, Debug)]
struct InfoCmd {
    input: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DecompileCmd {
    input: PathBuf,
    #[arg(short = 'o', long = "out")]
    out_dir: PathBuf,
    #[arg(long)]
    emit_asm: bool,
    #[arg(long)]
    emit_asm_opcodes: bool,
    #[arg(long)]
    emit_ghidra_script: bool,
    #[arg(long)]
    emit_ir: bool,
    #[arg(long = "extra-symbol-elf")]
    extra_symbol_elfs: Vec<PathBuf>,
    #[arg(long = "extra-symbol-map-targets")]
    extra_symbol_map_targets: Vec<PathBuf>,
    #[arg(long = "include-nearest-symbol-map")]
    include_nearest_symbol_map: bool,
    #[arg(long)]
    focus: Option<String>,
    #[arg(long)]
    max_functions: Option<usize>,
    #[arg(long, default_value_t = 0)]
    max_placeholder_ifs: usize,
    #[arg(long, default_value_t = 0)]
    max_unresolved_cf: usize,
    #[arg(long, default_value_t = 0.30)]
    max_indirect_call_ratio: f64,
    #[arg(long, default_value_t = 0.80)]
    min_disassembly_ratio: f64,
    #[arg(long, value_enum, default_value_t = FunctionScopeArg::AppUnknown)]
    function_scope: FunctionScopeArg,
    #[arg(long = "app-package")]
    app_packages: Vec<String>,
    #[arg(long, value_enum, default_value_t = AdapterBackendArg::Auto)]
    adapter_backend: AdapterBackendArg,
    #[arg(long, value_enum, default_value_t = AnalysisProfileArg::Balanced)]
    analysis_profile: AnalysisProfileArg,
    #[arg(long)]
    with_canonical_model_symbols: bool,
    #[arg(long)]
    no_canonical_model_symbols: bool,
    #[arg(long)]
    with_pool_value_hints: bool,
    #[arg(long)]
    no_pool_value_hints: bool,
    #[arg(long)]
    with_pool_semantic_hints: bool,
    #[arg(long)]
    no_pool_semantic_hints: bool,
    #[arg(long)]
    with_semantic_reporting: bool,
    #[arg(long)]
    no_semantic_reporting: bool,
    #[arg(long)]
    with_bootflow_category_seeds: bool,
    #[arg(long)]
    no_bootflow_category_seeds: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AnalysisProfileArg {
    Light,
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
    #[value(name = "app-unknown")]
    AppUnknown,
    #[value(name = "app")]
    App,
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
    Auto,
    Internal,
    Blutter,
}

impl AdapterBackendArg {
    fn to_core(self) -> AdapterBackend {
        match self {
            Self::Auto => AdapterBackend::Auto,
            Self::Internal => AdapterBackend::Internal,
            Self::Blutter => AdapterBackend::Blutter,
        }
    }
}

#[derive(Args, Debug)]
struct EngineFingerprintCmd {
    input: PathBuf,
    #[arg(short = 'o', long = "out")]
    out_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 24)]
    max_markers: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct MapSymbolsCmd {
    #[arg(long = "stripped")]
    stripped_path: PathBuf,
    #[arg(long = "unstripped")]
    unstripped_path: PathBuf,
    #[arg(short = 'o', long = "out")]
    out_dir: PathBuf,
    #[arg(long)]
    include_branches: bool,
    #[arg(long, default_value_t = 8192)]
    nearest_max_distance: u64,
    #[arg(long)]
    require_exec_match: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum AdapterSubcommand {
    Install(AdapterInstallCmd),
    List,
}

#[derive(Args, Debug)]
struct AdapterInstallCmd {
    #[arg(long = "dart-hash")]
    dart_hash: String,
}

#[derive(Args, Debug)]
struct AdapterCmd {
    #[command(subcommand)]
    subcommand: AdapterSubcommand,
}

fn find_repo_root(start: &Path) -> PathBuf {
    let mut p = start.to_path_buf();
    loop {
        let marker1 = p.join("Cargo.toml");
        let marker2 = p.join("adapters/manifest.json");
        if marker1.exists() && marker2.exists() {
            return p;
        }
        if !p.pop() {
            return start.to_path_buf();
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("resolve current dir")?;
    let repo_root = find_repo_root(&cwd);

    match cli.command {
        Command::Info(cmd) => handle_info(&repo_root, cmd)?,
        Command::Decompile(cmd) => handle_decompile(&repo_root, cmd)?,
        Command::EngineFingerprint(cmd) => handle_engine_fingerprint(cmd)?,
        Command::MapSymbols(cmd) => handle_map_symbols(cmd)?,
        Command::Adapter(cmd) => handle_adapter(&repo_root, cmd)?,
    }

    Ok(())
}

fn handle_info(repo_root: &Path, cmd: InfoCmd) -> Result<()> {
    let out = run_info(repo_root, &cmd.input)?;
    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("input: {}", out.input_path);
        println!("libapp: {}", out.libapp_path);
        println!("arch: {}", out.arch);
        println!("snapshot hash: {}", out.snapshot_hash);
        println!("adapter installed: {}", out.adapter_installed);
        if let Some(n) = out.function_count {
            println!("functions: {}", n);
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

fn handle_decompile(repo_root: &Path, cmd: DecompileCmd) -> Result<()> {
    let input = cmd.input.clone();
    let opt = build_decompile_options(cmd)?;
    let quality = run_decompile(repo_root, &input, &opt)?;
    println!("{}", serde_json::to_string_pretty(&quality)?);
    Ok(())
}

fn build_decompile_options(cmd: DecompileCmd) -> Result<DecompileOptions> {
    if cmd.emit_asm_opcodes && !cmd.emit_asm {
        bail!("--emit-asm-opcodes requires --emit-asm");
    }
    let profile = cmd.analysis_profile.to_core();
    let overrides = resolve_decompile_overrides(&cmd)?;
    let engine_options = DecompileEngineOptions::for_profile(profile).with_overrides(&overrides);
    Ok(DecompileOptions {
        out_dir: cmd.out_dir,
        emit_asm: cmd.emit_asm,
        emit_asm_opcodes: cmd.emit_asm_opcodes,
        emit_ghidra_script: cmd.emit_ghidra_script,
        emit_ir: cmd.emit_ir,
        extra_symbol_elfs: cmd.extra_symbol_elfs,
        extra_symbol_map_targets: cmd.extra_symbol_map_targets,
        include_nearest_symbol_map: cmd.include_nearest_symbol_map,
        focus: cmd.focus,
        max_functions: cmd.max_functions,
        max_placeholder_ifs: cmd.max_placeholder_ifs,
        max_unresolved_cf: cmd.max_unresolved_cf,
        max_indirect_call_ratio: cmd.max_indirect_call_ratio,
        min_disassembly_ratio: cmd.min_disassembly_ratio,
        function_scope: cmd.function_scope.to_core(),
        app_packages: cmd.app_packages,
        adapter_backend: cmd.adapter_backend.to_core(),
        analysis_profile: profile,
        engine_options,
    })
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
    Ok(DecompileEngineOptionOverrides {
        canonical_model_symbols,
        pool_value_hints,
        pool_semantic_hints,
        semantic_reporting,
        bootflow_category_seeds,
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

fn handle_map_symbols(cmd: MapSymbolsCmd) -> Result<()> {
    let opt = SymbolMapOptions {
        out_dir: cmd.out_dir,
        include_branches: cmd.include_branches,
        nearest_max_distance: cmd.nearest_max_distance,
        require_exec_match: cmd.require_exec_match,
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
        if !report.notes.is_empty() {
            println!("notes:");
            for n in &report.notes {
                println!("  - {}", n);
            }
        }
    }
    Ok(())
}

fn handle_adapter(repo_root: &Path, cmd: AdapterCmd) -> Result<()> {
    match cmd.subcommand {
        AdapterSubcommand::Install(cmd) => {
            let path = install_adapter(repo_root, &cmd.dart_hash)?;
            println!("installed adapter: {}", path.display());
        }
        AdapterSubcommand::List => {
            let rows = available_adapters(repo_root)?;
            if rows.is_empty() {
                println!("no manifest entries");
            } else {
                for (hash, version, adapter, installed) in rows {
                    println!(
                        "hash={} version={} adapter={} installed={}",
                        hash, version, adapter, installed
                    );
                }
            }
        }
    }
    Ok(())
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
            "sample.apk",
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
}

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use flutterdec_adapter::install_adapter;
use flutterdec_core::{
    available_adapters, run_decompile, run_info, run_symbol_map, DecompileOptions, SymbolMapOptions,
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
    emit_ir: bool,
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
        Command::Info(cmd) => {
            let out = run_info(&repo_root, &cmd.input)?;
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
            }
        }
        Command::Decompile(cmd) => {
            let opt = DecompileOptions {
                out_dir: cmd.out_dir,
                emit_asm: cmd.emit_asm,
                emit_ir: cmd.emit_ir,
                focus: cmd.focus,
                max_functions: cmd.max_functions,
                max_placeholder_ifs: cmd.max_placeholder_ifs,
                max_unresolved_cf: cmd.max_unresolved_cf,
                max_indirect_call_ratio: cmd.max_indirect_call_ratio,
                min_disassembly_ratio: cmd.min_disassembly_ratio,
            };
            let quality = run_decompile(&repo_root, &cmd.input, &opt)?;
            println!("{}", serde_json::to_string_pretty(&quality)?);
        }
        Command::MapSymbols(cmd) => {
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
        }
        Command::Adapter(adapter_cmd) => match adapter_cmd.subcommand {
            AdapterSubcommand::Install(cmd) => {
                let path = install_adapter(&repo_root, &cmd.dart_hash)?;
                println!("installed adapter: {}", path.display());
            }
            AdapterSubcommand::List => {
                let rows = available_adapters(&repo_root)?;
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
        },
    }

    Ok(())
}

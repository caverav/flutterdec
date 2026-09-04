//! Phase-level benchmark harness for the IR, CFG, emission and serialization
//! stages of the decompile pipeline.
//!
//! One process times a combined in-memory invocation per case and reports four
//! disjoint spans inside it:
//!
//! - `ir`: `build_function_ir` entry through its `FunctionIr` return.
//! - `cfg`: region-analysis entry through reachability, dominators,
//!   post-dominators, loops and regions. Nested inside emission, and charged by
//!   the decompiler under the `bench-spans` feature.
//! - `emission_exclusive`: emitter entry through the `PseudocodeArtifact`
//!   return, minus the nested CFG span.
//! - `serialization`: finished artifacts through in-memory pseudocode, emitted
//!   IR JSON, quality JSON and the artifact-derived section of the report JSON.
//!
//! Fixture generation, process startup, compilation, disk IO and output
//! formatting are all outside every span. Nothing is written to disk inside a
//! measured region.

mod json;
mod measure;
mod rng;
mod sha256;
mod stats;
mod workload;

use flutterdec_decompiler::{emit_pseudocode, PseudocodeArtifact};
use flutterdec_ir::{build_function_ir, FunctionIr};
use json::Json;
use measure::{Allocations, Host, ResourceMetrics, ResourceSnapshot};
use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;
use workload::Case;

/// Counting is per thread and lock free, so it can stay on during the measured
/// runs instead of forcing a separate uninstrumented pass whose timings would
/// then not correspond to the reported allocation numbers.
#[global_allocator]
static ALLOCATOR: measure::CountingAllocator = measure::CountingAllocator;

const DISCLOSED_SEED: u64 = 1_592_614_637;
const DEFAULT_WARMUPS: usize = 3;
const DEFAULT_RUNS: usize = 15;
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_MEMORY_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const RECONCILIATION_TOLERANCE: f64 = 0.02;
const TIMER_CALIBRATION_SAMPLES: usize = 4096;

const USAGE: &str = "\
flutterdec-bench - disjoint phase timing for the decompile pipeline

  run        Time the matrix and write a result document plus a sample stream
  manifest   Describe the matrix and its digests without timing anything
  aggregate  Pair two sample streams into medians, deviations and MDE
  resource   Measure allocation resources separately from timing selection

run options
  --matrix disclosed|held-out   Case set (default disclosed)
  --held-out-seed HEX           128-bit hex seed, required for --matrix held-out
  --seed N                      Disclosed seed, recorded only (default 1592614637)
  --warmups N                   Unmeasured passes (default 3)
  --runs N                      Measured passes (default 15). 0 runs the
                                warmups and the correctness pass only, which is
                                how a binary is warmed once before interleaving
                                measured pairs at --warmups 0.
  --correctness on|off          Structural and determinism pass over every case
                                before any span opens (default on). It costs
                                about twice a measured pass and its verdict is a
                                property of the binary, not of a pass, so an
                                interleaved schedule runs it once per binary in
                                the warmup invocation and off in each measured
                                pair. Off is refused with --runs 0, which would
                                otherwise do nothing at all.
  --timeout-seconds N           Per measured run (default 120)
  --memory-limit-bytes N        Peak resident set (default 2 GiB)
  --product-ref REF             Recorded binding
  --harness-ref REF             Recorded binding
  --patch-sha256 HEX            Recorded binding
  --binary-sha256 HEX           Recorded binding
  --label TEXT                  Free-form run label
  --out PATH                    Result document (default stdout)
  --samples PATH                Tab-separated sample stream

manifest options
  --matrix, --held-out-seed, --out as above

aggregate options
  --reference PATH              Sample stream from the reference revision
  --candidate PATH              Sample stream from the candidate revision
  --out PATH                    Analysis document (default stdout)

resource options
  --matrix, --held-out-seed, --warmups, and binding options as above
  --plant none|cfg-graph-clone|emitter-block-clone (default none)
  --out PATH                    Resource JSON document (default stdout)
  --samples PATH                Per-case/per-phase resource TSV
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("manifest") => manifest(&args[1..]),
        Some("aggregate") => aggregate(&args[1..]),
        Some("resource") => resource(&args[1..]),
        Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown subcommand {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("flutterdec-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    values: HashMap<String, String>,
}

impl Args {
    fn parse(argv: &[String]) -> Result<Self, String> {
        let mut values = HashMap::new();
        let mut index = 0usize;
        while index < argv.len() {
            let key = &argv[index];
            let Some(name) = key.strip_prefix("--") else {
                return Err(format!("expected an option, found {key}"));
            };
            let Some(value) = argv.get(index + 1) else {
                return Err(format!("--{name} needs a value"));
            };
            values.insert(name.to_string(), value.clone());
            index += 2;
        }
        Ok(Self { values })
    }

    fn text(&self, name: &str, fallback: &str) -> String {
        self.values
            .get(name)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    }

    fn number(&self, name: &str, fallback: u64) -> Result<u64, String> {
        match self.values.get(name) {
            None => Ok(fallback),
            Some(raw) => raw
                .parse()
                .map_err(|e| format!("--{name}: {raw} is not a number: {e}")),
        }
    }

    fn path(&self, name: &str) -> Option<&String> {
        self.values.get(name)
    }
}

fn cases_for(args: &Args) -> Result<(Vec<Case>, String, Option<u128>), String> {
    match args.text("matrix", "disclosed").as_str() {
        "disclosed" => Ok((workload::disclosed_cases(), "disclosed".to_string(), None)),
        "held-out" => {
            let raw = args
                .path("held-out-seed")
                .ok_or("--matrix held-out needs --held-out-seed")?;
            let seed = u128::from_str_radix(raw.trim_start_matches("0x"), 16)
                .map_err(|e| format!("--held-out-seed must be 128-bit hex: {e}"))?;
            Ok((
                workload::held_out_cases(seed),
                "held-out".to_string(),
                Some(seed),
            ))
        }
        other => Err(format!(
            "--matrix must be disclosed or held-out, got {other}"
        )),
    }
}

/// One digest over every case digest in order, so a single string binds the
/// whole matrix. This is what a held-out manifest is recorded by.
fn matrix_digest(cases: &[Case]) -> String {
    let mut hasher = sha256::Sha256::new();
    hasher.update(b"flutterdec-bench/matrix/v1\n");
    for case in cases {
        hasher.update(case.name.as_bytes());
        hasher.update(b" ");
        hasher.update(case.workload_sha256.as_bytes());
        hasher.update(b"\n");
    }
    sha256::hex(&hasher.finish())
}

fn case_manifest(case: &Case) -> Json {
    Json::o(vec![
        ("case", Json::s(case.name.clone())),
        ("topology", Json::s(case.topology.clone())),
        ("blocks", Json::U(case.blocks as u64)),
        ("load", Json::s(case.load.clone())),
        (
            "instructions_per_block",
            Json::U(case.instructions_per_block as u64),
        ),
        (
            "instructions",
            Json::U(case.disasm.instructions.len() as u64),
        ),
        ("workload_sha256", Json::s(case.workload_sha256.clone())),
    ])
}

fn manifest(argv: &[String]) -> Result<(), String> {
    let args = Args::parse(argv)?;
    let (cases, matrix, seed) = cases_for(&args)?;
    let document = Json::o(vec![
        ("schema", Json::s("flutterdec-bench/manifest/1")),
        ("matrix", Json::s(matrix)),
        (
            "held_out_seed_hex",
            match seed {
                Some(seed) => Json::s(format!("{seed:032x}")),
                None => Json::Null,
            },
        ),
        ("case_count", Json::U(cases.len() as u64)),
        ("matrix_sha256", Json::s(matrix_digest(&cases))),
        ("cases", Json::A(cases.iter().map(case_manifest).collect())),
    ]);
    emit(args.path("out"), &document.to_pretty())
}

struct Correctness {
    checks: Vec<(&'static str, bool)>,
    artifact_sha256: String,
    source_lines: usize,
    helper_definitions: usize,
    helper_references: usize,
}

impl Correctness {
    fn passed(&self) -> bool {
        self.checks.iter().all(|(_, ok)| *ok)
    }

    fn to_json(&self) -> Json {
        let mut fields: Vec<(&str, Json)> = self
            .checks
            .iter()
            .map(|(name, ok)| (*name, Json::Bool(*ok)))
            .collect();
        fields.push(("passed", Json::Bool(self.passed())));
        fields.push(("artifact_sha256", Json::s(self.artifact_sha256.clone())));
        fields.push(("source_lines", Json::U(self.source_lines as u64)));
        fields.push((
            "helper_definitions",
            Json::U(self.helper_definitions as u64),
        ));
        fields.push(("helper_references", Json::U(self.helper_references as u64)));
        Json::o(fields)
    }
}

/// Every `_block_N` the source mentions, split into the ones it defines and the
/// ones it only calls. A call with no definition is the dangling-helper defect
/// the mission exists to prevent, so a run that produced one is not a valid
/// measurement whatever its timings say.
fn helper_ids(source: &str) -> (Vec<u32>, Vec<u32>) {
    let mut defined = Vec::new();
    let mut referenced = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let mut rest = line;
        while let Some(at) = rest.find("_block_") {
            let tail = &rest[at + "_block_".len()..];
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            rest = &tail[digits.len()..];
            let Ok(id) = digits.parse::<u32>() else {
                continue;
            };
            if trimmed.starts_with("dynamic _block_") {
                defined.push(id);
            } else if rest.starts_with('(') {
                referenced.push(id);
            }
        }
    }
    defined.sort_unstable();
    defined.dedup();
    referenced.sort_unstable();
    referenced.dedup();
    (defined, referenced)
}

fn check_case(case: &Case, symbols: &HashMap<u64, String>) -> Correctness {
    let ir = build_function_ir(&case.disasm);

    let blocks_match = ir.blocks.len() == case.blocks;
    let dense_ids = ir.blocks.iter().enumerate().all(|(i, b)| b.id == i);
    let succs_match = blocks_match
        && ir
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.succs == case.expected_succs[i]);
    // Indexed by id, which is only the same as position when the ids are dense,
    // so the lookups go through `get` and a missing edge reads as a failure
    // rather than a panic.
    let reciprocal = ir.blocks.iter().all(|block| {
        block.succs.iter().all(|s| {
            ir.blocks
                .get(*s)
                .is_some_and(|target| target.preds.contains(&block.id))
        })
    }) && ir.blocks.iter().all(|block| {
        block.preds.iter().all(|p| {
            ir.blocks
                .get(*p)
                .is_some_and(|source| source.succs.contains(&block.id))
        })
    });

    let artifact = emit_pseudocode(&ir, symbols);
    let repeat = emit_pseudocode(&build_function_ir(&case.disasm), symbols);
    let deterministic = artifact.source == repeat.source;

    let (defined, referenced) = helper_ids(&artifact.source);
    let no_dangling_helper = referenced.iter().all(|id| defined.contains(id));

    Correctness {
        checks: vec![
            ("block_count_matches", blocks_match),
            ("successors_match", succs_match),
            ("dense_block_ids", dense_ids),
            ("predecessors_reciprocate", reciprocal),
            ("artifact_non_empty", !artifact.source.is_empty()),
            (
                "artifact_declares_a_function",
                artifact.source.starts_with("dynamic "),
            ),
            ("no_dangling_helper_call", no_dangling_helper),
            ("emission_is_deterministic", deterministic),
        ],
        artifact_sha256: sha256::digest_hex(artifact.source.as_bytes()),
        source_lines: artifact.source.lines().count(),
        helper_definitions: defined.len(),
        helper_references: referenced.len(),
    }
}

struct Measurement {
    combined: u64,
    ir: u64,
    cfg: u64,
    emission_exclusive: u64,
    serialization: u64,
    /// Allocations for the three spans that have a boundary the counter can be
    /// read at. Region analysis runs inside emission and shares its counter:
    /// splitting it out would need a read inside the CFG span, which is the one
    /// place the instrumentation deliberately does not reach.
    ir_allocations: Allocations,
    emission_allocations: Allocations,
    serialization_allocations: Allocations,
    serialized_bytes: usize,
}

impl Measurement {
    fn span_sum(&self) -> u64 {
        self.ir + self.cfg + self.emission_exclusive + self.serialization
    }

    /// How much of the combined span the four parts fail to account for. It is
    /// positive by construction, because the combined span also contains the
    /// clock reads at the inner boundaries; timer calibration is what that
    /// residue is compared against.
    fn reconciliation(&self) -> f64 {
        if self.combined == 0 {
            return 0.0;
        }
        (self.combined as f64 - self.span_sum() as f64) / self.combined as f64
    }
}

/// The combined in-memory invocation. Everything outside it - case generation,
/// the symbol map, the model and the option set - is built once by the caller,
/// because the contract excludes fixture generation and startup from the spans.
fn run_case(
    case: &Case,
    symbols: &HashMap<u64, String>,
    model: &flutterdec_adapter::ProgramModel,
    options: &flutterdec_core::DecompileOptions,
) -> Measurement {
    let allocations_0 = Allocations::now();
    let combined_started = Instant::now();

    let ir_started = Instant::now();
    let ir: FunctionIr = build_function_ir(&case.disasm);
    let ir_nanos = ir_started.elapsed().as_nanos() as u64;
    let allocations_1 = Allocations::now();

    // Clear before the span, never inside it: a stale charge from a previous
    // case would be subtracted from this one's emitter time.
    let _ = flutterdec_decompiler::bench_spans::take_cfg_nanos();
    let emission_started = Instant::now();
    let artifact: PseudocodeArtifact = emit_pseudocode(&ir, symbols);
    let emission_nanos = emission_started.elapsed().as_nanos() as u64;
    let cfg_nanos = flutterdec_decompiler::bench_spans::take_cfg_nanos();
    let allocations_2 = Allocations::now();

    let serialization_started = Instant::now();
    let serialized_bytes = flutterdec_core::bench_spans::serialize_artifacts(
        std::slice::from_ref(&ir),
        std::slice::from_ref(&artifact),
        model,
        options,
        1,
    );
    let serialization_nanos = serialization_started.elapsed().as_nanos() as u64;
    let allocations_3 = Allocations::now();

    let combined = combined_started.elapsed().as_nanos() as u64;

    Measurement {
        combined,
        ir: ir_nanos,
        cfg: cfg_nanos,
        emission_exclusive: emission_nanos.saturating_sub(cfg_nanos),
        serialization: serialization_nanos,
        ir_allocations: allocations_1.since(allocations_0),
        emission_allocations: allocations_2.since(allocations_1),
        serialization_allocations: allocations_3.since(allocations_2),
        serialized_bytes,
    }
}

fn resource_case(
    case: &Case,
    symbols: &HashMap<u64, String>,
    model: &flutterdec_adapter::ProgramModel,
    options: &flutterdec_core::DecompileOptions,
) -> ResourceSnapshot {
    use flutterdec_decompiler::bench_spans::{enter_resource_phase, ResourcePhase};

    measure::reset_resources();
    let ir: FunctionIr = {
        let _phase = enter_resource_phase(ResourcePhase::Ir);
        build_function_ir(&case.disasm)
    };
    let artifact: PseudocodeArtifact = {
        let _phase = enter_resource_phase(ResourcePhase::EmissionExclusive);
        emit_pseudocode(&ir, symbols)
    };
    let serialized_bytes = {
        let _phase = enter_resource_phase(ResourcePhase::Serialization);
        flutterdec_core::bench_spans::serialize_artifacts(
            std::slice::from_ref(&ir),
            std::slice::from_ref(&artifact),
            model,
            options,
            1,
        )
    };
    std::hint::black_box(serialized_bytes);
    measure::resource_snapshot()
}

fn resource_metrics_json(metrics: ResourceMetrics) -> Json {
    Json::o(vec![
        ("allocation_count", Json::U(metrics.allocation_count)),
        (
            "total_allocated_bytes",
            Json::U(metrics.total_allocated_bytes),
        ),
        ("peak_live_bytes", Json::U(metrics.peak_live_bytes)),
        ("live_bytes_at_snapshot", Json::U(metrics.live_bytes)),
    ])
}

fn resource(argv: &[String]) -> Result<(), String> {
    use flutterdec_decompiler::bench_spans::{set_resource_plant, ResourcePlant};

    let args = Args::parse(argv)?;
    let (cases, matrix, held_out_seed) = cases_for(&args)?;
    let warmups = args.number("warmups", DEFAULT_WARMUPS as u64)? as usize;
    let plant_name = args.text("plant", "none");
    let plant = match plant_name.as_str() {
        "none" => ResourcePlant::None,
        "cfg-graph-clone" => ResourcePlant::CfgGraphClone,
        "emitter-block-clone" => ResourcePlant::EmitterBlockClone,
        other => return Err(format!("unknown resource plant {other}")),
    };
    set_resource_plant(ResourcePlant::None);

    let symbols: HashMap<u64, String> = HashMap::new();
    let model = flutterdec_core::bench_spans::synthetic_model(1);
    let options = flutterdec_core::bench_spans::balanced_options();
    for _ in 0..warmups {
        for case in &cases {
            std::hint::black_box(run_case(case, &symbols, &model, &options).serialized_bytes);
        }
    }

    set_resource_plant(plant);
    let mut rows = Vec::with_capacity(cases.len());
    let mut sample_text = String::from(
        "case\tphase\tallocation_count\ttotal_allocated_bytes\tpeak_live_bytes\tprocess_peak_rss_bytes\n",
    );
    let phase_names = ["ir", "cfg", "emission_exclusive", "serialization"];
    let mut failures = Vec::new();
    for case in &cases {
        let snapshot = resource_case(case, &symbols, &model, &options);
        let rss = measure::peak_rss_bytes();
        if snapshot.instrumentation_recursions != 0 {
            failures.push(format!(
                "{}: {} allocator instrumentation recursion(s)",
                case.name, snapshot.instrumentation_recursions
            ));
        }
        let mut phases = Vec::new();
        for (name, metrics) in phase_names.iter().zip(snapshot.phases) {
            phases.push(Json::o(vec![
                ("phase", Json::s(*name)),
                ("metrics", resource_metrics_json(metrics)),
            ]));
            sample_text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                case.name,
                name,
                metrics.allocation_count,
                metrics.total_allocated_bytes,
                metrics.peak_live_bytes,
                rss.map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            ));
        }
        sample_text.push_str(&format!(
            "{}\tcombined\t{}\t{}\t{}\t{}\n",
            case.name,
            snapshot.combined.allocation_count,
            snapshot.combined.total_allocated_bytes,
            snapshot.combined.peak_live_bytes,
            rss.map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        ));
        rows.push(Json::o(vec![
            ("case", Json::s(case.name.clone())),
            ("epoch", Json::U(snapshot.epoch)),
            (
                "instrumentation_recursions",
                Json::U(snapshot.instrumentation_recursions),
            ),
            (
                "process_peak_rss_bytes",
                rss.map(Json::U).unwrap_or(Json::Null),
            ),
            ("phases", Json::A(phases)),
            ("combined", resource_metrics_json(snapshot.combined)),
        ]));
    }
    set_resource_plant(ResourcePlant::None);

    let document = Json::o(vec![
        ("schema", Json::s("flutterdec-bench/resource/1")),
        (
            "binding",
            Json::o(vec![
                ("label", Json::s(args.text("label", ""))),
                ("product_ref", Json::s(args.text("product-ref", "unset"))),
                ("harness_ref", Json::s(args.text("harness-ref", "unset"))),
                ("patch_sha256", Json::s(args.text("patch-sha256", "unset"))),
                (
                    "binary_sha256",
                    Json::s(args.text("binary-sha256", "unset")),
                ),
                ("matrix", Json::s(matrix)),
                ("matrix_sha256", Json::s(matrix_digest(&cases))),
                (
                    "held_out_seed_hex",
                    held_out_seed
                        .map(|seed| Json::s(format!("{seed:032x}")))
                        .unwrap_or(Json::Null),
                ),
                ("warmups", Json::U(warmups as u64)),
                ("threads", Json::U(1)),
                ("plant", Json::s(plant_name)),
                ("profile", Json::s(build_profile())),
            ]),
        ),
        ("cases", Json::A(rows)),
    ]);
    emit(args.path("out"), &document.to_pretty())?;
    if let Some(path) = args.path("samples") {
        std::fs::write(path, sample_text).map_err(|e| format!("write {path}: {e}"))?;
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

const PHASES: [&str; 5] = [
    "ir",
    "cfg",
    "emission_exclusive",
    "serialization",
    "combined",
];

fn run(argv: &[String]) -> Result<(), String> {
    let args = Args::parse(argv)?;
    let (cases, matrix, held_out_seed) = cases_for(&args)?;
    let warmups = args.number("warmups", DEFAULT_WARMUPS as u64)? as usize;
    let runs = args.number("runs", DEFAULT_RUNS as u64)? as usize;
    let timeout_seconds = args.number("timeout-seconds", DEFAULT_TIMEOUT_SECONDS)?;
    let memory_limit = args.number("memory-limit-bytes", DEFAULT_MEMORY_LIMIT_BYTES)?;
    let seed = args.number("seed", DISCLOSED_SEED)?;
    let correctness_enabled = match args.text("correctness", "on").as_str() {
        "on" => true,
        "off" => false,
        other => return Err(format!("--correctness must be on or off, got {other}")),
    };
    // Warmup-only with no correctness pass would generate the fixtures, run the
    // warmups and prove nothing, so it is a usage error rather than a fast path.
    if runs == 0 && !correctness_enabled {
        return Err("--runs 0 with --correctness off would measure and prove nothing".to_string());
    }

    // Fixture state, all built before any span opens.
    let symbols: HashMap<u64, String> = HashMap::new();
    let model = flutterdec_core::bench_spans::synthetic_model(1);
    let options = flutterdec_core::bench_spans::balanced_options();

    let correctness: Vec<Correctness> = if correctness_enabled {
        cases
            .iter()
            .map(|case| check_case(case, &symbols))
            .collect()
    } else {
        Vec::new()
    };
    let correctness_failures: Vec<&str> = cases
        .iter()
        .zip(&correctness)
        .filter(|(_, c)| !c.passed())
        .map(|(case, _)| case.name.as_str())
        .collect();

    let timer_overhead = measure::timer_overhead_nanos(TIMER_CALIBRATION_SAMPLES);

    for _ in 0..warmups {
        for case in &cases {
            let measurement = run_case(case, &symbols, &model, &options);
            std::hint::black_box(measurement.serialized_bytes);
        }
    }

    let mut samples: Vec<stats::Sample> = Vec::new();
    let mut run_rows: Vec<Json> = Vec::new();
    let mut over_timeout: Vec<usize> = Vec::new();
    let mut worst_reconciliation = 0.0f64;
    let mut reconciliation_failures: Vec<String> = Vec::new();

    for run_index in 0..runs {
        let run_started = Instant::now();
        let mut case_rows = Vec::with_capacity(cases.len());
        for case in &cases {
            let measurement = run_case(case, &symbols, &model, &options);
            std::hint::black_box(measurement.serialized_bytes);

            let values = [
                measurement.ir,
                measurement.cfg,
                measurement.emission_exclusive,
                measurement.serialization,
                measurement.combined,
            ];
            for (phase, nanos) in PHASES.iter().zip(values) {
                let allocations = match *phase {
                    "ir" => measurement.ir_allocations,
                    // Charged to the emitter, which is where the counter has a
                    // boundary to read at.
                    "cfg" => Allocations { count: 0, bytes: 0 },
                    "emission_exclusive" => measurement.emission_allocations,
                    "serialization" => measurement.serialization_allocations,
                    _ => Allocations {
                        count: measurement.ir_allocations.count
                            + measurement.emission_allocations.count
                            + measurement.serialization_allocations.count,
                        bytes: measurement.ir_allocations.bytes
                            + measurement.emission_allocations.bytes
                            + measurement.serialization_allocations.bytes,
                    },
                };
                samples.push(stats::Sample {
                    run: run_index,
                    case: case.name.clone(),
                    phase: (*phase).to_string(),
                    nanos,
                    alloc_count: allocations.count,
                    alloc_bytes: allocations.bytes,
                });
            }

            let residue = measurement.reconciliation();
            if residue.abs() > worst_reconciliation.abs() {
                worst_reconciliation = residue;
            }
            if residue.abs() > RECONCILIATION_TOLERANCE {
                reconciliation_failures
                    .push(format!("{} run {run_index}: {:.4}", case.name, residue));
            }

            case_rows.push(Json::o(vec![
                ("case", Json::s(case.name.clone())),
                ("combined_nanos", Json::U(measurement.combined)),
                ("ir_nanos", Json::U(measurement.ir)),
                ("cfg_nanos", Json::U(measurement.cfg)),
                (
                    "emission_exclusive_nanos",
                    Json::U(measurement.emission_exclusive),
                ),
                ("serialization_nanos", Json::U(measurement.serialization)),
                ("span_sum_nanos", Json::U(measurement.span_sum())),
                ("unaccounted_fraction", Json::F(residue)),
                (
                    "serialized_bytes",
                    Json::U(measurement.serialized_bytes as u64),
                ),
            ]));
        }
        let run_seconds = run_started.elapsed().as_secs_f64();
        if run_seconds > timeout_seconds as f64 {
            over_timeout.push(run_index);
        }
        run_rows.push(Json::o(vec![
            ("run", Json::U(run_index as u64)),
            ("seconds", Json::F(run_seconds)),
            ("cases", Json::A(case_rows)),
        ]));
    }

    let peak_rss = measure::peak_rss_bytes();
    let within_memory = peak_rss.map(|bytes| bytes <= memory_limit);
    let host: Host = measure::host();

    let document = Json::o(vec![
        ("schema", Json::s("flutterdec-bench/result/1")),
        (
            "binding",
            Json::o(vec![
                ("label", Json::s(args.text("label", ""))),
                ("product_ref", Json::s(args.text("product-ref", "unset"))),
                ("harness_ref", Json::s(args.text("harness-ref", "unset"))),
                ("patch_sha256", Json::s(args.text("patch-sha256", "unset"))),
                (
                    "binary_sha256",
                    Json::s(args.text("binary-sha256", "unset")),
                ),
                ("matrix", Json::s(matrix)),
                ("matrix_sha256", Json::s(matrix_digest(&cases))),
                (
                    "held_out_seed_hex",
                    match held_out_seed {
                        Some(seed) => Json::s(format!("{seed:032x}")),
                        None => Json::Null,
                    },
                ),
                ("disclosed_seed", Json::U(seed)),
                ("profile", Json::s(build_profile())),
                ("warmups", Json::U(warmups as u64)),
                ("measured_runs", Json::U(runs as u64)),
                ("correctness_checked", Json::Bool(correctness_enabled)),
                (
                    "command",
                    Json::s(std::env::args().collect::<Vec<_>>().join(" ")),
                ),
                ("threads", Json::U(1)),
            ]),
        ),
        (
            "host",
            Json::o(vec![
                ("hostname", Json::s(host.hostname)),
                ("kernel", Json::s(host.kernel)),
                ("cpu_model", Json::s(host.cpu_model)),
                ("logical_cpus", Json::U(host.logical_cpus as u64)),
            ]),
        ),
        (
            "limits",
            Json::o(vec![
                ("timeout_seconds", Json::U(timeout_seconds)),
                ("memory_limit_bytes", Json::U(memory_limit)),
                (
                    "peak_rss_bytes",
                    match peak_rss {
                        Some(bytes) => Json::U(bytes),
                        None => Json::Null,
                    },
                ),
                (
                    "within_memory_limit",
                    match within_memory {
                        Some(ok) => Json::Bool(ok),
                        None => Json::Null,
                    },
                ),
                (
                    "runs_over_timeout",
                    Json::A(over_timeout.iter().map(|r| Json::U(*r as u64)).collect()),
                ),
            ]),
        ),
        (
            "timer",
            Json::o(vec![
                ("overhead_nanos", Json::U(timer_overhead)),
                (
                    "calibration_samples",
                    Json::U(TIMER_CALIBRATION_SAMPLES as u64),
                ),
                (
                    "reconciliation_tolerance",
                    Json::F(RECONCILIATION_TOLERANCE),
                ),
                ("worst_unaccounted_fraction", Json::F(worst_reconciliation)),
                (
                    "reconciliation_failures",
                    Json::A(
                        reconciliation_failures
                            .iter()
                            .map(|f| Json::s(f.clone()))
                            .collect(),
                    ),
                ),
            ]),
        ),
        (
            "cases",
            Json::A(
                cases
                    .iter()
                    .enumerate()
                    .map(|(index, case)| {
                        let Json::O(mut fields) = case_manifest(case) else {
                            unreachable!("case manifest is an object")
                        };
                        // Absent rather than a passing default when the pass did
                        // not run, so a document cannot be read as a clean
                        // verdict it never produced.
                        if let Some(check) = correctness.get(index) {
                            fields.push(("correctness".to_string(), check.to_json()));
                        }
                        Json::O(fields)
                    })
                    .collect(),
            ),
        ),
        (
            "correctness_failures",
            Json::A(
                correctness_failures
                    .iter()
                    .map(|name| Json::s((*name).to_string()))
                    .collect(),
            ),
        ),
        ("runs", Json::A(run_rows)),
    ]);

    emit(args.path("out"), &document.to_pretty())?;

    if let Some(path) = args.path("samples") {
        let mut text = String::from(stats::SAMPLE_HEADER);
        text.push('\n');
        for sample in &samples {
            text.push_str(&sample.to_row());
            text.push('\n');
        }
        std::fs::write(path, text).map_err(|e| format!("write {path}: {e}"))?;
    }

    let mut failures = Vec::new();
    if !correctness_failures.is_empty() {
        failures.push(format!(
            "correctness failed on {} case(s): {}",
            correctness_failures.len(),
            correctness_failures.join(", ")
        ));
    }
    if !reconciliation_failures.is_empty() {
        failures.push(format!(
            "{} span sums missed the combined time by more than {:.0} percent",
            reconciliation_failures.len(),
            RECONCILIATION_TOLERANCE * 100.0
        ));
    }
    if !over_timeout.is_empty() {
        failures.push(format!(
            "{} measured run(s) exceeded {timeout_seconds}s",
            over_timeout.len()
        ));
    }
    if within_memory == Some(false) {
        failures.push(format!(
            "peak resident set {:?} exceeded {memory_limit} bytes",
            peak_rss
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn aggregate(argv: &[String]) -> Result<(), String> {
    let args = Args::parse(argv)?;
    let read = |name: &str| -> Result<Vec<stats::Sample>, String> {
        let path = args
            .path(name)
            .ok_or_else(|| format!("--{name} is required"))?;
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        stats::parse_samples(&text).map_err(|e| format!("{path}: {e}"))
    };
    let reference = read("reference")?;
    let candidate = read("candidate")?;
    emit(
        args.path("out"),
        &stats::analyse(&reference, &candidate).to_pretty(),
    )
}

fn emit(path: Option<&String>, text: &str) -> Result<(), String> {
    match path {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("write {path}: {e}")),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// Whether this binary was built with optimisations. A debug-built measurement
/// is not comparable to a release one, and reading it out of the build rather
/// than off a flag means a run cannot claim release mode it does not have.
fn build_profile() -> String {
    if cfg!(debug_assertions) {
        "debug".to_string()
    } else {
        "release".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four spans have to add up to the combined span, and the shortfall
    /// has to be the clock reads at the boundaries rather than untimed work.
    #[test]
    fn spans_are_disjoint_and_reconcile_with_the_combined_time() {
        let symbols = HashMap::new();
        let model = flutterdec_core::bench_spans::synthetic_model(1);
        let options = flutterdec_core::bench_spans::balanced_options();
        for case in workload::disclosed_cases()
            .into_iter()
            .filter(|c| c.blocks == 64)
        {
            // One warm pass first: the first touch of a case pays for lazily
            // initialised state that belongs to no phase.
            run_case(&case, &symbols, &model, &options);
            let measurement = run_case(&case, &symbols, &model, &options);
            assert!(
                measurement.span_sum() <= measurement.combined,
                "{}: parts {} exceed the whole {}",
                case.name,
                measurement.span_sum(),
                measurement.combined
            );
            assert!(
                measurement.reconciliation() <= RECONCILIATION_TOLERANCE,
                "{}: {:.4} of the combined span is unaccounted for",
                case.name,
                measurement.reconciliation()
            );
            assert!(measurement.serialized_bytes > 0, "{}", case.name);
        }
    }

    /// Region analysis is nested inside emission, so a positive CFG span that
    /// exceeded the emitter's total would mean the subtraction is wrong and the
    /// two spans overlap.
    #[test]
    fn the_cfg_span_is_charged_from_inside_emission() {
        let symbols = HashMap::new();
        let model = flutterdec_core::bench_spans::synthetic_model(1);
        let options = flutterdec_core::bench_spans::balanced_options();
        let case = workload::disclosed_cases()
            .into_iter()
            .find(|c| c.name == "nested-loop/1024/base")
            .expect("nested-loop case");
        run_case(&case, &symbols, &model, &options);
        let measurement = run_case(&case, &symbols, &model, &options);
        assert!(measurement.cfg > 0, "region analysis was charged");
        assert!(
            measurement.cfg <= measurement.cfg + measurement.emission_exclusive,
            "the emitter total cannot be smaller than its nested part"
        );
    }

    /// Every disclosed case has to pass every structural check, or the baseline
    /// is measuring a graph that does not hold together.
    ///
    /// Restricted to the two smaller sizes here because the check emits each
    /// case twice to prove determinism, and the 1024-block irreducible case
    /// alone takes seconds under the unoptimised test profile. The 1024 sizes
    /// are covered by the harness in the run that matters: `run` executes this
    /// same pass over every case before it opens a single span, and returns a
    /// failure exit code when any of it does not hold.
    #[test]
    fn every_disclosed_case_passes_correctness() {
        let symbols = HashMap::new();
        for case in workload::disclosed_cases()
            .into_iter()
            .filter(|c| c.blocks <= 256)
        {
            let check = check_case(&case, &symbols);
            assert!(
                check.passed(),
                "{}: {:?}",
                case.name,
                check
                    .checks
                    .iter()
                    .filter(|(_, ok)| !ok)
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
            );
        }
    }

    /// The dangling-helper scan has to see a call with no definition, or it
    /// would pass a source that has one.
    #[test]
    fn the_helper_scan_separates_definitions_from_calls() {
        let source =
            "dynamic f() {\n  return _block_7();\n}\n\ndynamic _block_7() {\n  return null;\n}";
        let (defined, referenced) = helper_ids(source);
        assert_eq!(defined, vec![7]);
        assert_eq!(referenced, vec![7]);

        let dangling = "dynamic f() {\n  return _block_9();\n}";
        let (defined, referenced) = helper_ids(dangling);
        assert!(defined.is_empty());
        assert_eq!(referenced, vec![9]);
    }

    #[test]
    fn argument_parsing_rejects_a_missing_value() {
        let argv: Vec<String> = vec!["--runs".to_string()];
        assert!(Args::parse(&argv).is_err());
        let argv: Vec<String> = vec!["runs".to_string(), "3".to_string()];
        assert!(Args::parse(&argv).is_err());
        let argv: Vec<String> = vec!["--runs".to_string(), "3".to_string()];
        assert_eq!(
            Args::parse(&argv).expect("parses").number("runs", 15),
            Ok(3)
        );
    }

    /// A warmup-only invocation with the correctness pass turned off would run
    /// the warmups, measure nothing and assert nothing, so it has to be refused
    /// rather than silently produce an empty document that reads as a clean run.
    #[test]
    fn a_run_that_would_prove_nothing_is_refused() {
        let argv =
            |flags: &[&str]| -> Vec<String> { flags.iter().map(|f| (*f).to_string()).collect() };
        let error = run(&argv(&["--runs", "0", "--correctness", "off"]))
            .expect_err("warmup-only with no correctness pass is refused");
        assert!(error.contains("prove nothing"), "{error}");

        let error =
            run(&argv(&["--correctness", "sometimes"])).expect_err("only on and off are accepted");
        assert!(error.contains("must be on or off"), "{error}");
    }

    /// The matrix digest binds the whole case set, so a dropped or reordered
    /// case cannot pass as the same workload.
    #[test]
    fn the_matrix_digest_binds_the_case_set() {
        let cases = workload::disclosed_cases();
        let full = matrix_digest(&cases);
        assert_eq!(full, matrix_digest(&workload::disclosed_cases()));
        assert_ne!(full, matrix_digest(&cases[1..]));
        let mut reordered = workload::disclosed_cases();
        reordered.swap(0, 1);
        assert_ne!(full, matrix_digest(&reordered));
    }
}

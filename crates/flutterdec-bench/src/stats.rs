//! Paired aggregation exactly as the metric contract states it.
//!
//! Per phase and per case: the median and median absolute deviation of the raw
//! samples, the paired relative delta `(candidate - reference) / reference` for
//! each of the runs, the noise as the median absolute deviation of those
//! deltas, and the minimum detectable effect as the larger of five percent and
//! three times that noise.
//!
//! Samples come in as tab-separated rows rather than by parsing the result
//! document. Both files are written from the same in-memory values in one pass,
//! so they cannot disagree, and this way the harness carries an emitter rather
//! than an emitter and a parser.

use crate::json::Json;

pub const MDE_FLOOR: f64 = 0.05;
pub const MDE_NOISE_MULTIPLE: f64 = 3.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub run: usize,
    pub case: String,
    pub phase: String,
    pub nanos: u64,
    pub alloc_count: u64,
    pub alloc_bytes: u64,
}

pub const SAMPLE_HEADER: &str = "run\tcase\tphase\tnanos\talloc_count\talloc_bytes";

impl Sample {
    pub fn to_row(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.run, self.case, self.phase, self.nanos, self.alloc_count, self.alloc_bytes
        )
    }
}

pub fn parse_samples(text: &str) -> Result<Vec<Sample>, String> {
    let mut out = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line == SAMPLE_HEADER {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 6 {
            return Err(format!("line {}: expected 6 fields", number + 1));
        }
        let number_at = |i: usize| -> Result<u64, String> {
            fields[i]
                .parse::<u64>()
                .map_err(|e| format!("line {}: field {i}: {e}", number + 1))
        };
        out.push(Sample {
            run: number_at(0)? as usize,
            case: fields[1].to_string(),
            phase: fields[2].to_string(),
            nanos: number_at(3)?,
            alloc_count: number_at(4)?,
            alloc_bytes: number_at(5)?,
        });
    }
    if out.is_empty() {
        return Err("no samples".to_string());
    }
    Ok(out)
}

/// Median with the even-length case averaged, which is the definition the
/// deviation below assumes.
pub fn median(values: &[f64]) -> f64 {
    assert!(!values.is_empty(), "median of nothing is undefined");
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a timing sample"));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// Median absolute deviation, unscaled. Not multiplied by 1.4826: the contract
/// compares it against a five percent floor and multiplies it by three itself,
/// and folding in a normal-consistency constant would move a threshold the
/// contract states in plain numbers.
pub fn median_absolute_deviation(values: &[f64]) -> f64 {
    let centre = median(values);
    let spread: Vec<f64> = values.iter().map(|v| (v - centre).abs()).collect();
    median(&spread)
}

pub fn minimum_detectable_effect(noise: f64) -> f64 {
    MDE_FLOOR.max(MDE_NOISE_MULTIPLE * noise)
}

struct Series {
    case: String,
    phase: String,
    reference: Vec<f64>,
    candidate: Vec<f64>,
}

/// Pairs the two sides by run index. A run present on one side only is dropped
/// and counted, because an unpaired delta has no denominator from the same
/// interleaved position and quietly using a neighbouring run would hide exactly
/// the drift the pairing exists to expose.
fn pair(reference: &[Sample], candidate: &[Sample]) -> (Vec<Series>, usize) {
    let mut keys: Vec<(String, String)> = reference
        .iter()
        .map(|s| (s.case.clone(), s.phase.clone()))
        .collect();
    keys.sort();
    keys.dedup();

    let mut series = Vec::new();
    let mut unpaired = 0usize;
    for (case, phase) in keys {
        let pick = |samples: &[Sample]| -> Vec<(usize, f64)> {
            let mut rows: Vec<(usize, f64)> = samples
                .iter()
                .filter(|s| s.case == case && s.phase == phase)
                .map(|s| (s.run, s.nanos as f64))
                .collect();
            rows.sort_by_key(|(run, _)| *run);
            rows
        };
        let left = pick(reference);
        let right = pick(candidate);

        let mut reference_values = Vec::new();
        let mut candidate_values = Vec::new();
        let mut right_index = 0usize;
        for (run, value) in &left {
            while right_index < right.len() && right[right_index].0 < *run {
                right_index += 1;
                unpaired += 1;
            }
            if right_index < right.len() && right[right_index].0 == *run {
                reference_values.push(*value);
                candidate_values.push(right[right_index].1);
                right_index += 1;
            } else {
                unpaired += 1;
            }
        }
        unpaired += right.len() - right_index;

        if !reference_values.is_empty() {
            series.push(Series {
                case,
                phase,
                reference: reference_values,
                candidate: candidate_values,
            });
        }
    }
    (series, unpaired)
}

fn summarise(series: &Series) -> Json {
    let deltas: Vec<f64> = series
        .reference
        .iter()
        .zip(&series.candidate)
        .map(|(reference, candidate)| {
            if *reference == 0.0 {
                0.0
            } else {
                (candidate - reference) / reference
            }
        })
        .collect();
    let noise = median_absolute_deviation(&deltas);
    let mde = minimum_detectable_effect(noise);
    let delta = median(&deltas);

    Json::o(vec![
        ("case", Json::s(series.case.clone())),
        ("phase", Json::s(series.phase.clone())),
        ("pairs", Json::U(deltas.len() as u64)),
        ("reference_median_nanos", Json::F(median(&series.reference))),
        (
            "reference_mad_nanos",
            Json::F(median_absolute_deviation(&series.reference)),
        ),
        ("candidate_median_nanos", Json::F(median(&series.candidate))),
        (
            "candidate_mad_nanos",
            Json::F(median_absolute_deviation(&series.candidate)),
        ),
        ("median_paired_delta", Json::F(delta)),
        ("noise_mad_of_deltas", Json::F(noise)),
        ("mde", Json::F(mde)),
        ("clears_mde", Json::Bool(delta.abs() >= mde)),
        (
            "direction",
            Json::s(if delta < 0.0 { "faster" } else { "slower" }),
        ),
    ])
}

pub fn analyse(reference: &[Sample], candidate: &[Sample]) -> Json {
    let (series, unpaired) = pair(reference, candidate);

    // Per phase, across every case: the same arithmetic over the pooled deltas.
    let mut phases: Vec<String> = series.iter().map(|s| s.phase.clone()).collect();
    phases.sort();
    phases.dedup();
    let phase_rows: Vec<Json> = phases
        .iter()
        .map(|phase| {
            let mut reference_values = Vec::new();
            let mut candidate_values = Vec::new();
            for entry in series.iter().filter(|s| &s.phase == phase) {
                reference_values.extend_from_slice(&entry.reference);
                candidate_values.extend_from_slice(&entry.candidate);
            }
            summarise(&Series {
                case: "all".to_string(),
                phase: phase.clone(),
                reference: reference_values,
                candidate: candidate_values,
            })
        })
        .collect();

    let worst = series
        .iter()
        .map(|entry| {
            let deltas: Vec<f64> = entry
                .reference
                .iter()
                .zip(&entry.candidate)
                .map(|(r, c)| if *r == 0.0 { 0.0 } else { (c - r) / r })
                .collect();
            (entry.case.clone(), entry.phase.clone(), median(&deltas))
        })
        .max_by(|a, b| a.2.partial_cmp(&b.2).expect("no NaN"));

    Json::o(vec![
        ("schema", Json::s("flutterdec-bench/analysis/1")),
        ("mde_floor", Json::F(MDE_FLOOR)),
        ("mde_noise_multiple", Json::F(MDE_NOISE_MULTIPLE)),
        ("series_count", Json::U(series.len() as u64)),
        ("unpaired_samples", Json::U(unpaired as u64)),
        ("by_phase", Json::A(phase_rows)),
        (
            "by_case_and_phase",
            Json::A(series.iter().map(summarise).collect()),
        ),
        (
            "worst_regression",
            match worst {
                Some((case, phase, delta)) => Json::o(vec![
                    ("case", Json::s(case)),
                    ("phase", Json::s(phase)),
                    ("median_paired_delta", Json::F(delta)),
                ]),
                None => Json::Null,
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(run: usize, case: &str, phase: &str, nanos: u64) -> Sample {
        Sample {
            run,
            case: case.to_string(),
            phase: phase.to_string(),
            nanos,
            alloc_count: 0,
            alloc_bytes: 0,
        }
    }

    #[test]
    fn median_handles_both_parities() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&[7.0]), 7.0);
    }

    /// Worked by hand: the deviations from the median 3 are 2, 1, 0, 1, 2, whose
    /// median is 1.
    #[test]
    fn mad_is_the_median_of_the_absolute_deviations() {
        assert_eq!(median_absolute_deviation(&[1.0, 2.0, 3.0, 4.0, 5.0]), 1.0);
        assert_eq!(median_absolute_deviation(&[5.0, 5.0, 5.0]), 0.0);
    }

    /// The floor binds when the measurement is quiet, and the noise multiple
    /// binds when it is not. Getting this backwards would let a candidate claim
    /// a win inside its own noise band.
    #[test]
    fn mde_is_the_larger_of_the_floor_and_three_times_noise() {
        assert_eq!(minimum_detectable_effect(0.0), 0.05);
        assert_eq!(minimum_detectable_effect(0.01), 0.05);
        assert!((minimum_detectable_effect(0.04) - 0.12).abs() < 1e-12);
    }

    /// A ten percent slowdown, injected exactly, has to come back as a ten
    /// percent median paired delta that clears the floor.
    #[test]
    fn a_known_injected_delta_is_recovered() {
        let reference: Vec<Sample> = (0..15)
            .map(|run| sample(run, "linear/64/base", "ir", 1000))
            .collect();
        let candidate: Vec<Sample> = (0..15)
            .map(|run| sample(run, "linear/64/base", "ir", 1100))
            .collect();
        let rendered = analyse(&reference, &candidate).to_pretty();
        assert!(
            rendered.contains("\"median_paired_delta\": 0.100000"),
            "{rendered}"
        );
        assert!(
            rendered.contains("\"noise_mad_of_deltas\": 0.000000"),
            "{rendered}"
        );
        assert!(rendered.contains("\"mde\": 0.050000"), "{rendered}");
        assert!(rendered.contains("\"clears_mde\": true"), "{rendered}");
        assert!(rendered.contains("\"unpaired_samples\": 0"), "{rendered}");
    }

    /// A change smaller than the floor must not be reported as detected, even
    /// when it is perfectly consistent across all fifteen pairs.
    #[test]
    fn a_change_inside_the_floor_does_not_clear_the_mde() {
        let reference: Vec<Sample> = (0..15).map(|run| sample(run, "c", "cfg", 1000)).collect();
        let candidate: Vec<Sample> = (0..15).map(|run| sample(run, "c", "cfg", 980)).collect();
        let rendered = analyse(&reference, &candidate).to_pretty();
        assert!(rendered.contains("\"clears_mde\": false"), "{rendered}");
    }

    /// A run index present on one side only is dropped and counted, never
    /// silently matched against its neighbour.
    #[test]
    fn unpaired_runs_are_reported_not_absorbed() {
        let reference: Vec<Sample> = (0..15).map(|run| sample(run, "c", "ir", 1000)).collect();
        let mut candidate: Vec<Sample> = (0..15).map(|run| sample(run, "c", "ir", 1000)).collect();
        candidate.retain(|s| s.run != 7);
        let rendered = analyse(&reference, &candidate).to_pretty();
        assert!(rendered.contains("\"pairs\": 14"), "{rendered}");
        assert!(rendered.contains("\"unpaired_samples\": 1"), "{rendered}");
    }

    #[test]
    fn sample_rows_round_trip() {
        let rows = vec![
            sample(0, "linear/64/base", "ir", 12),
            sample(1, "no-exit/1024/base", "serialization", 34),
        ];
        let text = format!(
            "{SAMPLE_HEADER}\n{}\n{}\n",
            rows[0].to_row(),
            rows[1].to_row()
        );
        assert_eq!(parse_samples(&text).expect("parses"), rows);
        assert!(parse_samples("run\tcase\n").is_err());
        assert!(parse_samples("").is_err());
    }
}

//! Span timing, allocation counting, resource limits and host identity.
//!
//! The four phase spans are disjoint by construction. Region analysis runs
//! inside emission, so the emitter cannot be timed as a whole without
//! overlapping the CFG span; instead the decompiler charges its region-analysis
//! time to a per-thread counter under the `bench-spans` feature and the
//! emission-exclusive span is what remains after subtracting it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

// Allocation counters, per thread and without synchronisation.
//
// The contract asks for single-threaded allocation counting, and that is also
// the only way to count without paying for it: an atomic read-modify-write on
// every allocation would sit inside the very spans being measured, on the
// emission path, which allocates the most.
thread_local! {
    static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
}

pub struct CountingAllocator;

// Safety: every method forwards to the system allocator unchanged and only adds
// two thread-local counter updates, which allocate nothing themselves. The
// counters use `try_with` because a thread destroying its local storage can
// still allocate, and a panic there would abort the process.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOC_COUNT.try_with(|c| c.set(c.get().wrapping_add(1)));
        let _ = ALLOC_BYTES.try_with(|c| c.set(c.get().wrapping_add(layout.size() as u64)));
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOC_COUNT.try_with(|c| c.set(c.get().wrapping_add(1)));
        let _ = ALLOC_BYTES.try_with(|c| c.set(c.get().wrapping_add(new_size as u64)));
        System.realloc(ptr, layout, new_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocations {
    pub count: u64,
    pub bytes: u64,
}

impl Allocations {
    pub fn now() -> Self {
        Self {
            count: ALLOC_COUNT.with(Cell::get),
            bytes: ALLOC_BYTES.with(Cell::get),
        }
    }

    pub fn since(self, earlier: Self) -> Self {
        Self {
            count: self.count.saturating_sub(earlier.count),
            bytes: self.bytes.saturating_sub(earlier.bytes),
        }
    }
}

/// Median cost of one `Instant::now()` pair, which is what the reconciliation
/// check has to allow for: the combined span contains the three inner span
/// boundaries, so its total is larger than the sum of its parts by roughly this
/// much per boundary.
pub fn timer_overhead_nanos(samples: usize) -> u64 {
    let mut observed = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let elapsed = started.elapsed();
        observed.push(elapsed.as_nanos() as u64);
    }
    observed.sort_unstable();
    observed[observed.len() / 2]
}

/// Peak resident set in bytes, from `VmHWM`. Absent outside Linux, in which
/// case the memory limit cannot be checked and the run says so rather than
/// claiming it passed.
pub fn peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmHWM:") else {
            continue;
        };
        let kib: u64 = rest.trim().trim_end_matches(" kB").trim().parse().ok()?;
        return Some(kib * 1024);
    }
    None
}

pub struct Host {
    pub hostname: String,
    pub kernel: String,
    pub cpu_model: String,
    pub logical_cpus: usize,
}

pub fn host() -> Host {
    let read = |path: &str| {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    };
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    Host {
        hostname: read("/proc/sys/kernel/hostname"),
        kernel: format!(
            "{} {}",
            read("/proc/sys/kernel/ostype"),
            read("/proc/sys/kernel/osrelease")
        ),
        cpu_model,
        logical_cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counters must move on a real allocation and stay put on arithmetic,
    /// otherwise the per-phase allocation numbers are noise.
    #[test]
    fn allocation_counters_track_real_allocations() {
        let before = Allocations::now();
        let held: Vec<u64> = (0..4096).collect();
        let after = Allocations::now().since(before);
        assert!(after.count >= 1, "a vector allocation was counted");
        assert!(
            after.bytes >= 4096 * 8,
            "at least the vector's bytes were counted, saw {}",
            after.bytes
        );
        drop(held);

        let quiet_before = Allocations::now();
        let mut acc = 0u64;
        for i in 0..1000u64 {
            acc = acc.wrapping_add(i);
        }
        assert_eq!(acc, 499_500);
        assert_eq!(
            Allocations::now().since(quiet_before),
            Allocations { count: 0, bytes: 0 },
            "arithmetic allocates nothing"
        );
    }

    /// A calibration that returned zero would silently disable the
    /// reconciliation allowance.
    #[test]
    fn timer_overhead_is_measured_and_bounded() {
        let overhead = timer_overhead_nanos(1000);
        assert!(overhead > 0, "a clock read is not free");
        assert!(
            overhead < 10_000,
            "a clock read is not a microsecond either"
        );
    }

    #[test]
    fn host_identity_is_readable_on_this_platform() {
        let host = host();
        assert!(!host.hostname.is_empty());
        assert!(host.logical_cpus > 0);
        assert!(peak_rss_bytes().is_some_and(|b| b > 0), "linux VmHWM");
    }
}

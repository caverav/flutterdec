//! Span timing, allocation counting, resource limits and host identity.
//!
//! The four phase spans are disjoint by construction. Region analysis runs
//! inside emission, so the emitter cannot be timed as a whole without
//! overlapping the CFG span; instead the decompiler charges its region-analysis
//! time to a per-thread counter under the `bench-spans` feature and the
//! emission-exclusive span is what remains after subtracting it.

use flutterdec_decompiler::bench_spans::{current_resource_phase, ResourcePhase};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::cmp;
use std::mem::{align_of, size_of};
use std::ptr;
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
    static RESOURCE_STATE: UnsafeCell<ResourceState> = const { UnsafeCell::new(ResourceState::new()) };
    static IN_INSTRUMENTATION: Cell<bool> = const { Cell::new(false) };
    static RECURSION_COUNT: Cell<u64> = const { Cell::new(0) };
}

const RESOURCE_PHASE_COUNT: usize = 4;
const INACTIVE_PHASE: u8 = u8::MAX;
const HEADER_MAGIC: u64 = 0x4652_4445_4352_5343;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceMetrics {
    pub allocation_count: u64,
    pub total_allocated_bytes: u64,
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub phases: [ResourceMetrics; RESOURCE_PHASE_COUNT],
    pub combined: ResourceMetrics,
    pub epoch: u64,
    pub instrumentation_recursions: u64,
}

#[derive(Clone, Copy)]
struct ResourceState {
    phases: [ResourceMetrics; RESOURCE_PHASE_COUNT],
    combined: ResourceMetrics,
    epoch: u64,
}

impl ResourceState {
    const fn new() -> Self {
        Self {
            phases: [ResourceMetrics {
                allocation_count: 0,
                total_allocated_bytes: 0,
                live_bytes: 0,
                peak_live_bytes: 0,
            }; RESOURCE_PHASE_COUNT],
            combined: ResourceMetrics {
                allocation_count: 0,
                total_allocated_bytes: 0,
                live_bytes: 0,
                peak_live_bytes: 0,
            },
            epoch: 1,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AllocationHeader {
    magic: u64,
    epoch: u64,
    size: usize,
    phase: u8,
}

fn phase_index(phase: ResourcePhase) -> usize {
    phase as usize
}

fn augmented_layout(layout: Layout, size: usize) -> (Layout, usize) {
    let alignment = cmp::max(layout.align(), align_of::<AllocationHeader>());
    let offset = size_of::<AllocationHeader>().next_multiple_of(alignment);
    let total = offset
        .checked_add(size)
        .expect("allocation layout overflow in resource ruler");
    // Safety: alignment is the maximum of two valid powers of two and total is
    // checked. The caller supplied a valid Layout.
    (
        unsafe { Layout::from_size_align_unchecked(total, alignment) },
        offset,
    )
}

fn with_resource_state(f: impl FnOnce(&mut ResourceState)) {
    let entered = IN_INSTRUMENTATION.try_with(|flag| flag.replace(true));
    let Ok(was_inside) = entered else {
        return;
    };
    if was_inside {
        let _ = RECURSION_COUNT.try_with(|count| count.set(count.get().saturating_add(1)));
        return;
    }
    let _ = RESOURCE_STATE.try_with(|state| {
        // Safety: this thread-local cell is only reached while the reentrancy
        // flag is held, so no two mutable references can coexist.
        f(unsafe { &mut *state.get() });
    });
    let _ = IN_INSTRUMENTATION.try_with(|flag| flag.set(false));
}

fn record_alloc(header: AllocationHeader) {
    if header.phase == INACTIVE_PHASE {
        return;
    }
    with_resource_state(|state| {
        if header.epoch != state.epoch {
            return;
        }
        let phase = &mut state.phases[header.phase as usize];
        phase.allocation_count = phase.allocation_count.saturating_add(1);
        phase.total_allocated_bytes = phase
            .total_allocated_bytes
            .saturating_add(header.size as u64);
        phase.live_bytes = phase.live_bytes.saturating_add(header.size as u64);
        phase.peak_live_bytes = phase.peak_live_bytes.max(phase.live_bytes);
        state.combined.allocation_count = state.combined.allocation_count.saturating_add(1);
        state.combined.total_allocated_bytes = state
            .combined
            .total_allocated_bytes
            .saturating_add(header.size as u64);
        state.combined.live_bytes = state.combined.live_bytes.saturating_add(header.size as u64);
        state.combined.peak_live_bytes = state
            .combined
            .peak_live_bytes
            .max(state.combined.live_bytes);
    });
}

fn record_dealloc(header: AllocationHeader) {
    if header.phase == INACTIVE_PHASE {
        return;
    }
    with_resource_state(|state| {
        if header.epoch != state.epoch {
            return;
        }
        let phase = &mut state.phases[header.phase as usize];
        phase.live_bytes = phase.live_bytes.saturating_sub(header.size as u64);
        state.combined.live_bytes = state.combined.live_bytes.saturating_sub(header.size as u64);
    });
}

fn header_for(size: usize) -> AllocationHeader {
    let phase = current_resource_phase()
        .map(|phase| phase_index(phase) as u8)
        .unwrap_or(INACTIVE_PHASE);
    let epoch = RESOURCE_STATE
        .try_with(|state| {
            // Safety: a plain Copy read of thread-local state; the allocator is
            // single-threaded by construction.
            unsafe { (*state.get()).epoch }
        })
        .unwrap_or(0);
    AllocationHeader {
        magic: HEADER_MAGIC,
        epoch,
        size,
        phase,
    }
}

pub fn reset_resources() {
    with_resource_state(|state| {
        state.epoch = state.epoch.wrapping_add(1).max(1);
        state.phases = [ResourceMetrics::default(); RESOURCE_PHASE_COUNT];
        state.combined = ResourceMetrics::default();
    });
    RECURSION_COUNT.with(|count| count.set(0));
}

pub fn resource_snapshot() -> ResourceSnapshot {
    let (phases, combined, epoch) = RESOURCE_STATE.with(|state| {
        // Safety: the snapshot is taken outside allocator instrumentation.
        let state = unsafe { *state.get() };
        (state.phases, state.combined, state.epoch)
    });
    ResourceSnapshot {
        phases,
        combined,
        epoch,
        instrumentation_recursions: RECURSION_COUNT.with(Cell::get),
    }
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
        let (system_layout, offset) = augmented_layout(layout, layout.size());
        let base = System.alloc(system_layout);
        if base.is_null() {
            return base;
        }
        let header = header_for(layout.size());
        ptr::write(base.cast::<AllocationHeader>(), header);
        record_alloc(header);
        base.add(offset)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOC_COUNT.try_with(|c| c.set(c.get().wrapping_add(1)));
        let _ = ALLOC_BYTES.try_with(|c| c.set(c.get().wrapping_add(layout.size() as u64)));
        let (system_layout, offset) = augmented_layout(layout, layout.size());
        let base = System.alloc_zeroed(system_layout);
        if base.is_null() {
            return base;
        }
        let header = header_for(layout.size());
        ptr::write(base.cast::<AllocationHeader>(), header);
        record_alloc(header);
        base.add(offset)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (system_layout, offset) = augmented_layout(layout, layout.size());
        let base = ptr.sub(offset);
        let header = ptr::read(base.cast::<AllocationHeader>());
        debug_assert_eq!(header.magic, HEADER_MAGIC);
        record_dealloc(header);
        System.dealloc(base, system_layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOC_COUNT.try_with(|c| c.set(c.get().wrapping_add(1)));
        let _ = ALLOC_BYTES.try_with(|c| c.set(c.get().wrapping_add(new_size as u64)));
        let (old_system_layout, old_offset) = augmented_layout(layout, layout.size());
        let (new_system_layout, new_offset) = augmented_layout(layout, new_size);
        debug_assert_eq!(old_system_layout.align(), new_system_layout.align());
        debug_assert_eq!(old_offset, new_offset);
        let old_base = ptr.sub(old_offset);
        let old_header = ptr::read(old_base.cast::<AllocationHeader>());
        debug_assert_eq!(old_header.magic, HEADER_MAGIC);
        let new_base = System.realloc(old_base, old_system_layout, new_system_layout.size());
        if new_base.is_null() {
            ptr::write(old_base.cast::<AllocationHeader>(), old_header);
            return new_base;
        }
        record_dealloc(old_header);
        let new_header = header_for(new_size);
        ptr::write(new_base.cast::<AllocationHeader>(), new_header);
        record_alloc(new_header);
        new_base.add(new_offset)
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
    use flutterdec_decompiler::bench_spans::{enter_resource_phase, ResourcePhase};

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

    #[test]
    fn resource_allocator_covers_full_lifecycle_without_misattribution() {
        unsafe {
            let layout = Layout::from_size_align(64, 16).unwrap();
            reset_resources();
            let ir = enter_resource_phase(ResourcePhase::Ir);
            let first = CountingAllocator.alloc(layout);
            assert!(!first.is_null());
            drop(ir);

            let cfg = enter_resource_phase(ResourcePhase::Cfg);
            CountingAllocator.dealloc(first, layout);
            let zeroed = CountingAllocator.alloc_zeroed(layout);
            assert!(!zeroed.is_null());
            assert!((0..64).all(|index| *zeroed.add(index) == 0));
            let grown = CountingAllocator.realloc(zeroed, layout, 160);
            assert!(!grown.is_null());
            drop(cfg);

            let snapshot = resource_snapshot();
            assert_eq!(
                snapshot.phases[ResourcePhase::Ir as usize].allocation_count,
                1
            );
            assert_eq!(
                snapshot.phases[ResourcePhase::Ir as usize].total_allocated_bytes,
                64
            );
            assert_eq!(snapshot.phases[ResourcePhase::Ir as usize].live_bytes, 0);
            assert_eq!(
                snapshot.phases[ResourcePhase::Ir as usize].peak_live_bytes,
                64
            );
            assert_eq!(
                snapshot.phases[ResourcePhase::Cfg as usize].allocation_count,
                2
            );
            assert_eq!(
                snapshot.phases[ResourcePhase::Cfg as usize].total_allocated_bytes,
                224
            );
            assert_eq!(snapshot.phases[ResourcePhase::Cfg as usize].live_bytes, 160);
            assert_eq!(
                snapshot.phases[ResourcePhase::Cfg as usize].peak_live_bytes,
                160
            );
            assert_eq!(snapshot.combined.allocation_count, 3);
            assert_eq!(snapshot.combined.total_allocated_bytes, 288);
            assert_eq!(snapshot.combined.peak_live_bytes, 160);
            assert_eq!(snapshot.instrumentation_recursions, 0);
            CountingAllocator.dealloc(grown, Layout::from_size_align(160, 16).unwrap());
        }
    }

    #[test]
    fn reset_ignores_old_lifetimes_and_thread_state_is_private() {
        unsafe {
            let layout = Layout::from_size_align(32, 8).unwrap();
            reset_resources();
            let phase = enter_resource_phase(ResourcePhase::Serialization);
            let old = CountingAllocator.alloc(layout);
            drop(phase);
            reset_resources();
            CountingAllocator.dealloc(old, layout);
            assert_eq!(resource_snapshot().combined, ResourceMetrics::default());

            let child = std::thread::spawn(|| {
                reset_resources();
                let phase = enter_resource_phase(ResourcePhase::EmissionExclusive);
                let layout = Layout::from_size_align(48, 8).unwrap();
                let ptr = CountingAllocator.alloc(layout);
                drop(phase);
                let snapshot = resource_snapshot();
                CountingAllocator.dealloc(ptr, layout);
                snapshot
            })
            .join()
            .unwrap();
            assert_eq!(child.combined.allocation_count, 1);
            assert_eq!(child.combined.total_allocated_bytes, 48);
            assert_eq!(resource_snapshot().combined, ResourceMetrics::default());
        }
    }

    #[test]
    fn nested_phase_exit_and_panic_cleanup_restore_the_parent() {
        use flutterdec_decompiler::bench_spans::current_resource_phase;

        assert_eq!(current_resource_phase(), None);
        let outer = enter_resource_phase(ResourcePhase::EmissionExclusive);
        assert_eq!(
            current_resource_phase(),
            Some(ResourcePhase::EmissionExclusive)
        );
        let panicked = std::panic::catch_unwind(|| {
            let _inner = enter_resource_phase(ResourcePhase::Cfg);
            assert_eq!(current_resource_phase(), Some(ResourcePhase::Cfg));
            panic!("phase cleanup plant");
        });
        assert!(panicked.is_err());
        assert_eq!(
            current_resource_phase(),
            Some(ResourcePhase::EmissionExclusive)
        );
        drop(outer);
        assert_eq!(current_resource_phase(), None);
    }
}

//! Which processors our own threads run on, where a processor has two kinds
//! of cores, or several last-level caches of one size (`VICTORY_CCD`, below).
//! `cpu_sets` and `keep_on_fast_cores` are fxceless/victory-fork's
//! (`framelog.rs`, commit 0b0074e0, GPL-3.0-or-later, as this).

/// The system's processor sets of group 0: each one's id and its efficiency
/// class (higher is faster).
fn cpu_sets() -> Vec<(u32, u8)> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut bytes = 0u32;
    // SAFETY: a size query, then the call into a buffer of that size.
    unsafe { GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut bytes, GetCurrentProcess(), 0) };
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8).max(1)];
    // SAFETY: as above; the buffer holds `bytes` bytes, aligned for the records.
    let ok = unsafe { GetSystemCpuSetInformation(buffer.as_mut_ptr().cast(), bytes, &mut bytes, GetCurrentProcess(), 0) };
    let mut sets = Vec::new();
    let mut at = 0usize;
    while ok != 0 && at + size_of::<SYSTEM_CPU_SET_INFORMATION>() <= bytes as usize {
        // SAFETY: a record inside the buffer; each says its own size.
        let info = unsafe { &*(buffer.as_ptr().cast::<u8>().add(at).cast::<SYSTEM_CPU_SET_INFORMATION>()) };
        if info.Size == 0 {
            break;
        }
        // SAFETY: the record's type is CpuSet (the only one there is).
        let set = unsafe { &info.Anonymous.CpuSet };
        if set.Group == 0 {
            sets.push((set.Id, set.EfficiencyClass));
        }
        at += info.Size as usize;
    }
    sets
}

/// The sets of the fastest class, if there are slower ones too.
fn fast_of(sets: &[(u32, u8)]) -> Vec<u32> {
    let best = sets.iter().map(|c| c.1).max().unwrap_or(0);
    let fast: Vec<u32> = sets.iter().filter(|c| c.1 == best).map(|c| c.0).collect();
    if fast.len() == sets.len() { Vec::new() } else { fast }
}

/// `VICTORY_FAST_CORES`: 0: no thread asks; 1: the threads a frame waits for
/// one by one; 2 (the default): the threads they share their work with too;
/// 3: those that work beside the rest as well (lists translated, descriptors
/// made): more threads by then than the fast cores run at a time, while the
/// slow ones stood idle.
fn stage() -> u32 {
    static STAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *STAGE.get_or_init(|| std::env::var("VICTORY_FAST_CORES").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(2))
}

/// Asks the system to run the calling thread, which a frame waits for, on
/// the fast cores only, where the processor has two kinds. That fork's submit
/// thread was found on a slow one in 1,529 looks of 24,021. (A selection of
/// processors is a preference the scheduler keeps unless nothing of it is
/// free.) The title's threads are not asked for.
pub(crate) fn keep_on_fast_cores(what: &str) {
    ask(what, 1);
}

/// [`keep_on_fast_cores`] for one of the threads a submission's preparing or
/// a landing shares its work with.
pub(crate) fn keep_helper_on_fast_cores(what: &str) {
    ask(what, 2);
}

/// [`keep_on_fast_cores`] for one of many threads whose work is ahead of what
/// a frame waits for.
pub(crate) fn keep_many_on_fast_cores(what: &str) {
    ask(what, 3);
}

fn ask(what: &str, from: u32) {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadSelectedCpuSets};
    static FAST: std::sync::OnceLock<(Vec<u32>, usize)> = std::sync::OnceLock::new();
    let (fast, of) = FAST.get_or_init(|| {
        let sets = cpu_sets();
        (fast_of(&sets), sets.len())
    });
    // All cores of one kind: where VICTORY_CCD may place threads, every one
    // that asks registers, whatever VICTORY_FAST_CORES says (that switch is
    // for processors with two kinds of cores).
    if fast.is_empty() {
        if ccd_possible() {
            register(what, from);
        }
        return;
    }
    if stage() < from {
        return;
    }
    // SAFETY: our own thread and a list of the system's own set ids.
    let ok = unsafe { SetThreadSelectedCpuSets(GetCurrentThread(), fast.as_ptr(), fast.len() as u32) };
    if ok == 0 {
        tracing::warn!(what, error = %std::io::Error::last_os_error(), "the thread could not be kept on the fast cores");
    } else {
        tracing::info!(what, fast = fast.len(), of, "the thread is kept on the fast cores (VICTORY_FAST_CORES=0: not)");
    }
}

// `VICTORY_CCD`: on a processor with two or more last-level caches of one
// size (a Ryzen with two core complexes, or the four-core groups of the 3000
// series), our own threads go to the caches without processor 0, where the
// title pins its own threads, so that those keep their cores to themselves.
// Adapted from the patch motioncsgo posted in avatarmanz/VIctory-7900XTX
// issue 399, to this file's simpler placement (no X3D or lane-core rules
// here; `VICTORY_LANE_CORE` from the same patch is not taken).

/// Each set of group 0: its id, the lowest logical processor under its
/// last-level cache (the domain; the one with processor 0 is 0) and that
/// cache's size.
fn l3_domain_of_sets() -> Vec<(u32, u32, u32)> {
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, GetSystemCpuSetInformation, RelationCache, SYSTEM_CPU_SET_INFORMATION,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut by_processor = [(0u32, 0usize); 64];
    let mut bytes = 0u32;
    // SAFETY: a size query, then the call into a buffer of that size.
    unsafe { GetLogicalProcessorInformationEx(RelationCache, std::ptr::null_mut(), &mut bytes) };
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8).max(1)];
    // SAFETY: as above; the buffer holds `bytes` bytes, aligned for the records.
    let ok = unsafe { GetLogicalProcessorInformationEx(RelationCache, buffer.as_mut_ptr().cast(), &mut bytes) };
    let mut at = 0usize;
    while ok != 0 && at + size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>() <= bytes as usize {
        // SAFETY: a record inside the buffer; each says its own size.
        let info = unsafe { &*(buffer.as_ptr().cast::<u8>().add(at).cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()) };
        if info.Size == 0 {
            break;
        }
        if info.Relationship == RelationCache {
            // SAFETY: a cache record (asked for), with at least its first group mask.
            let cache = unsafe { &info.Anonymous.Cache };
            let mask = unsafe { cache.Anonymous.GroupMask };
            if mask.Group == 0 {
                for (p, slot) in by_processor.iter_mut().enumerate() {
                    if mask.Mask >> p & 1 != 0 && cache.CacheSize > slot.0 {
                        *slot = (cache.CacheSize, mask.Mask);
                    }
                }
            }
        }
        at += info.Size as usize;
    }
    let mut bytes = 0u32;
    // SAFETY: as for `cpu_sets`.
    unsafe { GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut bytes, GetCurrentProcess(), 0) };
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8).max(1)];
    // SAFETY: as above.
    let ok = unsafe { GetSystemCpuSetInformation(buffer.as_mut_ptr().cast(), bytes, &mut bytes, GetCurrentProcess(), 0) };
    let mut domains = Vec::new();
    let mut at = 0usize;
    while ok != 0 && at + size_of::<SYSTEM_CPU_SET_INFORMATION>() <= bytes as usize {
        // SAFETY: a record inside the buffer; each says its own size.
        let info = unsafe { &*(buffer.as_ptr().cast::<u8>().add(at).cast::<SYSTEM_CPU_SET_INFORMATION>()) };
        if info.Size == 0 {
            break;
        }
        // SAFETY: the record's type is CpuSet (the only one there is).
        let set = unsafe { &info.Anonymous.CpuSet };
        if set.Group == 0
            && let Some(&(size, mask)) = by_processor.get(set.LogicalProcessorIndex as usize)
            && size != 0
            && mask != 0
        {
            domains.push((set.Id, mask.trailing_zeros(), size));
        }
        at += info.Size as usize;
    }
    domains
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ccd {
    Off,
    Near,
    Far,
}

fn ccd_from(text: Option<String>) -> Ccd {
    match text.as_deref().map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("far") | Some("1") => Ccd::Far,
        Some("near") => Ccd::Near,
        _ => Ccd::Off,
    }
}

fn ccd_static() -> Ccd {
    static MODE: std::sync::OnceLock<Ccd> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| ccd_from(std::env::var("VICTORY_CCD").ok()))
}

/// `VICTORY_CCD` now: `far` (or `1`): all registered threads off the title's
/// cache; `near`: only the many that translate lists (stage 3) off it, the
/// rest beside the title; unset or `0`: no selection, as before. With
/// `VICTORY_AB=<s>,VICTORY_CCD` (diagnostics) it changes while the title
/// runs; `VICTORY_CCD_MODE=near` then picks near for the on phases.
fn ccd() -> Ccd {
    #[cfg(feature = "diagnostics")]
    if let Some(on) = crate::ab::pick("VICTORY_CCD") {
        if !on {
            return Ccd::Off;
        }
        return match ccd_from(std::env::var("VICTORY_CCD_MODE").ok()) {
            Ccd::Near => Ccd::Near,
            _ => Ccd::Far,
        };
    }
    ccd_static()
}

fn ccd_possible() -> bool {
    #[cfg(feature = "diagnostics")]
    if crate::ab::bit("VICTORY_CCD").is_some() {
        return true;
    }
    ccd_static() != Ccd::Off
}

/// The sets of the cache with processor 0 and those of the others, where
/// there are two or more caches, all of one size. `None` otherwise.
fn ccd_split(domains: &[(u32, u32, u32)]) -> Option<(Vec<u32>, Vec<u32>)> {
    let size = domains.first()?.2;
    if domains.iter().any(|d| d.2 != size) {
        return None;
    }
    let home = domains.iter().map(|d| d.1).min()?;
    let near: Vec<u32> = domains.iter().filter(|d| d.1 == home).map(|d| d.0).collect();
    let far: Vec<u32> = domains.iter().filter(|d| d.1 != home).map(|d| d.0).collect();
    (!far.is_empty()).then_some((near, far))
}

fn split() -> Option<&'static (Vec<u32>, Vec<u32>)> {
    static SPLIT: std::sync::OnceLock<Option<(Vec<u32>, Vec<u32>)>> = std::sync::OnceLock::new();
    SPLIT
        .get_or_init(|| {
            let domains = l3_domain_of_sets();
            let split = ccd_split(&domains);
            tracing::info!(
                sets = domains.len(),
                near = split.as_ref().map_or(0, |s| s.0.len()),
                far = split.as_ref().map_or(0, |s| s.1.len()),
                "VICTORY_CCD: caches of the processor"
            );
            split
        })
        .as_ref()
}

/// Threads registered for VICTORY_CCD: a handle and the stage.
static ASKED: std::sync::Mutex<Vec<(usize, u32)>> = std::sync::Mutex::new(Vec::new());

fn register(what: &str, from: u32) {
    use windows_sys::Win32::System::Threading::{GetCurrentThreadId, OpenThread, THREAD_SET_LIMITED_INFORMATION};
    if split().is_none() {
        return;
    }
    // SAFETY: our own thread's id; the handle is kept for the run.
    let thread = unsafe { OpenThread(THREAD_SET_LIMITED_INFORMATION, 0, GetCurrentThreadId()) };
    if thread.is_null() {
        return;
    }
    ASKED.lock().unwrap_or_else(|e| e.into_inner()).push((thread as usize, from));
    settle();
    tracing::info!(what, from, "VICTORY_CCD: thread registered");
}

/// Tells every registered thread where it goes now.
pub(crate) fn settle() {
    use windows_sys::Win32::System::Threading::SetThreadSelectedCpuSets;
    let Some((near, far)) = split() else {
        return;
    };
    let mode = ccd();
    let asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    for &(thread, from) in asked.iter() {
        let sets: &[u32] = match mode {
            Ccd::Off => &[],
            Ccd::Far => far,
            Ccd::Near if from == 3 => far,
            Ccd::Near => near,
        };
        let list = if sets.is_empty() { std::ptr::null() } else { sets.as_ptr() };
        // SAFETY: a handle kept open, and the system's own set ids (none: clear).
        unsafe { SetThreadSelectedCpuSets(thread as _, list, sets.len() as u32) };
    }
    tracing::debug!(mode = ?mode, threads = asked.len(), near = near.len(), far = far.len(), "VICTORY_CCD: our threads placed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fast_cores_are_those_of_the_best_class_if_there_is_a_worse_one() {
        assert_eq!(fast_of(&[(256, 1), (257, 1), (258, 0)]), [256, 257]);
        assert!(fast_of(&[(256, 0), (257, 0)]).is_empty());
        assert!(fast_of(&[]).is_empty());
        assert!(!cpu_sets().is_empty());
        std::thread::spawn(|| keep_on_fast_cores("test")).join().unwrap();
    }

    /// Sets from id 256, `per` to each cache, all caches of `size`.
    fn caches(n: u32, per: u32, size: u32) -> Vec<(u32, u32, u32)> {
        (0..n).map(|i| (256 + i, i / per * per, size)).collect()
    }

    #[test]
    fn several_caches_alike_split_into_the_one_with_processor_0_and_the_rest() {
        const MB: u32 = 1 << 20;
        // Two complexes of twelve (5900X): the second's sets are the far ones.
        let (near, far) = ccd_split(&caches(24, 12, 32 * MB)).unwrap();
        assert_eq!(near, (256..268).collect::<Vec<_>>());
        assert_eq!(far, (268..280).collect::<Vec<_>>());
        // Two of sixteen (5950X, 7950X, 9950X).
        let (near, far) = ccd_split(&caches(32, 16, 32 * MB)).unwrap();
        assert_eq!((near.len(), far.len()), (16, 16));
        // Zen 2 (3950X): four caches of eight sets, one of them with processor 0.
        let (near, far) = ccd_split(&caches(32, 8, 16 * MB)).unwrap();
        assert_eq!((near.len(), far.len()), (8, 24));
        // One cache (9800X3D, any single complex): nothing to split.
        assert!(ccd_split(&caches(16, 16, 96 * MB)).is_none());
        // The stacked cache on one complex only (9950X3D): sizes differ, nothing.
        let x3d: Vec<(u32, u32, u32)> =
            (0..32).map(|i| (256 + i, if i < 16 { 0 } else { 16 }, if i < 16 { 96 * MB } else { 32 * MB })).collect();
        assert!(ccd_split(&x3d).is_none());
        // Nothing known: nothing.
        assert!(ccd_split(&[]).is_none());
    }

    #[test]
    fn the_mode_reads_far_near_or_off() {
        assert_eq!(ccd_from(Some("far".into())), Ccd::Far);
        assert_eq!(ccd_from(Some(" FAR ".into())), Ccd::Far);
        assert_eq!(ccd_from(Some("1".into())), Ccd::Far);
        assert_eq!(ccd_from(Some("near".into())), Ccd::Near);
        assert_eq!(ccd_from(Some("0".into())), Ccd::Off);
        assert_eq!(ccd_from(Some("other".into())), Ccd::Off);
        assert_eq!(ccd_from(None), Ccd::Off);
    }

    #[test]
    fn this_machine_s_caches_are_read() {
        let here = l3_domain_of_sets();
        assert!(here.len() <= cpu_sets().len());
        assert!(here.iter().all(|d| d.2 > 0));
        eprintln!("this machine: {} sets with a cache, split {:?}", here.len(), ccd_split(&here).map(|s| (s.0.len(), s.1.len())));
    }
}

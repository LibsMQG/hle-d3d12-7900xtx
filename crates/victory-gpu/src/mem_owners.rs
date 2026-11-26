//! `VICTORY_MEM_OWNERS=<csv path>` (off by default): who owns the
//! write-combined memory. Counting only: nothing allocated changes.
//!
//! A memory map of the game in the jail (`tools/sampler` `ramap`) showed
//! about 15 GB of write-combined commit, 10.9 GB of it in some 500 blocks of
//! exactly 32 MB, with no known owner. Write-combined memory is what upload
//! heaps (and some of the driver's own memory) live in. With the switch:
//!
//! - every upload buffer VIctory makes (`Host::upload_buffer`) is written to
//!   the CSV with its address, size, the code that asked for it and the
//!   memory block (allocation) Windows says it lies in;
//! - every 30 s a thread of its own walks the process's address space
//!   (`VirtualQuery` only) and says, per size, how many write-combined
//!   blocks there are and how much is committed, and of the 32 MB ones how
//!   many hold an upload buffer of ours (by code site), next to the host
//!   lists alive (each holds a command allocator, whose memory the driver
//!   owns and no counter of ours sees).
//!
//! What it cannot see: a block freed and made again at the same address is
//! taken for the one recorded there before.

use std::collections::HashMap;
use std::io::Write;
use std::panic::Location;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_WRITECOMBINE, VirtualQuery};

/// How often the address space is walked.
const EVERY: Duration = Duration::from_secs(30);

fn path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("VICTORY_MEM_OWNERS").ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()))
        .as_deref()
}

#[cfg(test)]
pub(crate) static FORCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn on() -> bool {
    #[cfg(test)]
    if FORCED.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    path().is_some()
}

/// Upload buffers of ours, by the block they lie in: per code site, how many
/// and their bytes.
type Owners = HashMap<usize, HashMap<&'static Location<'static>, (u64, u64)>>;

struct State {
    owners: Owners,
    csv: Option<std::io::BufWriter<std::fs::File>>,
    started: Instant,
    walking: bool,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| {
            let csv = path().and_then(|p| match std::fs::File::create(p) {
                Ok(f) => {
                    let mut w = std::io::BufWriter::new(f);
                    let _ = writeln!(w, "seconds,kind,address,bytes,block,block_bytes,write_combined,site");
                    Some(w)
                }
                Err(e) => {
                    tracing::warn!(error = %e, path = p, "VICTORY_MEM_OWNERS: no CSV; counting in the log only");
                    None
                }
            });
            Mutex::new(State { owners: HashMap::new(), csv, started: Instant::now(), walking: false })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The block (allocation base) `at` lies in, its reserved bytes from the
/// base to the end of the region holding `at`, and whether that region is
/// write-combined. None for an address Windows knows nothing of.
fn block_of(at: usize) -> Option<(usize, usize, bool)> {
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a query of our own address space into a local structure.
    let got = unsafe { VirtualQuery(at as *const _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
    if got == 0 || info.AllocationBase.is_null() {
        return None;
    }
    let base = info.AllocationBase as usize;
    let end = info.BaseAddress as usize + info.RegionSize;
    Some((base, end - base, info.Protect & PAGE_WRITECOMBINE != 0))
}

/// `Host::upload_buffer` made `bytes` at `at` for the code at `site`.
pub(crate) fn upload(at: *const u8, bytes: u64, site: &'static Location<'static>) {
    record("upload", at, bytes, site);
}

/// `Host::shared_buffer` (system memory the CPU maps write-back and shaders
/// read and write: host copies of title memory) made `bytes` at `at` for
/// the code at `site`. Its memory is ordinary private memory to Windows.
pub(crate) fn shared(at: *const u8, bytes: u64, site: &'static Location<'static>) {
    record("shared", at, bytes, site);
}

fn record(kind: &str, at: *const u8, bytes: u64, site: &'static Location<'static>) {
    if !on() || at.is_null() {
        return;
    }
    let (block, block_bytes, wc) = block_of(at as usize).unwrap_or((0, 0, false));
    let mut s = state();
    let entry = s.owners.entry(block).or_default().entry(site).or_insert((0, 0));
    entry.0 += 1;
    entry.1 += bytes;
    let seconds = s.started.elapsed().as_secs_f64();
    if let Some(w) = s.csv.as_mut() {
        let _ = writeln!(
            w,
            "{seconds:.3},{kind},{:#x},{bytes},{block:#x},{block_bytes},{wc},{}:{}",
            at as usize,
            site.file(),
            site.line()
        );
    }
    if !s.walking {
        s.walking = true;
        drop(s);
        let _ = std::thread::Builder::new().name("victory-mem-owners".into()).spawn(|| loop {
            std::thread::sleep(EVERY);
            report();
        });
    }
}

/// A write-combined block: reserved bytes, committed bytes.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) reserved: u64,
    pub(crate) committed: u64,
}

/// The process's write-combined blocks, by base: every region with the
/// write-combine flag counted under its allocation base.
pub(crate) fn write_combined() -> HashMap<usize, Block> {
    let mut out: HashMap<usize, Block> = HashMap::new();
    let mut at = 0usize;
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: a query of our own address space into a local structure.
        let got = unsafe { VirtualQuery(at as *const _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize == 0 {
            break;
        }
        let wc = info.State == MEM_COMMIT && (info.Protect & PAGE_WRITECOMBINE != 0 || info.AllocationProtect & PAGE_WRITECOMBINE != 0);
        if wc && !info.AllocationBase.is_null() {
            let b = out.entry(info.AllocationBase as usize).or_default();
            b.committed += info.RegionSize as u64;
        }
        // Reserved bytes: every region of a block whose committed part is
        // write-combined (summed below from the bases found).
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    // The blocks' reserved size: walk each block found from its base.
    for (&base, b) in out.iter_mut() {
        let mut at = base;
        loop {
            // SAFETY: as above.
            let got = unsafe { VirtualQuery(at as *const _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
            if got == 0 || info.AllocationBase as usize != base || info.RegionSize == 0 {
                break;
            }
            b.reserved += info.RegionSize as u64;
            at = info.BaseAddress as usize + info.RegionSize;
        }
    }
    out
}

/// The log lines: write-combined blocks by size, the 32 MB ones against
/// the upload buffers recorded in them, and the host lists alive.
pub(crate) fn report() -> String {
    let walk = Instant::now();
    let blocks = write_combined();
    let took = walk.elapsed();
    let mb = |b: u64| b as f64 / (1 << 20) as f64;
    let mut by_size: HashMap<u64, (u64, u64)> = HashMap::new();
    for b in blocks.values() {
        let e = by_size.entry(b.reserved).or_insert((0, 0));
        e.0 += 1;
        e.1 += b.committed;
    }
    let mut sizes: Vec<_> = by_size.into_iter().collect();
    sizes.sort_by_key(|s| std::cmp::Reverse(s.1.1));
    let total: u64 = blocks.values().map(|b| b.committed).sum();
    let sizes_text: Vec<String> =
        sizes.iter().take(8).map(|(size, (n, c))| format!("{n} x {:.2} MB ({:.2} GB committed)", mb(*size), mb(*c) / 1024.0)).collect();
    // The 32 MB blocks: which hold upload buffers of ours, by site.
    let big: Vec<usize> = blocks.iter().filter(|(_, b)| b.reserved == 32 << 20).map(|(&base, _)| base).collect();
    let s = state();
    let mut ours = 0usize;
    let mut sites: HashMap<&'static Location<'static>, (u64, u64)> = HashMap::new();
    for base in &big {
        if let Some(owners) = s.owners.get(base) {
            ours += 1;
            for (site, (n, bytes)) in owners {
                let e = sites.entry(site).or_insert((0, 0));
                e.0 += n;
                e.1 += bytes;
            }
        }
    }
    let mut s = s;
    if let Some(w) = s.csv.as_mut() {
        let _ = w.flush();
    }
    drop(s);
    let mut sites: Vec<_> = sites.into_iter().collect();
    sites.sort_by_key(|s| std::cmp::Reverse(s.1.1));
    let sites_text: Vec<String> =
        sites.iter().take(6).map(|(site, (n, b))| format!("{}:{} {n} buffers {:.1} MB", site.file(), site.line(), mb(*b))).collect();
    let lists_alive = crate::host::ALIVE[0].load(std::sync::atomic::Ordering::Relaxed);
    let lists_made = crate::host::LISTS[0].load(std::sync::atomic::Ordering::Relaxed);
    let preludes = crate::host::LISTS[1].load(std::sync::atomic::Ordering::Relaxed);
    let text = format!(
        "write-combined: {} blocks, {:.2} GB committed (walk {:.0} ms); by size: {}; of {} blocks of 32 MB, {} hold an upload \
         buffer of ours ({}); host lists alive {lists_alive} (made {lists_made}, preludes {preludes})",
        blocks.len(),
        mb(total) / 1024.0,
        took.as_secs_f64() * 1e3,
        sizes_text.join(", "),
        big.len(),
        ours,
        if sites_text.is_empty() { "none".into() } else { sites_text.join("; ") },
    );
    tracing::info!("VICTORY_MEM_OWNERS: {text}");
    say_private();
    text
}

/// The host's private blocks (above title memory's 1 TiB, not
/// write-combined), by reserved size: base -> (reserved, committed).
pub(crate) fn host_private() -> HashMap<usize, Block> {
    use windows_sys::Win32::System::Memory::MEM_PRIVATE;
    let mut out: HashMap<usize, Block> = HashMap::new();
    let mut at = 0usize;
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: a query of our own address space into a local structure.
        let got = unsafe { VirtualQuery(at as *const _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize == 0 {
            break;
        }
        let base = info.AllocationBase as usize;
        if info.Type == MEM_PRIVATE && base >= 1 << 40 && info.AllocationProtect & PAGE_WRITECOMBINE == 0 {
            let b = out.entry(base).or_default();
            b.reserved += info.RegionSize as u64;
            if info.State == MEM_COMMIT {
                b.committed += info.RegionSize as u64;
            }
        }
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    out
}

/// The log line for the host's private memory: blocks by size (the eight
/// with most commit), and per size the blocks that hold a buffer of ours,
/// by code site.
fn say_private() {
    let blocks = host_private();
    let mb = |b: u64| b as f64 / (1 << 20) as f64;
    let mut by_size: HashMap<u64, (u64, u64, Vec<usize>)> = HashMap::new();
    for (&base, b) in &blocks {
        let e = by_size.entry(b.reserved).or_default();
        e.0 += 1;
        e.1 += b.committed;
        e.2.push(base);
    }
    let mut sizes: Vec<_> = by_size.into_iter().collect();
    sizes.sort_by_key(|s| std::cmp::Reverse(s.1.1));
    let s = state();
    let parts: Vec<String> = sizes
        .iter()
        .take(8)
        .map(|(size, (n, committed, bases))| {
            let mut ours = 0u64;
            let mut sites: HashMap<&'static Location<'static>, u64> = HashMap::new();
            for base in bases {
                if let Some(owners) = s.owners.get(base) {
                    ours += 1;
                    for (site, (count, _)) in owners {
                        *sites.entry(site).or_default() += count;
                    }
                }
            }
            let mut sites: Vec<_> = sites.into_iter().collect();
            sites.sort_by_key(|s| std::cmp::Reverse(s.1));
            let named: Vec<String> = sites.iter().take(3).map(|(site, n)| format!("{}:{} {n}", site.file(), site.line())).collect();
            format!(
                "{n} x {:.2} MB ({:.2} GB committed; {ours} hold a buffer of ours{})",
                mb(*size),
                mb(*committed) / 1024.0,
                if named.is_empty() { String::new() } else { format!(": {}", named.join(", ")) }
            )
        })
        .collect();
    let total: u64 = blocks.values().map(|b| b.committed).sum();
    tracing::info!(
        "VICTORY_MEM_OWNERS: host private memory: {} blocks, {:.2} GB committed; by size: {}",
        blocks.len(),
        mb(total) / 1024.0,
        parts.join("; ")
    );
}

/// Tests: the code sites recorded in the block holding `at`, with counts.
#[cfg(test)]
pub(crate) fn recorded_at(at: usize) -> Vec<(&'static Location<'static>, u64)> {
    let Some((block, _, _)) = block_of(at) else {
        return Vec::new();
    };
    state().owners.get(&block).map(|m| m.iter().map(|(s, (n, _))| (*s, *n)).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree};

    #[test]
    fn a_write_combined_block_is_found_whole_and_an_ordinary_one_is_not() {
        // SAFETY: plain allocations of our own, freed below.
        let (wc, plain) = unsafe {
            // Reserved write-combined (Windows wants the flag at the
            // reservation too), half of it committed.
            let wc = VirtualAlloc(std::ptr::null(), 32 << 20, MEM_RESERVE, PAGE_READWRITE | PAGE_WRITECOMBINE);
            let committed = VirtualAlloc(wc, 16 << 20, MEM_COMMIT, PAGE_READWRITE | PAGE_WRITECOMBINE);
            assert!(!committed.is_null(), "committed");
            let plain = VirtualAlloc(std::ptr::null(), 32 << 20, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
            (wc as usize, plain as usize)
        };
        assert!(wc != 0 && plain != 0);
        let blocks = write_combined();
        assert_eq!(blocks.get(&wc), Some(&Block { reserved: 32 << 20, committed: 16 << 20 }), "found whole: reserved and committed");
        assert!(!blocks.contains_key(&plain), "an ordinary block is not write-combined");
        assert_eq!(block_of(wc + (5 << 20)).map(|b| (b.0, b.2)), Some((wc, true)), "an address inside is told its block");
        assert_eq!(block_of(plain + 100).map(|b| (b.0, b.2)), Some((plain, false)));
        let private = host_private();
        assert!(!private.contains_key(&wc), "a write-combined block is not counted as private");
        if plain >= 1 << 40 {
            assert_eq!(private.get(&plain), Some(&Block { reserved: 32 << 20, committed: 32 << 20 }), "the plain one is");
        }
        // SAFETY: our own allocations.
        unsafe {
            VirtualFree(wc as *mut _, 0, MEM_RELEASE);
            VirtualFree(plain as *mut _, 0, MEM_RELEASE);
        }
        assert!(!write_combined().contains_key(&wc), "freed: gone");
    }
}

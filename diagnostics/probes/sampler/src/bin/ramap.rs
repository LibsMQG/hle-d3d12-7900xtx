// Author: libs (hle-d3d12-7900xtx)
//! ramap: where a process's memory is, and how much of it is held twice.
//!
//! Opens the target with PROCESS_QUERY_INFORMATION | PROCESS_VM_READ only: it
//! cannot write to it, change protections or suspend it. It walks the address
//! space (VirtualQueryEx), asks which 4 KiB pages are in RAM
//! (QueryWorkingSetEx) and, with --content, hashes the resident pages (and
//! only those, so it faults nothing in). Page contents are never stored.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::fmt::Write as _;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::ProcessStatus::*;
use windows_sys::Win32::System::Threading::*;

const PAGE: usize = 4096;
const CHUNK: usize = 256; // pages per read and per working set query: 1 MiB
const GIB4: usize = 1 << 32;
/// VIctory keeps title memory below 2^40 (xmem::address_range).
const TIB: usize = 1 << 40;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Category {
    TitleViews,
    TitlePrivate,
    Low,
    Upload,
    HostPrivate,
    HostMapped,
    Stack,
    MappedFile,
    Image,
}

impl Category {
    fn name(self) -> &'static str {
        match self {
            Category::TitleViews => "guest: views of physical pages (pool and windows)",
            Category::TitlePrivate => "guest: private memory (4 GiB - 1 TiB)",
            Category::Low => "private below 4 GiB (32-bit guest and other)",
            Category::Upload => "write-combine (buffer di caricamento verso GPU)",
            Category::HostPrivate => "runtime + driver: private",
            Category::HostMapped => "VIctory + driver: sezioni senza file",
            Category::Stack => "stack dei thread",
            Category::MappedFile => "file mappati",
            Category::Image => "eseguibili e DLL",
        }
    }
    fn is_title(self) -> bool {
        matches!(self, Category::TitleViews | Category::TitlePrivate)
    }
    fn is_host(self) -> bool {
        matches!(self, Category::Upload | Category::HostPrivate | Category::HostMapped)
    }
}

struct Region {
    base: usize,
    size: usize,
    state: u32,
    protect: u32,
    kind: u32,
}

#[derive(Default, Clone, Copy)]
struct Totals {
    reserved: u64,
    committed: u64,
    /// Bytes in the working set, counting a page once per view of it.
    resident: u64,
    /// Resident bytes with a page shown n times counted 1/n each.
    unique: f64,
    private_resident: u64,
}

#[derive(Default, Clone, Copy)]
struct Content {
    hashed: u64,
    zero: u64,
    /// Non-zero pages whose bytes are also in title memory.
    in_title: u64,
    /// Non-zero pages whose bytes were already seen in host memory.
    host_twice: u64,
}

struct Allocation {
    base: usize,
    category: Category,
    protect: u32,
    totals: Totals,
    file: String,
}

fn wide(b: &[u16]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..n])
}

fn find_pid(name: &str) -> Option<u32> {
    // SAFETY: a process snapshot walked with a correctly sized entry.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut entry);
        while ok != 0 {
            if wide(&entry.szExeFile).eq_ignore_ascii_case(name) {
                found = Some(entry.th32ProcessID);
                break;
            }
            ok = Process32NextW(snap, &mut entry);
        }
        CloseHandle(snap);
        found
    }
}

fn regions(process: HANDLE) -> Vec<Region> {
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        // SAFETY: a plain out structure of the size passed.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let got = unsafe { VirtualQueryEx(process, at as *const c_void, &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 {
            break;
        }
        let base = info.BaseAddress as usize;
        if info.State != MEM_FREE {
            out.push((
                info.AllocationBase as usize,
                Region { base, size: info.RegionSize, state: info.State, protect: info.Protect | if info.State == MEM_COMMIT { 0 } else { info.AllocationProtect & PAGE_WRITECOMBINE }, kind: info.Type },
            ));
        }
        let Some(next) = base.checked_add(info.RegionSize) else { break };
        if next <= at {
            break;
        }
        at = next;
    }
    out.into_iter().map(|(_, r)| r).collect()
}

fn mapped_name(process: HANDLE, at: usize) -> String {
    let mut buffer = [0u16; 1024];
    // SAFETY: the buffer's length is passed.
    let n = unsafe { K32GetMappedFileNameW(process, at as *const c_void, buffer.as_mut_ptr(), buffer.len() as u32) };
    let full = String::from_utf16_lossy(&buffer[..n as usize]);
    full.rsplit('\\').next().unwrap_or("").to_string()
}

fn allocation_base(process: HANDLE, at: usize) -> usize {
    // SAFETY: as in `regions`.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let got = unsafe { VirtualQueryEx(process, at as *const c_void, &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
    if got == 0 { at } else { info.AllocationBase as usize }
}

fn categorize(parts: &[&Region], file: &str) -> Category {
    let first = parts[0];
    if first.kind == MEM_IMAGE {
        return Category::Image;
    }
    if !file.is_empty() {
        return Category::MappedFile;
    }
    if parts.iter().any(|r| r.state == MEM_COMMIT && r.protect & PAGE_GUARD != 0) {
        return Category::Stack;
    }
    if parts.iter().any(|r| r.state == MEM_COMMIT && r.protect & PAGE_WRITECOMBINE != 0) {
        return Category::Upload;
    }
    match (first.base, first.kind) {
        (b, MEM_PRIVATE) if b < GIB4 => Category::Low,
        (b, MEM_PRIVATE) if b < TIB => Category::TitlePrivate,
        (b, _) if b < TIB && b >= GIB4 => Category::TitleViews,
        (_, MEM_PRIVATE) => Category::HostPrivate,
        _ => Category::HostMapped,
    }
}

/// The working set entry of each page from `at`: (resident, shared, share count).
fn working_set(process: HANDLE, at: usize, pages: usize) -> Vec<(bool, bool, u32)> {
    let mut entries: Vec<PSAPI_WORKING_SET_EX_INFORMATION> = (0..pages)
        .map(|k| {
            // SAFETY: a plain structure, all zeroes valid.
            let mut e: PSAPI_WORKING_SET_EX_INFORMATION = unsafe { std::mem::zeroed() };
            e.VirtualAddress = (at + k * PAGE) as *mut c_void;
            e
        })
        .collect();
    // SAFETY: the byte length matches the vector.
    let ok = unsafe {
        K32QueryWorkingSetEx(process, entries.as_mut_ptr().cast(), (pages * size_of::<PSAPI_WORKING_SET_EX_INFORMATION>()) as u32)
    };
    entries
        .iter()
        .map(|e| {
            // SAFETY: the union is a bit field over one integer.
            let flags = unsafe { e.VirtualAttributes.Flags };
            let valid = ok != 0 && flags & 1 != 0;
            (valid, flags >> 15 & 1 != 0, (flags >> 1 & 7) as u32)
        })
        .collect()
}

fn hash(page: &[u8]) -> Option<u128> {
    let (mut a, mut b, mut any) = (0x9e37_79b9_7f4a_7c15u64, 0xc2b2_ae3d_27d4_eb4fu64, 0u64);
    for word in page.chunks_exact(8) {
        let w = u64::from_le_bytes(word.try_into().unwrap());
        any |= w;
        a = (a ^ w).wrapping_mul(0x100_0000_01b3).rotate_left(23);
        b = (b.wrapping_add(w)).wrapping_mul(0xff51_afd7_ed55_8ccd).rotate_left(31);
    }
    (any != 0).then_some((a as u128) << 64 | b as u128)
}

fn readable(protect: u32) -> bool {
    protect & (PAGE_NOACCESS | PAGE_GUARD) == 0 && protect != 0
}

const GB: f64 = (1u64 << 30) as f64;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |key: &str| args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned());
    let content = args.iter().any(|a| a == "--content");
    let out = value("--out");
    let pid = match (value("--pid"), value("--name")) {
        (Some(p), _) => p.parse().ok(),
        (None, Some(n)) => find_pid(&n),
        _ => None,
    };
    let Some(pid) = pid else {
        eprintln!("usage: ramap (--pid <pid> | --name <exe>) [--content] [--out report.txt]");
        std::process::exit(2);
    };
    // SAFETY: query and read rights only.
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if process.is_null() {
        eprintln!("cannot open process {pid}: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
    let started = std::time::Instant::now();

    // Regions grouped by allocation.
    let mut grouped: BTreeMap<usize, Vec<Region>> = BTreeMap::new();
    for region in regions(process) {
        grouped.entry(allocation_base(process, region.base)).or_default().push(region);
    }

    let mut totals: BTreeMap<Category, Totals> = BTreeMap::new();
    let mut allocations: Vec<Allocation> = Vec::new();
    // Resident readable pages to hash, as runs: (category, address, pages).
    let mut runs: Vec<(Category, usize, usize, usize)> = Vec::new();
    for (base, parts) in &grouped {
        let kind = parts[0].kind;
        let file = if kind == MEM_PRIVATE { String::new() } else { mapped_name(process, parts[0].base) };
        let refs: Vec<&Region> = parts.iter().collect();
        let category = categorize(&refs, &file);
        let mut sum = Totals::default();
        let mut protect = 0;
        for r in parts {
            sum.reserved += r.size as u64;
            if r.state != MEM_COMMIT {
                continue;
            }
            sum.committed += r.size as u64;
            protect |= r.protect;
            let pages = r.size / PAGE;
            for first in (0..pages).step_by(CHUNK) {
                let n = CHUNK.min(pages - first);
                let at = r.base + first * PAGE;
                let set = working_set(process, at, n);
                let mut run = 0;
                for (k, &(valid, shared, count)) in set.iter().enumerate() {
                    if valid {
                        sum.resident += PAGE as u64;
                        if shared {
                            sum.unique += PAGE as f64 / count.max(1) as f64;
                        } else {
                            sum.unique += PAGE as f64;
                            sum.private_resident += PAGE as u64;
                        }
                    }
                    if valid && readable(r.protect) {
                        run += 1;
                    } else if run > 0 {
                        runs.push((category, at + (k - run) * PAGE, run, allocations.len()));
                        run = 0;
                    }
                }
                if run > 0 {
                    runs.push((category, at + (n - run) * PAGE, run, allocations.len()));
                }
            }
        }
        let t = totals.entry(category).or_default();
        t.reserved += sum.reserved;
        t.committed += sum.committed;
        t.resident += sum.resident;
        t.unique += sum.unique;
        t.private_resident += sum.private_resident;
        allocations.push(Allocation { base: *base, category, protect, totals: sum, file });
    }

    let mut report = String::new();
    // SAFETY: both structures are sized for their calls.
    let (counters, performance) = unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        c.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        K32GetProcessMemoryInfo(process, (&raw mut c).cast(), c.cb);
        let mut p: PERFORMANCE_INFORMATION = std::mem::zeroed();
        p.cb = size_of::<PERFORMANCE_INFORMATION>() as u32;
        K32GetPerformanceInfo(&mut p, p.cb);
        (c, p)
    };
    let page = performance.PageSize as f64;
    let _ = writeln!(report, "ramap pid {pid}");
    let _ = writeln!(report, "per Windows: working set {:.2} GB, private (process commit) {:.2} GB", counters.WorkingSetSize as f64 / GB, counters.PrivateUsage as f64 / GB);
    let _ = writeln!(
        report,
        "sistema: RAM libera {:.2} / {:.2} GB, commit {:.2} / {:.2} GB",
        performance.PhysicalAvailable as f64 * page / GB,
        performance.PhysicalTotal as f64 * page / GB,
        performance.CommitTotal as f64 * page / GB,
        performance.CommitLimit as f64 * page / GB
    );
    let _ = writeln!(report, "\n== by category (GB) ==");
    let _ = writeln!(report, "{:<52}{:>10}{:>10}{:>10}{:>10}{:>10}", "category", "reserved", "commit", "in RAM", "unique*", "only its own");
    let mut sum = Totals::default();
    for (c, t) in &totals {
        let _ = writeln!(report, "{:<52}{:>10.2}{:>10.2}{:>10.2}{:>10.2}{:>10.2}", c.name(), t.reserved as f64 / GB, t.committed as f64 / GB, t.resident as f64 / GB, t.unique / GB, t.private_resident as f64 / GB);
        sum.reserved += t.reserved;
        sum.committed += t.committed;
        sum.resident += t.resident;
        sum.unique += t.unique;
        sum.private_resident += t.private_resident;
    }
    let _ = writeln!(report, "{:<52}{:>10.2}{:>10.2}{:>10.2}{:>10.2}{:>10.2}", "TOTALE", sum.reserved as f64 / GB, sum.committed as f64 / GB, sum.resident as f64 / GB, sum.unique / GB, sum.private_resident as f64 / GB);
    let _ = writeln!(report, "* unique: a page shown in n views counts 1/n (Windows counts n, up to 7 at most)");

    // Allocations of the same shape, outside title memory.
    let mut shapes: HashMap<(Category, u64, u32), (u64, u64, u64)> = HashMap::new();
    for a in allocations.iter().filter(|a| a.category.is_host()) {
        let e = shapes.entry((a.category, a.totals.reserved, a.protect)).or_default();
        e.0 += 1;
        e.1 += a.totals.committed;
        e.2 += a.totals.resident;
    }
    let mut shapes: Vec<_> = shapes.into_iter().collect();
    shapes.sort_by_key(|(_, v)| std::cmp::Reverse((v.2, v.1)));
    let _ = writeln!(report, "\n== runtime + driver: allocations grouped by shape (top 30 by RAM) ==");
    for ((c, size, protect), (n, committed, resident)) in shapes.iter().take(30) {
        let _ = writeln!(report, "{n:>6} x {:>10.2} MB  prot {protect:#06x}  commit {:>6.2} GB  in RAM {:>6.2} GB  {}", *size as f64 / (1 << 20) as f64, *committed as f64 / GB, *resident as f64 / GB, c.name());
    }

    // Sorted apart: `runs` names allocations by their index in `allocations`.
    let mut biggest: Vec<&Allocation> = allocations.iter().collect();
    biggest.sort_by_key(|a| std::cmp::Reverse((a.totals.resident, a.totals.committed)));
    let _ = writeln!(report, "\n== largest allocations by RAM (top 40) ==");
    for a in biggest.iter().take(40) {
        let _ = writeln!(report, "{:#018x}  commit {:>9.1} MB  in RAM {:>9.1} MB  prot {:#06x}  {} {}", a.base, a.totals.committed as f64 / (1 << 20) as f64, a.totals.resident as f64 / (1 << 20) as f64, a.protect, a.category.name(), a.file);
    }

    if content {
        // Title memory first, so that host pages can be looked up in it.
        runs.sort_by_key(|r| (!r.0.is_title(), r.1));
        let mut title: HashSet<u128> = HashSet::new();
        let mut host: HashSet<u128> = HashSet::new();
        let mut per: BTreeMap<Category, Content> = BTreeMap::new();
        let mut buffer = vec![0u8; CHUNK * PAGE];
        let mut failed = 0u64;
        let mut per_alloc: HashMap<usize, Content> = HashMap::new();
        for &(category, at, pages, alloc) in &runs {
            // Reading write-combined memory is very slow; sample it all the
            // same, it is where staging copies live.
            let mut got = 0usize;
            // SAFETY: `pages` is at most CHUNK, the buffer's size.
            let ok = unsafe { ReadProcessMemory(process, at as *const c_void, buffer.as_mut_ptr().cast(), pages * PAGE, &mut got) };
            if ok == 0 {
                failed += (pages * PAGE) as u64; // changed since the walk
                continue;
            }
            let c = per.entry(category).or_default();
            let a = per_alloc.entry(alloc).or_default();
            for bytes in buffer[..got].chunks_exact(PAGE) {
                c.hashed += PAGE as u64;
                a.hashed += PAGE as u64;
                let Some(h) = hash(bytes) else {
                    c.zero += PAGE as u64;
                    a.zero += PAGE as u64;
                    continue;
                };
                if category.is_title() {
                    title.insert(h);
                } else {
                    if title.contains(&h) {
                        c.in_title += PAGE as u64;
                        a.in_title += PAGE as u64;
                    }
                    if category.is_host() && !host.insert(h) {
                        c.host_twice += PAGE as u64;
                        a.host_twice += PAGE as u64;
                    }
                }
            }
        }
        let _ = writeln!(report, "\n== contents of pages in RAM (GB) ==");
        let _ = writeln!(report, "{:<52}{:>10}{:>10}{:>18}{:>18}", "category", "read", "zero", "also in guest", "repeated in host");
        for (c, v) in &per {
            let _ = writeln!(report, "{:<52}{:>10.2}{:>10.2}{:>18.2}{:>18.2}", c.name(), v.hashed as f64 / GB, v.zero as f64 / GB, v.in_title as f64 / GB, v.host_twice as f64 / GB);
        }
        let _ = writeln!(report, "distinct non-zero pages in guest memory: {:.2} GB", (title.len() * PAGE) as f64 / GB);
        // The same, per allocation shape outside title memory.
        let mut shaped: HashMap<(Category, u64, u32), (u64, Content)> = HashMap::new();
        for (&i, v) in &per_alloc {
            let a = &allocations[i];
            if !a.category.is_host() {
                continue;
            }
            let e = shaped.entry((a.category, a.totals.reserved, a.protect)).or_default();
            e.0 += 1;
            e.1.hashed += v.hashed;
            e.1.zero += v.zero;
            e.1.in_title += v.in_title;
            e.1.host_twice += v.host_twice;
        }
        let mut shaped: Vec<_> = shaped.into_iter().collect();
        shaped.sort_by_key(|(_, (_, v))| std::cmp::Reverse(v.hashed));
        let _ = writeln!(report, "\n== contents by shape, runtime + driver (top 25 by pages read, GB) ==");
        for ((c, size, protect), (n, v)) in shaped.iter().take(25) {
            let _ = writeln!(
                report,
                "{n:>6} x {:>10.2} MB  prot {protect:#06x}  read {:>6.2}  zero {:>6.2}  also in guest {:>6.2}  repeated {:>6.2}  {}",
                *size as f64 / (1 << 20) as f64,
                v.hashed as f64 / GB,
                v.zero as f64 / GB,
                v.in_title as f64 / GB,
                v.host_twice as f64 / GB,
                c.name()
            );
        }
        let _ = writeln!(report, "not read because they changed during the scan: {:.2} GB", failed as f64 / GB);
        let _ = writeln!(report, "nota: il processo gira durante la lettura, i confronti sono una fotografia approssimata");
    }
    let _ = writeln!(report, "durata scansione: {:.1} s", started.elapsed().as_secs_f64());
    // SAFETY: our handle.
    unsafe { CloseHandle(process) };
    print!("{report}");
    if let Some(path) = out {
        if let Err(e) = std::fs::write(&path, &report) {
            eprintln!("cannot write {path}: {e}");
            std::process::exit(1);
        }
    }
}

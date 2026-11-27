// Author: libs (hle-d3d12-7900xtx)
//! membench: what the memory calls VIctory leans on cost on this PC, on
//! memory of this process only: ReadProcessMemory against a plain copy,
//! GetWriteWatch, VirtualProtect and VirtualQuery, on private memory and on
//! views of a pagefile section (as the title's page pool is shown).

use std::ffi::c_void;
use std::time::Instant;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const MEM_WRITE_WATCH: u32 = 0x20_0000;
const K4: usize = 4 << 10;
const K64: usize = 64 << 10;
const MB: usize = 1 << 20;

fn private(len: usize, watch: bool) -> *mut u8 {
    // SAFETY: a plain allocation.
    let p = unsafe { VirtualAlloc(std::ptr::null(), len, MEM_RESERVE | MEM_COMMIT | if watch { MEM_WRITE_WATCH } else { 0 }, PAGE_READWRITE) };
    assert!(!p.is_null());
    // SAFETY: just committed.
    unsafe { std::ptr::write_bytes(p.cast::<u8>(), 1, len) };
    p.cast()
}

fn view(len: usize) -> *mut u8 {
    // SAFETY: a committed pagefile section and one view of all of it.
    unsafe {
        let section = CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE | SEC_COMMIT, 0, len as u32, std::ptr::null());
        assert!(!section.is_null());
        let at = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, len).Value;
        assert!(!at.is_null());
        std::ptr::write_bytes(at.cast::<u8>(), 1, len);
        at.cast()
    }
}

/// Copies `len` bytes in chunks `rounds` times; GB per second.
fn copy_rate(from: *const u8, to: *mut u8, len: usize, chunk: usize, kernel: bool, rounds: usize) -> f64 {
    let started = Instant::now();
    for _ in 0..rounds {
        let mut at = 0;
        while at < len {
            let n = chunk.min(len - at);
            if kernel {
                let mut done = 0;
                // SAFETY: both ranges lie in this process's live allocations.
                let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), from.add(at).cast(), to.add(at).cast(), n, &mut done) };
                assert!(ok != 0 && done == n);
            } else {
                // SAFETY: as above; the two do not overlap.
                unsafe { std::ptr::copy_nonoverlapping(from.add(at), to.add(at), n) };
            }
            at += n;
        }
        std::hint::black_box(to);
    }
    (len * rounds) as f64 / started.elapsed().as_secs_f64() / 1e9
}

fn micros(calls: usize, mut call: impl FnMut(usize)) -> f64 {
    let started = Instant::now();
    for k in 0..calls {
        call(k);
    }
    started.elapsed().as_secs_f64() * 1e6 / calls as f64
}

fn main() {
    let len = 192 * MB;
    let to = private(len, false);
    let plain = private(len, false);
    let mapped = view(len);
    println!("copy of {} MB, GB/s (a frame copies about 190 MB)", len / MB);
    println!("{:<26}{:>10}{:>16}{:>22}", "source", "chunk", "plain copy", "ReadProcessMemory");
    for (name, from) in [("private memory", plain), ("section view", mapped)] {
        for chunk in [K4, K64, MB] {
            let a = copy_rate(from, to, len, chunk, false, 6);
            let b = copy_rate(from, to, len, chunk, true, 6);
            println!("{name:<26}{:>7} KB{a:>16.1}{b:>22.1}", chunk / K4 * 4);
        }
    }
    println!("\nsmall reads, microseconds a call (same 64 KB region, warm)");
    for size in [256, K4, K64] {
        let a = micros(200_000, |k| {
            // SAFETY: inside both allocations.
            unsafe { std::ptr::copy_nonoverlapping(plain.add((k % 16) * K4), to, size) };
            std::hint::black_box(to);
        });
        let b = micros(200_000, |k| {
            let mut done = 0;
            // SAFETY: inside both allocations.
            unsafe { ReadProcessMemory(GetCurrentProcess(), plain.add((k % 16) * K4).cast(), to.cast(), size, &mut done) };
        });
        println!("{size:>8} bytes: plain {a:>7.2}   ReadProcessMemory {b:>7.2}");
    }

    println!("\nGetWriteWatch with reset, microseconds a call");
    let watched = private(200 * MB, true);
    let mut found = vec![std::ptr::null_mut::<c_void>(); 200 * MB / K4];
    for (region, dirty) in [(K64, 0), (4 * MB, 0), (4 * MB, 16), (174 * MB, 0), (174 * MB, 256)] {
        let mut ask = || {
            let mut count = found.len();
            let mut granularity = 0;
            // SAFETY: write-watched memory; the buffer holds `count` entries.
            let failed = unsafe { GetWriteWatch(1, watched.cast(), region, found.as_mut_ptr(), &mut count, &mut granularity) };
            assert_eq!(failed, 0);
        };
        ask();
        let mut spent = 0.0;
        let calls = 300;
        for _ in 0..calls {
            for k in 0..dirty {
                // SAFETY: inside the region.
                unsafe { watched.add((k * 37 % (region / K4)) * K4).write_volatile(2) };
            }
            let started = Instant::now();
            ask();
            spent += started.elapsed().as_secs_f64();
        }
        println!("  {:>7} KB, {dirty:>3} pages written: {:>8.1}", region / 1024, spent * 1e6 / f64::from(calls));
    }

    println!("\nVirtualProtect of 64 KB (read-only and back), microseconds a call");
    for (name, base) in [("private memory", plain), ("section view", mapped)] {
        let t = micros(20_000, |k| {
            let mut old = 0;
            let protect = if k % 2 == 0 { PAGE_READONLY } else { PAGE_READWRITE };
            // SAFETY: 64 KB inside a live allocation, restored on the odd call.
            let ok = unsafe { VirtualProtect(base.add((k / 2 % 1024) * K64).cast(), K64, protect, &mut old) };
            assert!(ok != 0);
        });
        println!("  {name:<18}{t:>8.2}");
    }

    println!("\nVirtualQuery, microseconds a call, by how far the pages share the first one's state");
    for run in [K64, 4 * MB, 64 * MB, 174 * MB] {
        for (name, base) in [("private memory", plain), ("section view", mapped)] {
            let mut old = 0;
            if run < len {
                // SAFETY: one page just past the run gets another protection.
                unsafe { VirtualProtect(base.add(run).cast(), K4, PAGE_READONLY, &mut old) };
            }
            let t = micros(300, |_| {
                // SAFETY: plain query into a structure of the size passed.
                let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
                let got = unsafe { VirtualQuery(base.cast(), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
                assert!(got != 0 && info.RegionSize >= run.min(len));
            });
            if run < len {
                // SAFETY: restored.
                unsafe { VirtualProtect(base.add(run).cast(), K4, PAGE_READWRITE, &mut old) };
            }
            println!("  {name:<18}{:>7} KB: {t:>9.1}", run / 1024);
        }
    }
}

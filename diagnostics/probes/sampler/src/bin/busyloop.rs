// Author: libs (hle-d3d12-amd)
//! Harmless test target for `sampler`: a few named threads with known hot spots.
//! Usage: busyloop [seconds]   (default 15; exits by itself)
use std::hint::black_box;
use std::time::{Duration, Instant};

#[inline(never)]
fn hot_function_alpha(n: u64) -> u64 {
    let mut x = n;
    for i in 0..20_000u64 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(i ^ (x >> 7));
    }
    black_box(x)
}

#[inline(never)]
fn hot_function_beta(n: u64) -> u64 {
    let mut x = n;
    for i in 0..10_000u64 {
        x = x.rotate_left(5) ^ i.wrapping_mul(0x9E37_79B9);
    }
    black_box(x)
}

/// Spends most of its time inside ntdll/kernelbase (QueryPerformanceCounter),
/// so the inclusive view should attribute that time here.
#[inline(never)]
fn caller_into_system_dlls(iters: u32) -> u128 {
    let mut acc = 0u128;
    for _ in 0..iters {
        acc = acc.wrapping_add(black_box(Instant::now()).elapsed().as_nanos());
    }
    acc
}

fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(15);
    let end = Instant::now() + Duration::from_secs(secs);
    let mut hs = Vec::new();
    hs.push(std::thread::Builder::new().name("hot-spinner".into()).spawn(move || {
        let mut v = 1u64;
        while Instant::now() < end {
            v = hot_function_alpha(v);
            v = hot_function_alpha(v);
            v = hot_function_beta(v);
        }
        black_box(v);
    }).unwrap());
    hs.push(std::thread::Builder::new().name("syscall-caller".into()).spawn(move || {
        let mut a = 0u128;
        while Instant::now() < end {
            a = a.wrapping_add(caller_into_system_dlls(1000));
        }
        black_box(a);
    }).unwrap());
    hs.push(std::thread::Builder::new().name("sleeper".into()).spawn(move || {
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
        }
    }).unwrap());
    println!("busyloop pid {} running {} s", std::process::id(), secs);
    for h in hs {
        h.join().unwrap();
    }
}

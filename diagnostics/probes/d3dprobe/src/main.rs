// Author: libs (hle-d3d12-7900xtx)
//! d3dprobe: what the host's Direct3D 12 driver keeps in write-combined
//! memory for command lists and for upload buffers. One scenario per run,
//! so that each starts from a clean process. It measures its own address
//! space and stops if the PC's commit headroom gets small.

use std::collections::BTreeMap;
use std::ffi::c_void;

use windows::core::{Interface, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::GetCurrentProcess;

const MB: f64 = (1u64 << 20) as f64;
const STOP_BELOW_GB: f64 = 6.0;

fn measure(label: &str) {
    // Allocations with committed write-combined pages: whole size -> (count, committed).
    let mut all: BTreeMap<usize, (usize, u64, bool)> = BTreeMap::new();
    let mut at = 0usize;
    loop {
        let mut m = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: a plain out structure of the size passed.
        if unsafe { VirtualQuery(Some(at as *const c_void), &mut m, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        if m.State != MEM_FREE {
            let e = all.entry(m.AllocationBase as usize).or_default();
            e.0 += m.RegionSize;
            if m.State == MEM_COMMIT {
                e.1 += m.RegionSize as u64;
                e.2 |= m.Protect.0 & PAGE_WRITECOMBINE.0 != 0;
            }
        }
        let next = (m.BaseAddress as usize).saturating_add(m.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    let mut shapes: BTreeMap<usize, (u64, u64)> = BTreeMap::new();
    let mut total = 0;
    for &(size, committed, wc) in all.values() {
        if wc {
            let s = shapes.entry(size).or_default();
            s.0 += 1;
            s.1 += committed;
            total += committed;
        }
    }
    let mut shapes: Vec<_> = shapes.into_iter().collect();
    shapes.sort_by_key(|s| std::cmp::Reverse(s.1 .1));
    let text: Vec<String> = shapes.iter().take(4).map(|(size, (n, _))| format!("{n} x {:.2} MB", *size as f64 / MB)).collect();
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: the structure and its size.
    unsafe {
        let _ = GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut c).cast(), size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32);
    }
    println!("{label:<58} private {:>8.0} MB  write-combine {:>8.0} MB  [{}]", c.PrivateUsage as f64 / MB, total as f64 / MB, text.join(", "));
}

fn check_headroom() {
    let mut p = PERFORMANCE_INFORMATION::default();
    // SAFETY: the structure and its size.
    unsafe {
        let _ = GetPerformanceInfo(&mut p, size_of::<PERFORMANCE_INFORMATION>() as u32);
    }
    let free = (p.CommitLimit - p.CommitTotal) as f64 * p.PageSize as f64 / (1u64 << 30) as f64;
    if free < STOP_BELOW_GB {
        println!("FERMATA: margine di commit {free:.1} GB");
        std::process::exit(3);
    }
}

struct Gpu {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    value: u64,
}

impl Gpu {
    fn new() -> Result<Self> {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: plain object creation, here and below.
        unsafe {
            D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device)?;
            let device = device.unwrap();
            let queue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })?;
            let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            Ok(Gpu { device, queue, fence, value: 0 })
        }
    }

    fn run(&mut self, list: &ID3D12GraphicsCommandList) -> Result<()> {
        self.value += 1;
        // SAFETY: a closed list; a null event makes the wait block.
        unsafe {
            self.queue.ExecuteCommandLists(&[Some(list.cast()?)]);
            self.queue.Signal(&self.fence, self.value)?;
            self.fence.SetEventOnCompletion(self.value, HANDLE::default())?;
            self.device.GetDeviceRemovedReason()
        }
    }

    fn buffer(&self, size: u64, kind: D3D12_HEAP_TYPE) -> Result<ID3D12Resource> {
        let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let state = if kind == D3D12_HEAP_TYPE_UPLOAD { D3D12_RESOURCE_STATE_GENERIC_READ } else { D3D12_RESOURCE_STATE_COMMON };
        let mut buffer: Option<ID3D12Resource> = None;
        // SAFETY: plain creation of a buffer.
        unsafe { self.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut buffer)? };
        Ok(buffer.unwrap())
    }

    /// A mapped upload buffer, every page of it written.
    fn upload(&self, size: u64) -> Result<ID3D12Resource> {
        let buffer = self.buffer(size, D3D12_HEAP_TYPE_UPLOAD)?;
        let mut data = std::ptr::null_mut();
        // SAFETY: the mapping holds `size` bytes.
        unsafe {
            buffer.Map(0, None, Some(&mut data))?;
            std::ptr::write_bytes(data.cast::<u8>(), 1, size as usize);
        }
        Ok(buffer)
    }
}

type List = (ID3D12CommandAllocator, ID3D12GraphicsCommandList);

/// Records `commands` small copies (no pipeline needed) and closes the list.
fn record(list: &ID3D12GraphicsCommandList, from: &ID3D12Resource, to: &ID3D12Resource, commands: u64) -> Result<()> {
    // SAFETY: an open list; the offsets lie inside both buffers (1 MiB).
    unsafe {
        for k in 0..commands {
            let at = (k % 4000) * 256;
            list.CopyBufferRegion(to, at, from, at, 256);
        }
        list.Close()
    }
}

/// `count` lists with their own allocators, each recording `commands` copies
/// and run once; then reset, then dropped.
fn lists(gpu: &mut Gpu, count: usize, commands: u64) -> Result<()> {
    let from = gpu.upload(1 << 20)?;
    let to = gpu.buffer(1 << 20, D3D12_HEAP_TYPE_DEFAULT)?;
    measure("prima delle liste");
    let mut made: Vec<List> = Vec::new();
    for _ in 0..count {
        check_headroom();
        // SAFETY: plain object creation.
        unsafe {
            let allocator: ID3D12CommandAllocator = gpu.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = gpu.device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            made.push((allocator, list));
        }
    }
    measure(&format!("{count} liste create, vuote"));
    for (_, list) in &made {
        check_headroom();
        record(list, &from, &to, commands)?;
    }
    measure(&format!("registrati {commands} comandi in ognuna"));
    for (_, list) in &made {
        gpu.run(list)?;
    }
    measure("eseguite");
    for (allocator, list) in &made {
        // SAFETY: the GPU is done with them.
        unsafe {
            allocator.Reset()?;
            list.Reset(allocator, None)?;
            list.Close()?;
        }
    }
    measure("dopo Reset di allocator e liste");
    drop(made);
    measure("liste e allocator distrutti");
    Ok(())
}

/// `count` lists, each recording `commands` copies, run once; then each
/// keeps its list and gets a new allocator, the old one destroyed. Then
/// `rounds` more rounds of the same with a new allocator after each.
fn renew(gpu: &mut Gpu, count: usize, commands: u64, rounds: usize) -> Result<()> {
    let from = gpu.upload(1 << 20)?;
    let to = gpu.buffer(1 << 20, D3D12_HEAP_TYPE_DEFAULT)?;
    measure("prima delle liste");
    let mut made: Vec<List> = Vec::new();
    for _ in 0..count {
        check_headroom();
        // SAFETY: plain object creation.
        unsafe {
            let allocator: ID3D12CommandAllocator = gpu.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = gpu.device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            made.push((allocator, list));
        }
    }
    for (_, list) in &made {
        check_headroom();
        record(list, &from, &to, commands)?;
        gpu.run(list)?;
    }
    measure(&format!("{count} liste da {commands} comandi eseguite"));
    let mut spent = std::time::Duration::ZERO;
    let mut renewed = 0u32;
    for round in 0..=rounds {
        for (allocator, list) in &mut made {
            let started = std::time::Instant::now();
            // SAFETY: the GPU is done with the list and its allocator; the
            // list is closed and moves to the new one before the old goes.
            unsafe {
                let fresh: ID3D12CommandAllocator = gpu.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
                list.Reset(&fresh, None)?;
                *allocator = fresh;
            }
            spent += started.elapsed();
            renewed += 1;
            if round == rounds {
                // SAFETY: an open list.
                unsafe { list.Close()? };
            } else {
                check_headroom();
                record(list, &from, &to, commands)?;
                gpu.run(list)?;
            }
        }
        if round == 0 || round == rounds {
            let what = if round == rounds { "ultimo rinnovo, liste vuote" } else { "primo rinnovo e nuova registrazione" };
            measure(what);
        }
    }
    println!("rinnovo: {renewed} volte, media {:.1} us", spent.as_secs_f64() * 1e6 / f64::from(renewed));
    Ok(())
}

/// The same recording work as `lists`, with `kept` lists used in turn.
fn reuse(gpu: &mut Gpu, kept: usize, total: usize, commands: u64) -> Result<()> {
    let from = gpu.upload(1 << 20)?;
    let to = gpu.buffer(1 << 20, D3D12_HEAP_TYPE_DEFAULT)?;
    measure("prima delle liste");
    let mut made: Vec<List> = Vec::new();
    for _ in 0..kept {
        // SAFETY: plain object creation.
        unsafe {
            let allocator: ID3D12CommandAllocator = gpu.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = gpu.device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            list.Close()?;
            made.push((allocator, list));
        }
    }
    for k in 0..total {
        check_headroom();
        let (allocator, list) = &made[k % kept];
        // SAFETY: each run below waits, so the list is idle.
        unsafe {
            allocator.Reset()?;
            list.Reset(allocator, None)?;
        }
        record(list, &from, &to, commands)?;
        gpu.run(list)?;
    }
    measure(&format!("{total} registrazioni da {commands} comandi su {kept} liste"));
    Ok(())
}

/// Upload buffers made and freed in waves, as one per copy would be.
fn churn(gpu: &mut Gpu) -> Result<()> {
    const SIZES: [u64; 6] = [64 << 10, 256 << 10, 1 << 20, 4 << 20, 5 << 20 | 1 << 18, 11 << 20];
    measure("prima dei buffer");
    for wave in 1..=3 {
        let mut held = Vec::new();
        let mut bytes = 0;
        // About 2 GiB alive at the top of each wave.
        let mut k = 0;
        while bytes < 2u64 << 30 {
            check_headroom();
            let size = SIZES[(k * 7 + wave) % SIZES.len()];
            held.push(gpu.upload(size)?);
            bytes += size;
            k += 1;
        }
        measure(&format!("ondata {wave}: {} buffer vivi, {:.0} MB chiesti", held.len(), bytes as f64 / MB));
        drop(held);
        measure(&format!("ondata {wave}: tutti liberati"));
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    measure("5 s dopo");
    Ok(())
}

/// Waves in which one small buffer in sixteen survives: what stays committed
/// if the driver cuts small buffers out of larger blocks.
fn survivors(gpu: &mut Gpu) -> Result<()> {
    measure("prima dei buffer");
    let mut kept = Vec::new();
    let mut kept_bytes = 0u64;
    for wave in 1..=3 {
        let mut held = Vec::new();
        for _ in 0..4096 {
            check_headroom();
            held.push(gpu.upload(256 << 10)?);
        }
        measure(&format!("ondata {wave}: 4096 buffer da 256 KB creati"));
        for (k, buffer) in held.into_iter().enumerate() {
            if k % 16 == 0 {
                kept.push(buffer);
                kept_bytes += 256 << 10;
            }
        }
        measure(&format!("ondata {wave}: restano vivi {:.0} MB", kept_bytes as f64 / MB));
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |i: usize, default: u64| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let mut gpu = Gpu::new()?;
    measure("solo dispositivo");
    match args.first().map(String::as_str) {
        Some("lists") => lists(&mut gpu, number(1, 756) as usize, number(2, 2000)),
        Some("reuse") => reuse(&mut gpu, number(1, 32) as usize, number(2, 756) as usize, number(3, 2000)),
        Some("renew") => renew(&mut gpu, number(1, 100) as usize, number(2, 500_000), number(3, 3) as usize),
        Some("churn") => churn(&mut gpu),
        Some("survivors") => survivors(&mut gpu),
        _ => {
            println!("usage: d3dprobe lists [count] [commands] | reuse [kept] [total] [commands] | churn | survivors");
            Ok(())
        }
    }
}

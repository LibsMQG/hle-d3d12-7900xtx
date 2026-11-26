//! uvmprobe: can this PC do on Windows what a unified-memory driver does on
//! Linux? No game and no VIctory code. Each test prints what it found.
//!
//!  1. capabilities
//!  2. size of a reserved (tiled) buffer: a mirror of a console address range
//!  3. one video-memory tile at two addresses, and a tile mapped elsewhere
//!  4. the same through compute shaders, then an SRV and a UAV of that one
//!     buffer in one dispatch
//!  5. what UpdateTileMappings costs, one 64 KiB tile at a time and batched
//!  6. what a CPU-side fault costs, and fetching 64 KiB from video memory

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use windows::core::{s, Interface, Result, PCSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_FEATURE_LEVEL_12_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory4};
use windows::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::*;

const TILE: u64 = 65536;
const GIB: u64 = 1 << 30;
const STOP_BELOW_GB: f64 = 6.0;

fn check_headroom() {
    let mut p = PERFORMANCE_INFORMATION::default();
    // SAFETY: the structure and its size.
    unsafe {
        let _ = GetPerformanceInfo(&mut p, size_of::<PERFORMANCE_INFORMATION>() as u32);
    }
    let free = (p.CommitLimit - p.CommitTotal) as f64 * p.PageSize as f64 / GIB as f64;
    if free < STOP_BELOW_GB {
        println!("STOP: commit headroom {free:.1} GB");
        std::process::exit(3);
    }
}

struct Gpu {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    value: u64,
}

impl Gpu {
    fn new() -> Result<Self> {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: plain object creation.
        unsafe {
            D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device)?;
            let device = device.unwrap();
            let queue: ID3D12CommandQueue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })?;
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            list.Close()?;
            Ok(Gpu { device, queue, fence, allocator, list, value: 0 })
        }
    }

    /// Waits until everything on the queue so far is done.
    fn wait(&mut self) -> Result<()> {
        self.value += 1;
        // SAFETY: a null event makes the wait block.
        unsafe {
            self.queue.Signal(&self.fence, self.value)?;
            self.fence.SetEventOnCompletion(self.value, HANDLE::default())?;
            self.device.GetDeviceRemovedReason()
        }
    }

    /// Records with `f`, runs, waits.
    fn run(&mut self, f: impl FnOnce(&ID3D12GraphicsCommandList)) -> Result<()> {
        // SAFETY: the allocator is idle: every run waits.
        unsafe {
            self.allocator.Reset()?;
            self.list.Reset(&self.allocator, None)?;
            f(&self.list);
            self.list.Close()?;
            self.queue.ExecuteCommandLists(&[Some(self.list.cast()?)]);
        }
        self.wait()
    }

    fn desc(size: u64, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: flags,
            ..Default::default()
        }
    }

    fn buffer(&self, size: u64, kind: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS) -> Result<ID3D12Resource> {
        let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
        let state = match kind {
            D3D12_HEAP_TYPE_UPLOAD => D3D12_RESOURCE_STATE_GENERIC_READ,
            D3D12_HEAP_TYPE_READBACK => D3D12_RESOURCE_STATE_COPY_DEST,
            _ => D3D12_RESOURCE_STATE_COMMON,
        };
        let mut out: Option<ID3D12Resource> = None;
        // SAFETY: plain creation of a buffer.
        unsafe {
            self.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &Self::desc(size, flags), state, None, &mut out)?
        };
        Ok(out.unwrap())
    }

    fn reserved(&self, size: u64) -> Result<ID3D12Resource> {
        let mut out: Option<ID3D12Resource> = None;
        // SAFETY: plain creation of a reserved buffer.
        unsafe {
            self.device.CreateReservedResource(
                &Self::desc(size, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS),
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut out,
            )?
        };
        Ok(out.unwrap())
    }

    fn heap(&self, size: u64) -> Result<ID3D12Heap> {
        let desc = D3D12_HEAP_DESC {
            SizeInBytes: size,
            Properties: D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() },
            Alignment: 0,
            Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS,
        };
        let mut out: Option<ID3D12Heap> = None;
        // SAFETY: plain creation of a heap.
        unsafe { self.device.CreateHeap(&desc, &mut out)? };
        Ok(out.unwrap())
    }

    /// Maps `count` tiles of `resource` from tile `at` onto `heap` from tile `from`.
    fn map_tiles(&self, resource: &ID3D12Resource, at: u32, heap: &ID3D12Heap, from: u32, count: u32) {
        let start = D3D12_TILED_RESOURCE_COORDINATE { X: at, ..Default::default() };
        let size = D3D12_TILE_REGION_SIZE { NumTiles: count, ..Default::default() };
        let flag = D3D12_TILE_RANGE_FLAG_NONE;
        // SAFETY: one region and one range, each array of one element; the
        // tiles lie inside the resource and the heap.
        unsafe {
            self.queue.UpdateTileMappings(
                resource,
                1,
                Some(&start),
                Some(&size),
                heap,
                1,
                Some(&flag),
                Some(&from),
                Some(&count),
                D3D12_TILE_MAPPING_FLAG_NONE,
            );
        }
    }
}

fn transition(r: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: a borrowed pointer: the barrier does not outlive `r`
                // and releases nothing.
                pResource: unsafe { std::mem::transmute_copy(r) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// "Any tile may have been reached through another address": null resources.
fn aliasing() -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_ALIASING,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Aliasing: ManuallyDrop::new(D3D12_RESOURCE_ALIASING_BARRIER::default()),
        },
    }
}

fn uav_barrier() -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_UAV, ..Default::default() }
}

fn mapped(r: &ID3D12Resource) -> *mut u8 {
    let mut p = std::ptr::null_mut();
    // SAFETY: an upload or readback buffer.
    unsafe { r.Map(0, None, Some(&mut p)).unwrap() };
    p.cast()
}

fn pattern(seed: u32, words: usize) -> Vec<u32> {
    (0..words as u32).map(|i| i.wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(97_531))).collect()
}

fn capabilities(gpu: &Gpu) -> Result<()> {
    // SAFETY: each structure with its own size.
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory1()?;
        let adapter: windows::Win32::Graphics::Dxgi::IDXGIAdapter1 = factory.EnumAdapterByLuid(gpu.device.GetAdapterLuid())?;
        let d = adapter.GetDesc1()?;
        println!("adapter: {}", String::from_utf16_lossy(&d.Description).trim_end_matches('\0'));
        fn ask<T: Default>(device: &ID3D12Device, feature: D3D12_FEATURE) -> Option<T> {
            let mut data = T::default();
            // SAFETY: the structure that goes with the feature, and its size.
            unsafe { device.CheckFeatureSupport(feature, (&mut data as *mut T).cast(), size_of::<T>() as u32) }
                .ok()
                .map(|()| data)
        }
        if let Some(o) = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS>(&gpu.device, D3D12_FEATURE_D3D12_OPTIONS) {
            println!("tiled resources tier: {}", o.TiledResourcesTier.0);
        }
        if let Some(o) = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS12>(&gpu.device, D3D12_FEATURE_D3D12_OPTIONS12) {
            println!("enhanced barriers: {}", o.EnhancedBarriersSupported.as_bool());
        }
        if let Some(o) = ask::<D3D12_FEATURE_DATA_D3D12_OPTIONS16>(&gpu.device, D3D12_FEATURE_D3D12_OPTIONS16) {
            println!("GPU upload heap (CPU writes straight into video memory): {}", o.GPUUploadHeapSupported.as_bool());
        }
        if let Some(o) =
            ask::<D3D12_FEATURE_DATA_GPU_VIRTUAL_ADDRESS_SUPPORT>(&gpu.device, D3D12_FEATURE_GPU_VIRTUAL_ADDRESS_SUPPORT)
        {
            println!(
                "GPU virtual address bits: {} per resource, {} per process",
                o.MaxGPUVirtualAddressBitsPerResource, o.MaxGPUVirtualAddressBitsPerProcess
            );
        }
    }
    Ok(())
}

/// One video-memory tile at two addresses of a reserved buffer, by copies.
fn alias_copies(gpu: &mut Gpu, r: &ID3D12Resource) -> Result<()> {
    const PAIRS: u32 = 256;
    let heap = gpu.heap(u64::from(PAIRS) * 2 * TILE)?;
    let up = gpu.buffer(TILE, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_FLAG_NONE)?;
    let back = gpu.buffer(TILE, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE)?;
    let (p_up, p_back) = (mapped(&up), mapped(&back));
    let words = (TILE / 4) as usize;
    let mut bad = 0;
    for k in 0..PAIRS {
        // Two addresses far apart: near 1 GiB and near 20 GiB.
        let a = (GIB / TILE) as u32 + k * 3;
        let b = (20 * GIB / TILE) as u32 + k * 5;
        gpu.map_tiles(r, a, &heap, k, 1);
        gpu.map_tiles(r, b, &heap, k, 1);
        let want = pattern(k, words);
        // SAFETY: both mappings hold a tile.
        unsafe { std::ptr::copy_nonoverlapping(want.as_ptr(), p_up.cast::<u32>(), words) };
        gpu.run(|l| unsafe {
            l.ResourceBarrier(&[aliasing(), transition(r, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST)]);
            l.CopyBufferRegion(r, u64::from(a) * TILE, &up, 0, TILE);
            l.ResourceBarrier(&[aliasing(), transition(r, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            l.CopyBufferRegion(&back, 0, r, u64::from(b) * TILE, TILE);
            l.ResourceBarrier(&[transition(r, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON)]);
        })?;
        // SAFETY: the copy finished.
        let got = unsafe { std::slice::from_raw_parts(p_back.cast::<u32>(), words) };
        bad += u32::from(got != want.as_slice());
    }
    println!("write at address A, read at address B (same tile): {} of {PAIRS} pairs identical", PAIRS - bad);

    // Remap: address B now shows another tile, holding other data.
    let (a, b) = ((GIB / TILE) as u32, (20 * GIB / TILE) as u32);
    gpu.map_tiles(r, b, &heap, 7, 1);
    gpu.run(|l| unsafe {
        l.ResourceBarrier(&[aliasing(), transition(r, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        l.CopyBufferRegion(&back, 0, r, u64::from(b) * TILE, TILE);
        l.ResourceBarrier(&[transition(r, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON)]);
    })?;
    // SAFETY: the copy finished.
    let got = unsafe { std::slice::from_raw_parts(p_back.cast::<u32>(), words) };
    println!("after remapping B to another tile, B shows that tile's data: {}", got == pattern(7, words).as_slice());
    let _ = a;
    Ok(())
}

const SHADER: &str = r#"
#define RS "RootConstants(num32BitConstants=1, b0), SRV(t0), UAV(u0), UAV(u1)"
cbuffer C : register(b0) { uint seed; };
ByteAddressBuffer src_srv : register(t0);
RWByteAddressBuffer src_uav : register(u0);
RWByteAddressBuffer dst : register(u1);
[RootSignature(RS)] [numthreads(64,1,1)]
void gen(uint3 id : SV_DispatchThreadID) { dst.Store(id.x * 4, id.x * 2654435761u + seed * 97531u); }
[RootSignature(RS)] [numthreads(64,1,1)]
void copy_uav(uint3 id : SV_DispatchThreadID) { dst.Store(id.x * 4, src_uav.Load(id.x * 4)); }
[RootSignature(RS)] [numthreads(64,1,1)]
void copy_srv(uint3 id : SV_DispatchThreadID) { dst.Store(id.x * 4, src_srv.Load(id.x * 4)); }
"#;

fn pipeline(gpu: &Gpu, entry: PCSTR) -> Result<(ID3D12RootSignature, ID3D12PipelineState)> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and its length; the blobs live while used.
    unsafe {
        let made = D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            None,
            None,
            None,
            entry,
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        );
        if let (Err(_), Some(e)) = (&made, &errors) {
            println!("{}", String::from_utf8_lossy(std::slice::from_raw_parts(e.GetBufferPointer().cast::<u8>(), e.GetBufferSize())));
        }
        made?;
        let code = code.unwrap();
        let bytes = std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize());
        let root: ID3D12RootSignature = gpu.device.CreateRootSignature(0, bytes)?;
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() },
            ..Default::default()
        };
        let state: ID3D12PipelineState = gpu.device.CreateComputePipelineState(&desc)?;
        Ok((root, state))
    }
}

/// The same aliasing through shaders; then one dispatch that reads the buffer
/// as an SRV and writes it as a UAV, which legacy resource states forbid.
fn alias_shaders(gpu: &mut Gpu, r: &ID3D12Resource) -> Result<()> {
    const ROUNDS: u32 = 64;
    const WORDS: u32 = (TILE / 4) as u32;
    let heap = gpu.heap(4 * TILE)?;
    let back = gpu.buffer(TILE, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE)?;
    let out = gpu.buffer(TILE, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)?;
    let p_back = mapped(&back);
    let (a, b, c) = (100u32, 200_000u32, 300_000u32);
    gpu.map_tiles(r, a, &heap, 0, 1);
    gpu.map_tiles(r, b, &heap, 0, 1);
    gpu.map_tiles(r, c, &heap, 1, 1);
    let make = gpu_pipelines(gpu)?;
    // SAFETY: live resources.
    let (base, out_at) = unsafe { (r.GetGPUVirtualAddress(), out.GetGPUVirtualAddress()) };
    let at = |tile: u32| base + u64::from(tile) * TILE;

    for (name, srv_read) in [("UAV read at B", false), ("SRV read at B + UAV write at C, one buffer, one dispatch", true)] {
        let mut good = 0;
        for seed in 0..ROUNDS {
            let reader = if srv_read { &make[2] } else { &make[1] };
            gpu.run(|l| unsafe {
                l.ResourceBarrier(&[
                    aliasing(),
                    transition(r, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
                    transition(&out, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
                ]);
                // Write the tile through address A.
                l.SetComputeRootSignature(&make[0].0);
                l.SetPipelineState(&make[0].1);
                l.SetComputeRoot32BitConstant(0, seed, 0);
                l.SetComputeRootUnorderedAccessView(3, at(a));
                l.Dispatch(WORDS / 64, 1, 1);
                l.ResourceBarrier(&[uav_barrier(), aliasing()]);
                // Read it through address B: into `out`, or into tile C of
                // the same buffer when reading as an SRV.
                l.SetComputeRootSignature(&reader.0);
                l.SetPipelineState(&reader.1);
                l.SetComputeRootShaderResourceView(1, at(b));
                l.SetComputeRootUnorderedAccessView(2, at(b));
                l.SetComputeRootUnorderedAccessView(3, if srv_read { at(c) } else { out_at });
                l.Dispatch(WORDS / 64, 1, 1);
                l.ResourceBarrier(&[
                    uav_barrier(),
                    aliasing(),
                    transition(r, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE),
                    transition(&out, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE),
                ]);
                if srv_read {
                    l.CopyBufferRegion(&back, 0, r, u64::from(c) * TILE, TILE);
                } else {
                    l.CopyBufferRegion(&back, 0, &out, 0, TILE);
                }
                l.ResourceBarrier(&[
                    transition(r, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON),
                    transition(&out, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON),
                ]);
            })?;
            // SAFETY: the copy finished.
            let got = unsafe { std::slice::from_raw_parts(p_back.cast::<u32>(), WORDS as usize) };
            let ok = got
                .iter()
                .enumerate()
                .all(|(i, &v)| v == (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(97_531)));
            good += u32::from(ok);
        }
        println!("shader write at A, {name}: {good} of {ROUNDS} identical");
    }
    Ok(())
}

fn gpu_pipelines(gpu: &Gpu) -> Result<[(ID3D12RootSignature, ID3D12PipelineState); 3]> {
    Ok([pipeline(gpu, s!("gen"))?, pipeline(gpu, s!("copy_uav"))?, pipeline(gpu, s!("copy_srv"))?])
}

fn mapping_cost(gpu: &mut Gpu, r: &ID3D12Resource) -> Result<()> {
    const HEAP_TILES: u32 = 1024;
    let heap = gpu.heap(u64::from(HEAP_TILES) * TILE)?;
    for n in [8u32, 1000, 9000] {
        // One call per tile, scattered addresses: how the title remaps.
        let mut calls = Vec::new();
        let mut total = Vec::new();
        for round in 0..5u32 {
            gpu.wait()?;
            let t = Instant::now();
            for k in 0..n {
                gpu.map_tiles(r, 1000 + k * 7 + round, &heap, (k * 13 + round) % HEAP_TILES, 1);
            }
            calls.push(t.elapsed().as_secs_f64() * 1e3);
            gpu.wait()?;
            total.push(t.elapsed().as_secs_f64() * 1e3);
        }
        // The same tiles in one call: n regions, n ranges.
        let mut batched = Vec::new();
        for round in 0..5u32 {
            let starts: Vec<_> =
                (0..n).map(|k| D3D12_TILED_RESOURCE_COORDINATE { X: 1000 + k * 7 + round, ..Default::default() }).collect();
            let sizes = vec![D3D12_TILE_REGION_SIZE { NumTiles: 1, ..Default::default() }; n as usize];
            let flags = vec![D3D12_TILE_RANGE_FLAG_NONE; n as usize];
            let froms: Vec<u32> = (0..n).map(|k| (k * 11 + round) % HEAP_TILES).collect();
            let counts = vec![1u32; n as usize];
            gpu.wait()?;
            let t = Instant::now();
            // SAFETY: n regions and n ranges, arrays of n elements.
            unsafe {
                gpu.queue.UpdateTileMappings(
                    r,
                    n,
                    Some(starts.as_ptr()),
                    Some(sizes.as_ptr()),
                    &heap,
                    n,
                    Some(flags.as_ptr()),
                    Some(froms.as_ptr()),
                    Some(counts.as_ptr()),
                    D3D12_TILE_MAPPING_FLAG_NONE,
                );
            }
            gpu.wait()?;
            batched.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "remap {n:5} tiles: one call each {:.2} ms in the calls, {:.2} ms until done; one batched call {:.2} ms until done",
            med(&mut calls),
            med(&mut total),
            med(&mut batched)
        );
        check_headroom();
    }
    Ok(())
}

/// Neighbouring tiles mapped as one region, in runs of `run` tiles.
fn run_cost(gpu: &mut Gpu, r: &ID3D12Resource) -> Result<()> {
    const HEAP_TILES: u32 = 1024;
    let heap = gpu.heap(u64::from(HEAP_TILES) * TILE)?;
    for run in [1u32, 16, 256] {
        let n = 9216 / run;
        let mut total = Vec::new();
        for round in 0..5u32 {
            gpu.wait()?;
            let t = Instant::now();
            for k in 0..n {
                gpu.map_tiles(r, 100_000 + k * (run + 3) + round, &heap, (k * run + round) % (HEAP_TILES - run), run);
            }
            gpu.wait()?;
            total.push(t.elapsed().as_secs_f64() * 1e3);
        }
        total.sort_by(f64::total_cmp);
        println!("remap 9216 tiles as {n:5} regions of {run:3} neighbouring tiles, one call each: {:.1} ms until done", total[2]);
    }
    Ok(())
}

/// Fetching many 64 KiB blocks in one submission, and what the copy to
/// where the title reads them then costs.
fn batched_fetch(gpu: &mut Gpu) -> Result<()> {
    const MOST: u64 = 848; // about 53 MiB: what one frame applied in an earlier run
    let vram = gpu.buffer(MOST * 2 * TILE, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_NONE)?;
    let back = gpu.buffer(MOST * TILE, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE)?;
    let p = mapped(&back);
    let mut to = vec![1u8; (MOST * TILE) as usize];
    for blocks in [1u64, 16, 128, MOST] {
        let (mut card, mut ram) = (Vec::new(), Vec::new());
        for _ in 0..20 {
            let t = Instant::now();
            // Every other block, so that each is a copy of its own.
            gpu.run(|l| unsafe {
                for k in 0..blocks {
                    l.CopyBufferRegion(&back, k * TILE, &vram, k * 2 * TILE, TILE);
                }
            })?;
            card.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            // SAFETY: the copies finished; both sides hold `blocks` tiles.
            unsafe { std::ptr::copy_nonoverlapping(p, to.as_mut_ptr(), (blocks * TILE) as usize) };
            ram.push(t.elapsed().as_secs_f64() * 1e3);
        }
        card.sort_by(f64::total_cmp);
        ram.sort_by(f64::total_cmp);
        let all = card[10] + ram[10];
        println!(
            "fetch {blocks:4} blocks in one submission: card to readback {:.2} ms, readback to RAM (1 thread) {:.2} ms, \
             total {:.2} ms = {:.1} us a block",
            card[10],
            ram[10],
            all,
            all * 1e3 / blocks as f64
        );
        check_headroom();
    }
    println!("check byte: {}", to[0]);
    Ok(())
}

static FAULTS: AtomicU64 = AtomicU64::new(0);
static REGION: AtomicUsize = AtomicUsize::new(0);
static REGION_LEN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn on_fault(info: *mut EXCEPTION_POINTERS) -> i32 {
    const ACCESS_VIOLATION: u32 = 0xC000_0005;
    // SAFETY: the system passes the exception being dispatched.
    let record = unsafe { &*(*info).ExceptionRecord };
    if record.ExceptionCode.0 as u32 != ACCESS_VIOLATION || record.NumberParameters < 2 {
        return 0;
    }
    let at = record.ExceptionInformation[1];
    let start = REGION.load(Ordering::Relaxed);
    if at < start || at >= start + REGION_LEN.load(Ordering::Relaxed) {
        return 0;
    }
    let page = at & !(TILE as usize - 1);
    let mut old = PAGE_PROTECTION_FLAGS::default();
    // SAFETY: a block of our region.
    if unsafe { VirtualProtect(page as *const c_void, TILE as usize, PAGE_READWRITE, &mut old) }.is_err() {
        return 0;
    }
    FAULTS.fetch_add(1, Ordering::Relaxed);
    -1 // continue execution
}

fn cpu_faults(gpu: &mut Gpu) -> Result<()> {
    const BLOCKS: usize = 2048;
    let len = BLOCKS * TILE as usize;
    // SAFETY: a fresh allocation, released below.
    let base = unsafe { VirtualAlloc(None, len, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) } as usize;
    assert_ne!(base, 0);
    // Touch everything first, so no first-touch faults are timed.
    // SAFETY: our allocation.
    unsafe { std::ptr::write_bytes(base as *mut u8, 1, len) };
    REGION.store(base, Ordering::Relaxed);
    REGION_LEN.store(len, Ordering::Relaxed);
    // SAFETY: the handler only acts on faults inside our region.
    unsafe { AddVectoredExceptionHandler(1, Some(on_fault)) };

    let t = Instant::now();
    for k in 0..BLOCKS {
        let mut old = PAGE_PROTECTION_FLAGS::default();
        // SAFETY: a block of our allocation.
        unsafe { VirtualProtect((base + k * TILE as usize) as *const c_void, TILE as usize, PAGE_NOACCESS, &mut old)? };
    }
    println!("CPU side, protect a 64 KiB block: {:.2} us each", t.elapsed().as_secs_f64() * 1e6 / BLOCKS as f64);
    let t = Instant::now();
    let mut sum = 0u64;
    for k in 0..BLOCKS {
        // SAFETY: inside our allocation; the handler opens the block.
        sum += u64::from(unsafe { std::ptr::read_volatile((base + k * TILE as usize) as *const u8) });
    }
    println!(
        "CPU side, a read that faults and is let through: {:.2} us each ({} faults, check {sum})",
        t.elapsed().as_secs_f64() * 1e6 / BLOCKS as f64,
        FAULTS.load(Ordering::Relaxed)
    );

    // What a fault would then wait for: 64 KiB out of video memory, with an
    // otherwise idle card.
    let vram = gpu.buffer(TILE, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_NONE)?;
    let back = gpu.buffer(TILE, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE)?;
    let p = mapped(&back);
    let mut times = Vec::new();
    for _ in 0..400 {
        let t = Instant::now();
        gpu.run(|l| unsafe { l.CopyBufferRegion(&back, 0, &vram, 0, TILE) })?;
        // SAFETY: the copy finished; both sides hold a tile.
        unsafe { std::ptr::copy_nonoverlapping(p, base as *mut u8, TILE as usize) };
        times.push(t.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "fetch 64 KiB from video memory, submit to bytes in RAM, idle card: median {:.0} us, 95th percentile {:.0} us",
        times[times.len() / 2],
        times[times.len() * 95 / 100]
    );
    // SAFETY: our allocation; nothing reads it after this.
    unsafe { VirtualFree(base as *mut c_void, 0, MEM_RELEASE)? };
    Ok(())
}

fn main() -> Result<()> {
    check_headroom();
    let mut gpu = Gpu::new()?;
    println!("== capabilities");
    capabilities(&gpu)?;

    println!("== reserved buffers (address space only, no memory)");
    let mut mirror = None;
    for gib in [1u64, 6, 24, 32] {
        match gpu.reserved(gib * GIB) {
            Ok(r) => {
                println!("{gib:3} GiB: created");
                if gib == 24 {
                    mirror = Some(r);
                }
            }
            Err(e) => println!("{gib:3} GiB: refused ({e})"),
        }
    }
    let Some(mirror) = mirror else {
        println!("no 24 GiB mirror: stopping");
        return Ok(());
    };

    println!("== one video-memory tile at two addresses (copies)");
    alias_copies(&mut gpu, &mirror)?;
    println!("== one video-memory tile at two addresses (shaders)");
    alias_shaders(&mut gpu, &mirror)?;
    println!("== cost of remapping");
    mapping_cost(&mut gpu, &mirror)?;
    println!("== cost of remapping runs");
    run_cost(&mut gpu, &mirror)?;
    println!("== demand fetching");
    cpu_faults(&mut gpu)?;
    println!("== batched fetching");
    batched_fetch(&mut gpu)?;
    println!("device still there: {}", unsafe { gpu.device.GetDeviceRemovedReason() }.is_ok());
    Ok(())
}

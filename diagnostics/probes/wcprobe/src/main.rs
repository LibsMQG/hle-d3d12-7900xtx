// Author: libs (hle-d3d12-7900xtx)
//! wcprobe: which D3D12 objects make the driver hold write-combined memory.
//!
//! No game. One device on the default adapter; each phase makes one kind of
//! object as VIctory does in the loading, then the process's own address
//! space is walked (VirtualQuery) for write-combined memory: committed
//! bytes, blocks by reserved size, the 32 MB ones apart. Nothing is written
//! outside this process; no setting is touched.
//!
//! Usage: wcprobe [--lists N] [--commands K] [--textures N] [--uploads N]

use std::collections::BTreeMap;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::Interface;

const MB: f64 = (1u64 << 20) as f64;

/// Write-combined memory of this process: committed bytes, and blocks by
/// reserved size (size -> (count, committed)).
fn wc() -> (u64, BTreeMap<u64, (u64, u64)>) {
    let mut blocks: BTreeMap<usize, (u64, u64)> = BTreeMap::new(); // base -> (reserved, committed)
    let mut at = 0usize;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: a query of our own address space into a local structure.
        let got = unsafe { VirtualQuery(Some(at as *const _), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize == 0 {
            break;
        }
        let base = info.AllocationBase as usize;
        let flagged = info.AllocationProtect.0 & PAGE_WRITECOMBINE.0 != 0 || info.Protect.0 & PAGE_WRITECOMBINE.0 != 0;
        if info.State != MEM_FREE && flagged && base != 0 {
            let b = blocks.entry(base).or_default();
            b.0 += info.RegionSize as u64;
            if info.State == MEM_COMMIT {
                b.1 += info.RegionSize as u64;
            }
        }
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    let mut by: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    let mut total = 0;
    for (reserved, committed) in blocks.values() {
        let e = by.entry(*reserved).or_default();
        e.0 += 1;
        e.1 += committed;
        total += committed;
    }
    (total, by)
}

/// Bytes of this process's committed write-combined memory in RAM (in the
/// working set), page by page.
fn wc_resident() -> u64 {
    use windows::Win32::System::ProcessStatus::{PSAPI_WORKING_SET_EX_INFORMATION, QueryWorkingSetEx};
    let mut resident = 0u64;
    let mut at = 0usize;
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: as in `wc`.
        let got = unsafe { VirtualQuery(Some(at as *const _), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize == 0 {
            break;
        }
        if info.State == MEM_COMMIT && info.Protect.0 & PAGE_WRITECOMBINE.0 != 0 {
            let pages = info.RegionSize / 4096;
            for first in (0..pages).step_by(1024) {
                let n = 1024.min(pages - first);
                let mut e: Vec<PSAPI_WORKING_SET_EX_INFORMATION> = (0..n)
                    .map(|k| PSAPI_WORKING_SET_EX_INFORMATION {
                        VirtualAddress: (info.BaseAddress as usize + (first + k) * 4096) as *mut _,
                        ..Default::default()
                    })
                    .collect();
                // SAFETY: the byte length matches the vector.
                let ok = unsafe {
                    QueryWorkingSetEx(GetCurrentProcess(), e.as_mut_ptr().cast(), (n * size_of::<PSAPI_WORKING_SET_EX_INFORMATION>()) as u32)
                };
                if ok.is_ok() {
                    // SAFETY: the union is a bit field over one integer.
                    resident += e.iter().filter(|x| unsafe { x.VirtualAttributes.Flags } & 1 != 0).count() as u64 * 4096;
                }
            }
        }
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    resident
}

fn private_commit() -> u64 {
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    c.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: a structure of the size given.
    let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut c).cast(), c.cb) };
    c.PrivateUsage as u64
}

struct Meter {
    last: (u64, u64),
}

impl Meter {
    fn say(&mut self, what: &str) {
        let (total, by) = wc();
        let private = private_commit();
        let b32 = by.get(&(32 << 20)).map_or(0, |b| b.0);
        let top: Vec<String> = {
            let mut v: Vec<_> = by.iter().collect();
            v.sort_by_key(|(_, (_, c))| std::cmp::Reverse(*c));
            v.iter().take(4).map(|(s, (n, c))| format!("{n} x {:.2} MB ({:.0} MB)", **s as f64 / MB, *c as f64 / MB)).collect()
        };
        println!(
            "{what:<58} WC {:>8.1} MB ({:+8.1}), in RAM {:>7.1}  32MB blocks {b32:>4}  private {:>8.1} MB ({:+8.1})  [{}]",
            total as f64 / MB,
            (total as f64 - self.last.0 as f64) / MB,
            wc_resident() as f64 / MB,
            private as f64 / MB,
            (private as f64 - self.last.1 as f64) / MB,
            top.join(", ")
        );
        self.last = (total, private);
    }
}

fn arg(name: &str, default: usize) -> usize {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn buffer(device: &ID3D12Device, kind: D3D12_HEAP_TYPE, bytes: u64, state: D3D12_RESOURCE_STATES) -> windows::core::Result<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes.max(1),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    // SAFETY: plain creation.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut r)? };
    r.ok_or_else(windows::core::Error::empty)
}

fn main() -> windows::core::Result<()> {
    let lists_n = arg("--lists", 760);
    let commands = arg("--commands", 20_000);
    let textures_n = arg("--textures", 4_000);
    let uploads_n = arg("--uploads", 50_000);
    let mut m = Meter { last: (0, 0) };
    m.say("start (no device)");
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: plain device creation on the default adapter.
    unsafe { D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device)? };
    let device = device.ok_or_else(windows::core::Error::empty)?;
    // SAFETY: plain queue and fence creation.
    let (queue, fence): (ID3D12CommandQueue, ID3D12Fence) = unsafe {
        (
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })?,
            device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?,
        )
    };
    let mut value = 0u64;
    let mut wait = |queue: &ID3D12CommandQueue| -> windows::core::Result<()> {
        value += 1;
        // SAFETY: our queue and fence; a null event blocks until reached.
        unsafe {
            queue.Signal(&fence, value)?;
            fence.SetEventOnCompletion(value, HANDLE::default())?;
        }
        Ok(())
    };
    m.say("device, queue, fence");

    if std::env::args().any(|a| a == "--split") {
        // What one allocator and one list cost apart.
        let n = lists_n;
        let allocs: Vec<ID3D12CommandAllocator> = (0..n)
            // SAFETY: plain creation.
            .map(|_| unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) })
            .collect::<windows::core::Result<_>>()?;
        m.say(&format!("{n} allocators alone"));
        let mut lone: Vec<ID3D12GraphicsCommandList> = Vec::new();
        for _ in 0..n {
            // SAFETY: plain creation on one allocator, each closed before the next.
            unsafe {
                let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocs[0], None)?;
                list.Close()?;
                lone.push(list);
            }
        }
        m.say(&format!("{n} lists, all on one allocator"));
        // One allocator after a big recording, then reset: does it shrink?
        let a = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, 1 << 20, D3D12_RESOURCE_STATE_COMMON)?;
        let b = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, 1 << 20, D3D12_RESOURCE_STATE_COMMON)?;
        // SAFETY: our allocator and list; waited for before the reset.
        unsafe {
            allocs[1].Reset()?;
            lone[0].Reset(&allocs[1], None)?;
            for k in 0..400_000u64 {
                lone[0].CopyBufferRegion(&a, (k * 256) % (1 << 20), &b, (k * 256) % (1 << 20), 256);
            }
            lone[0].Close()?;
            queue.ExecuteCommandLists(&[Some(lone[0].cast::<ID3D12CommandList>()?)]);
        }
        wait(&queue)?;
        m.say("one allocator after 400,000 copies");
        // SAFETY: the card is done with it.
        unsafe { allocs[1].Reset()? };
        m.say("that allocator reset");
        lone.clear();
        m.say("lists released");
        drop(allocs);
        m.say("allocators released");
        return Ok(());
    }

    // 1. Shader-visible view heaps of 2^21 views, as VIctory's.
    let heaps: Vec<ID3D12DescriptorHeap> = (0..3)
        .map(|_| {
            // SAFETY: plain heap creation.
            unsafe {
                device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: 1 << 21,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })
            }
        })
        .collect::<windows::core::Result<_>>()?;
    m.say("3 shader-visible view heaps of 2^21");

    // 2. Command allocators and lists, recorded, run, reset, recorded again.
    let a = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, 1 << 20, D3D12_RESOURCE_STATE_COMMON)?;
    let b = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, 1 << 20, D3D12_RESOURCE_STATE_COMMON)?;
    let mut lists: Vec<(ID3D12CommandAllocator, ID3D12GraphicsCommandList)> = Vec::new();
    for _ in 0..lists_n {
        // SAFETY: plain creation; the list starts open.
        unsafe {
            let alloc: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)?;
            list.Close()?;
            lists.push((alloc, list));
        }
    }
    m.say(&format!("{lists_n} allocators + lists, empty"));
    for round in 0..3 {
        for (i, (alloc, list)) in lists.iter().enumerate() {
            // SAFETY: the allocator's lists have finished (waited for below).
            unsafe {
                alloc.Reset()?;
                list.Reset(alloc, None)?;
                // Round 0 heavy for all; later rounds heavy for a few only, as
                // a loading's lists then a heavy scene's.
                let n = if round == 0 || i % 16 == 0 { commands } else { commands / 50 };
                for k in 0..n {
                    let at = ((k * 256) % (1 << 20)) as u64;
                    list.CopyBufferRegion(&a, at, &b, at, 256);
                }
                list.Close()?;
                queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>()?)]);
            }
            if i % 64 == 63 {
                wait(&queue)?;
            }
        }
        wait(&queue)?;
        m.say(&format!("lists recorded, round {round} ({commands} copies {})", if round == 0 { "each" } else { "in 1/16" }));
    }
    lists.truncate(lists_n / 2);
    m.say("half of the lists released");
    lists.clear();
    m.say("all lists released");

    // 3. Textures in video memory, made and released.
    let mut textures = Vec::new();
    for i in 0..textures_n {
        let side = [256u64, 512, 1024][i % 3];
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: side,
            Height: side as u32,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let mut r: Option<ID3D12Resource> = None;
        // SAFETY: plain creation.
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut r)? };
        textures.push(r);
    }
    m.say(&format!("{textures_n} textures in video memory"));
    // Contents copied into a third of them from an upload buffer, as uploads do.
    let up = buffer(&device, D3D12_HEAP_TYPE_UPLOAD, 4 << 20, D3D12_RESOURCE_STATE_GENERIC_READ)?;
    // SAFETY: plain creation; the list starts open.
    let (alloc, list): (ID3D12CommandAllocator, ID3D12GraphicsCommandList) = unsafe {
        let alloc: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let list = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)?;
        (alloc, list)
    };
    for (i, t) in textures.iter().enumerate().filter(|(i, _)| i % 3 == 0) {
        let Some(t) = t else { continue };
        let side = [256u32, 512, 1024][i % 3];
        let to = D3D12_TEXTURE_COPY_LOCATION {
            // SAFETY: borrowed pointers the locations never release.
            pResource: unsafe { std::mem::transmute_copy(t) },
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        let from = D3D12_TEXTURE_COPY_LOCATION {
            // SAFETY: as above.
            pResource: unsafe { std::mem::transmute_copy(&up) },
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        Width: side,
                        Height: side,
                        Depth: 1,
                        RowPitch: side * 4,
                    },
                },
            },
        };
        // SAFETY: an open list; live resources (COMMON textures promote to COPY_DEST).
        unsafe { list.CopyTextureRegion(&to, 0, 0, 0, &from, None) };
    }
    // SAFETY: our list and queue.
    unsafe {
        list.Close()?;
        queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>()?)]);
    }
    wait(&queue)?;
    m.say("a third of the textures filled by copies");
    drop((list, alloc, up));
    textures.clear();
    m.say("textures released");

    // 4. Upload buffers made and freed, sizes as the loading's staging.
    let sizes = [15u64 << 10, 47 << 10, 87 << 10, 175 << 10, 343 << 10, 687 << 10, 1367 << 10, 3 << 20];
    let mut held = std::collections::VecDeque::new();
    for i in 0..uploads_n {
        let s = sizes[(i * 7 + i / 13) % sizes.len()];
        held.push_back(buffer(&device, D3D12_HEAP_TYPE_UPLOAD, s, D3D12_RESOURCE_STATE_GENERIC_READ)?);
        if held.len() > 200 {
            held.pop_front();
        }
    }
    m.say(&format!("{uploads_n} upload buffers made and freed (200 kept)"));
    held.clear();
    m.say("all upload buffers freed");
    drop(heaps);
    m.say("view heaps released");
    Ok(())
}

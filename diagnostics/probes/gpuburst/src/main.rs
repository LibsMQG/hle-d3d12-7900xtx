// Author: libs (hle-d3d12-amd)
//! gpuburst: how the card's clock, and the time of a fixed piece of work,
//! depend on how full the card is. It runs short compute work back to back,
//! then with gaps between the pieces (as VIctory's submissions have them), and
//! then with gaps while a little endless work keeps the card from looking
//! idle. It changes no setting: it reads the clock through AMD's own library
//! (`atiadlxx.dll`) and measures its own work with the card's timestamps.
//!
//! cargo run --release --offline -- [seconds per phase]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{s, w, Interface, Result, PCSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_FEATURE_LEVEL_12_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

// ---- The clock, through AMD's library -------------------------------------------------------

#[repr(C)]
struct Log {
    size: i32,
    sensors: [[i32; 2]; 256],
}

extern "system" fn adl_alloc(n: i32) -> *mut c_void {
    let mut v = vec![0u8; n.max(1) as usize];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p.cast()
}

struct Adl {
    ctx: *mut c_void,
    query: unsafe extern "system" fn(*mut c_void, i32, *mut Log) -> i32,
}

#[derive(Clone, Copy)]
struct Reading {
    at: Instant,
    gfx: i32,
    mem: i32,
    activity: i32,
    watts: i32,
}

impl Adl {
    fn new() -> Option<Adl> {
        // SAFETY: the library is AMD's own; the functions have the shapes its
        // public header gives.
        unsafe {
            let lib = LoadLibraryW(w!("atiadlxx.dll")).ok()?;
            let create = GetProcAddress(lib, s!("ADL2_Main_Control_Create"))?;
            let query = GetProcAddress(lib, s!("ADL2_New_QueryPMLogData_Get"))?;
            let create: unsafe extern "system" fn(extern "system" fn(i32) -> *mut c_void, i32, *mut *mut c_void) -> i32 =
                std::mem::transmute(create);
            let mut ctx = std::ptr::null_mut();
            if create(adl_alloc, 1, &mut ctx) != 0 {
                return None;
            }
            Some(Adl { ctx, query: std::mem::transmute::<_, unsafe extern "system" fn(*mut c_void, i32, *mut Log) -> i32>(query) })
        }
    }

    fn read(&self) -> Option<Reading> {
        let mut log = Box::new(Log { size: size_of::<Log>() as i32, sensors: [[0; 2]; 256] });
        // SAFETY: a live context, adapter 0 (the only AMD card here), our struct.
        if unsafe { (self.query)(self.ctx, 0, &mut *log) } != 0 {
            return None;
        }
        let v = |i: usize| log.sensors[i][1];
        Some(Reading { at: Instant::now(), gfx: v(1), mem: v(2), activity: v(19), watts: v(73) })
    }
}

// ---- The card --------------------------------------------------------------------------------

const ALU: &str = r#"
#define RS "RootConstants(num32BitConstants=2, b0), UAV(u0), SRV(t0)"
cbuffer C : register(b0) { uint n; uint mask; };
RWByteAddressBuffer o : register(u0);
ByteAddressBuffer s : register(t0);
[RootSignature(RS)]
[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    float4 x = float4(id.x * 0.001, id.x * 0.37, 1.25, 2.5);
    [loop] for (uint i = 0; i < n; i++) { x = mad(x, x.yzwx, 0.7); x = frac(x); }
    o.Store4((id.x & 4095u) * 16u, asuint(x));
}
"#;

const MEM: &str = r#"
#define RS "RootConstants(num32BitConstants=2, b0), UAV(u0), SRV(t0)"
cbuffer C : register(b0) { uint n; uint mask; };
RWByteAddressBuffer o : register(u0);
ByteAddressBuffer s : register(t0);
[RootSignature(RS)]
[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint x = id.x * 2654435761u + 12345u;
    uint4 sum = 0;
    [loop] for (uint i = 0; i < n; i++) {
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        sum += s.Load4((x & mask) << 4);
    }
    o.Store4((id.x & 4095u) * 16u, sum);
}
"#;

struct Card {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    value: u64,
    frequency: f64,
}

fn buffer(device: &ID3D12Device, size: u64, kind: D3D12_HEAP_TYPE, uav: bool) -> Result<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: if uav { D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS } else { D3D12_RESOURCE_FLAG_NONE },
        ..Default::default()
    };
    let state = if kind == D3D12_HEAP_TYPE_READBACK { D3D12_RESOURCE_STATE_COPY_DEST } else { D3D12_RESOURCE_STATE_COMMON };
    let mut made: Option<ID3D12Resource> = None;
    // SAFETY: plain object creation.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut made)? };
    made.ok_or_else(windows::core::Error::empty)
}

fn pipeline(device: &ID3D12Device, source: &str) -> Result<(ID3D12RootSignature, ID3D12PipelineState)> {
    let mut code: Option<ID3DBlob> = None;
    // SAFETY: the source lives through the call; the root signature is in the shader.
    unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            None,
        )?;
        let code = code.ok_or_else(windows::core::Error::empty)?;
        let bytes = std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pso: ID3D12PipelineState = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() },
            ..Default::default()
        })?;
        Ok((root, pso))
    }
}

/// A list that does one unit of work between two timestamps.
struct Unit {
    list: ID3D12CommandList,
    _allocator: ID3D12CommandAllocator,
    slot: u32,
}

struct Work {
    units: Vec<Unit>,
    readback: ID3D12Resource,
    data: *const u64,
    _heap: ID3D12QueryHeap,
    _o: ID3D12Resource,
    _s: ID3D12Resource,
    _pipe: (ID3D12RootSignature, ID3D12PipelineState),
}

const RING: usize = 32;

impl Card {
    fn new() -> Result<Card> {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: plain object creation.
        unsafe {
            D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device)?;
            let device = device.ok_or_else(windows::core::Error::empty)?;
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })?;
            let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let frequency = queue.GetTimestampFrequency()? as f64;
            Ok(Card { device, queue, fence, value: 0, frequency })
        }
    }

    fn send(&mut self, list: &ID3D12CommandList) -> Result<u64> {
        self.value += 1;
        // SAFETY: a closed list; our queue and fence.
        unsafe {
            self.queue.ExecuteCommandLists(&[Some(list.clone())]);
            self.queue.Signal(&self.fence, self.value)?;
        }
        Ok(self.value)
    }

    fn wait(&self, value: u64) -> Result<()> {
        // SAFETY: a live fence; with no event the call waits.
        unsafe {
            if self.fence.GetCompletedValue() < value {
                self.fence.SetEventOnCompletion(value, HANDLE::default())?;
            }
        }
        Ok(())
    }

    fn work(&mut self, source: &str, groups: u32, n: u32, mask: u32, shared: &ID3D12Resource) -> Result<Work> {
        let pipe = pipeline(&self.device, source)?;
        let o = buffer(&self.device, 64 << 10, D3D12_HEAP_TYPE_DEFAULT, true)?;
        let readback = buffer(&self.device, (RING * 16) as u64, D3D12_HEAP_TYPE_READBACK, false)?;
        let mut data = std::ptr::null_mut();
        let mut heap: Option<ID3D12QueryHeap> = None;
        // SAFETY: a fresh readback buffer stays mapped; plain creation.
        unsafe {
            readback.Map(0, None, Some(&mut data))?;
            self.device.CreateQueryHeap(
                &D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: (RING * 2) as u32, NodeMask: 0 },
                &mut heap,
            )?;
        }
        let heap = heap.ok_or_else(windows::core::Error::empty)?;
        let mut units = Vec::new();
        for slot in 0..RING as u32 {
            // SAFETY: lists recorded once and closed, executed only after
            // their last run ended (their fence value was waited for).
            let unit = unsafe {
                let allocator: ID3D12CommandAllocator = self.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
                let list: ID3D12GraphicsCommandList =
                    self.device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
                let barrier = D3D12_RESOURCE_BARRIER {
                    Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                    Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                    Anonymous: D3D12_RESOURCE_BARRIER_0 {
                        UAV: std::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER { pResource: std::mem::ManuallyDrop::new(None) }),
                    },
                };
                list.ResourceBarrier(&[barrier]);
                list.EndQuery(&heap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 2);
                list.SetComputeRootSignature(&pipe.0);
                list.SetPipelineState(&pipe.1);
                list.SetComputeRoot32BitConstants(0, 2, [n, mask].as_ptr().cast(), 0);
                list.SetComputeRootUnorderedAccessView(1, o.GetGPUVirtualAddress());
                list.SetComputeRootShaderResourceView(2, shared.GetGPUVirtualAddress());
                list.Dispatch(groups, 1, 1);
                list.EndQuery(&heap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 2 + 1);
                list.ResolveQueryData(&heap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 2, 2, &readback, u64::from(slot) * 16);
                list.Close()?;
                Unit { list: list.cast()?, _allocator: allocator, slot }
            };
            units.push(unit);
        }
        Ok(Work { units, readback, data: data.cast(), _heap: heap, _o: o, _s: shared.clone(), _pipe: pipe })
    }
}

impl Work {
    /// The milliseconds unit `slot` took, its last run being over.
    fn took(&self, slot: u32, frequency: f64) -> f64 {
        // SAFETY: inside the mapping; the resolve into it is done (waited for).
        let (a, b) = unsafe {
            let p = self.data.add(slot as usize * 2);
            (p.read_volatile(), p.add(1).read_volatile())
        };
        let _ = &self.readback;
        b.saturating_sub(a) as f64 * 1e3 / frequency
    }
}

// ---- The phases ------------------------------------------------------------------------------

/// Work on another queue that keeps the card from looking idle: one group at
/// a time, endless.
fn keep_busy(device: &ID3D12Device, kind: D3D12_COMMAND_LIST_TYPE, stop: Arc<AtomicBool>, shared: &ID3D12Resource) -> Result<std::thread::JoinHandle<()>> {
    let device = device.clone();
    let shared = shared.clone();
    struct Held(ID3D12Device, ID3D12Resource);
    // SAFETY: D3D12 objects may be used from any thread.
    unsafe impl Send for Held {}
    let held = Held(device, shared);
    Ok(std::thread::spawn(move || {
        let held = held;
        let Held(device, shared) = held;
        let run = || -> Result<()> {
            // SAFETY: plain objects of this thread; lists reused after their
            // fence value was passed.
            unsafe {
                let queue: ID3D12CommandQueue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: kind, ..Default::default() })?;
                let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
                let pipe = pipeline(&device, ALU)?;
                let o = buffer(&device, 64 << 10, D3D12_HEAP_TYPE_DEFAULT, true)?;
                let mut lists = Vec::new();
                for _ in 0..8 {
                    let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(kind)?;
                    let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, kind, &allocator, None)?;
                    list.SetComputeRootSignature(&pipe.0);
                    list.SetPipelineState(&pipe.1);
                    list.SetComputeRoot32BitConstants(0, 2, [60_000u32, 0].as_ptr().cast(), 0);
                    list.SetComputeRootUnorderedAccessView(1, o.GetGPUVirtualAddress());
                    list.SetComputeRootShaderResourceView(2, shared.GetGPUVirtualAddress());
                    list.Dispatch(1, 1, 1);
                    list.Close()?;
                    lists.push((allocator, list.cast::<ID3D12CommandList>()?));
                }
                let mut value = 0u64;
                let mut sent: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
                while !stop.load(Ordering::Relaxed) {
                    if sent.len() >= 4 {
                        let v = sent.pop_front().unwrap();
                        if fence.GetCompletedValue() < v {
                            fence.SetEventOnCompletion(v, HANDLE::default())?;
                        }
                    }
                    value += 1;
                    let i = (value % 8) as usize;
                    // A list may run again only once its last run is over.
                    if value > 8 && fence.GetCompletedValue() + 8 < value {
                        fence.SetEventOnCompletion(value - 8, HANDLE::default())?;
                    }
                    queue.ExecuteCommandLists(&[Some(lists[i].1.clone())]);
                    queue.Signal(&fence, value)?;
                    sent.push_back(value);
                }
                if fence.GetCompletedValue() < value {
                    fence.SetEventOnCompletion(value, HANDLE::default())?;
                }
                Ok(())
            }
        };
        if let Err(e) = run() {
            eprintln!("keep-busy queue: {e}");
        }
    }))
}

#[derive(Clone, Copy)]
enum Gap {
    /// Pieces back to back, several in flight.
    None,
    /// Pieces one at a time, `busy` of the time.
    Share(f64),
    /// Twenty pieces, then a gap as long.
    Burst,
}

struct Result_ {
    unit: f64,
    gfx: (i32, i32, i32),
    mem: i32,
    activity: i32,
    watts: i32,
    samples: usize,
}

fn phase(card: &mut Card, work: &Work, gap: Gap, seconds: f64, readings: &Mutex<Vec<Reading>>) -> Result<Result_> {
    let began = Instant::now();
    let settle = Duration::from_secs_f64(seconds * 0.5);
    let total = Duration::from_secs_f64(seconds);
    let mut took = Vec::new();
    let mut value_of = [0u64; RING];
    let mut i = 0usize;
    let mut last = 2.0;
    while began.elapsed() < total {
        match gap {
            Gap::None => {
                // Eight in flight.
                let slot = i % RING;
                if value_of[slot] != 0 {
                    card.wait(value_of[slot])?;
                    if began.elapsed() > settle {
                        took.push(work.took(slot as u32, card.frequency));
                    }
                }
                if i >= 8 {
                    let old = (i - 8) % RING;
                    card.wait(value_of[old])?;
                }
                value_of[slot] = card.send(&work.units[slot].list)?;
                i += 1;
            }
            Gap::Share(busy) => {
                let slot = i % RING;
                let v = card.send(&work.units[slot].list)?;
                card.wait(v)?;
                last = work.took(slot as u32, card.frequency);
                if began.elapsed() > settle {
                    took.push(last);
                }
                i += 1;
                let until = Instant::now() + Duration::from_secs_f64(last * (1.0 - busy) / busy / 1e3);
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            }
            Gap::Burst => {
                let mut values = Vec::new();
                for k in 0..20usize {
                    let slot = k;
                    values.push((slot, card.send(&work.units[slot].list)?));
                }
                for &(slot, v) in &values {
                    card.wait(v)?;
                    if began.elapsed() > settle {
                        took.push(work.took(slot as u32, card.frequency));
                    }
                }
                let until = Instant::now() + Duration::from_secs_f64(0.020 * last / 2.0);
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            }
        }
    }
    card.wait(card.value)?;
    let _ = last;
    let (from, to) = (began + settle, Instant::now());
    let mut w: Vec<Reading> = readings.lock().unwrap().iter().copied().filter(|r| r.at >= from && r.at <= to).collect();
    if w.is_empty() {
        w.push(Reading { at: Instant::now(), gfx: 0, mem: 0, activity: 0, watts: 0 });
    }
    let mut g: Vec<i32> = w.iter().map(|r| r.gfx).collect();
    g.sort_unstable();
    let med = |f: fn(&Reading) -> i32| {
        let mut v: Vec<i32> = w.iter().map(f).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    took.sort_by(f64::total_cmp);
    let unit = if took.is_empty() { 0.0 } else { took[took.len() / 2] };
    Ok(Result_ {
        unit,
        gfx: (g[g.len() / 4], g[g.len() / 2], g[3 * g.len() / 4]),
        mem: med(|r| r.mem),
        activity: med(|r| r.activity),
        watts: med(|r| r.watts),
        samples: w.len(),
    })
}

fn main() -> Result<()> {
    let seconds: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7.0);
    let readings: Arc<Mutex<Vec<Reading>>> = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (readings, stop) = (readings.clone(), stop.clone());
        std::thread::spawn(move || {
            let Some(adl) = Adl::new() else {
                eprintln!("no AMD library: no clock");
                return;
            };
            while !stop.load(Ordering::Relaxed) {
                if let Some(r) = adl.read() {
                    readings.lock().unwrap().push(r);
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        })
    };
    let mut card = Card::new()?;
    let size: u64 = 1 << 30;
    let shared = buffer(&card.device, size, D3D12_HEAP_TYPE_DEFAULT, true)?;
    let mask = (size / 16 - 1) as u32;
    println!("idle: a 5 s reading first, with nothing running");
    std::thread::sleep(Duration::from_secs(5));
    {
        let r = readings.lock().unwrap();
        if let Some(l) = r.last() {
            println!("   clock now {} MHz, memory {} MHz, activity {}%, {} W", l.gfx, l.mem, l.activity, l.watts);
        }
    }
    for (name, source, groups, n0) in [("core work (ALU)", ALU, 4096u32, 40u32), ("memory work (random reads)", MEM, 4096, 8)] {
        // Calibrate to about 2 ms of work at the clock the card settles on
        // when it has work all the time.
        let mut n = n0;
        for _ in 0..4 {
            let w = card.work(source, groups, n, mask, &shared)?;
            let r = phase(&mut card, &w, Gap::None, 2.0, &readings)?;
            if r.unit > 0.0 {
                n = ((f64::from(n) * 2.0 / r.unit).round() as u32).max(1);
            }
        }
        let w = card.work(source, groups, n, mask, &shared)?;
        println!("\n== {name}: n = {n}, {groups} groups of 64 threads ==");
        println!(
            "{:<44} {:>9} {:>8}   {:>20} {:>7} {:>9} {:>7}",
            "phase", "piece ms", "against", "core MHz p25/50/75", "mem", "activity", "watts"
        );
        let mut first = 0.0;
        let mut print = |label: &str, r: &Result_| {
            if first == 0.0 {
                first = r.unit;
            }
            println!(
                "{:<44} {:>9.3} {:>7.2}x   {:>6}/{:>5}/{:>5}      {:>5} {:>8}% {:>6}   ({} samples)",
                label,
                r.unit,
                r.unit / first,
                r.gfx.0,
                r.gfx.1,
                r.gfx.2,
                r.mem,
                r.activity,
                r.watts,
                r.samples
            );
        };
        let r = phase(&mut card, &w, Gap::None, seconds, &readings)?;
        print("back to back, 8 in flight", &r);
        let r = phase(&mut card, &w, Gap::Share(0.80), seconds, &readings)?;
        print("one at a time, 80% of the time", &r);
        let r = phase(&mut card, &w, Gap::Share(0.55), seconds, &readings)?;
        print("one at a time, 55% (the game's share)", &r);
        let r = phase(&mut card, &w, Gap::Share(0.30), seconds, &readings)?;
        print("one at a time, 30%", &r);
        let r = phase(&mut card, &w, Gap::Burst, seconds, &readings)?;
        print("twenty pieces, then a gap as long", &r);
        for (label, kind) in [
            ("55%, with a little endless work (compute q.)", D3D12_COMMAND_LIST_TYPE_COMPUTE),
            ("55%, with a little endless work (direct q.)", D3D12_COMMAND_LIST_TYPE_DIRECT),
        ] {
            let halt = Arc::new(AtomicBool::new(false));
            let handle = keep_busy(&card.device, kind, halt.clone(), &shared)?;
            std::thread::sleep(Duration::from_millis(1500));
            let r = phase(&mut card, &w, Gap::Share(0.55), seconds, &readings)?;
            halt.store(true, Ordering::Relaxed);
            let _ = handle.join();
            print(label, &r);
        }
        let r = phase(&mut card, &w, Gap::None, seconds, &readings)?;
        print("back to back again", &r);
    }
    stop.store(true, Ordering::Relaxed);
    let _ = sampler.join();
    Ok(())
}

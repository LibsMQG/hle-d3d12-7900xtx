// Author: libs (hle-d3d12-amd)
//! sampler: a small user-mode sampling profiler for Windows (no admin, no ETW).
//!
//! It only ever opens the target with PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
//! (no write / no VM_OPERATION rights), and every SuspendThread is paired with a
//! ResumeThread through an RAII guard. Threads are suspended one at a time.
#![allow(non_snake_case, clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::Diagnostics::Debug::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::ProcessStatus::K32GetMappedFileNameW;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;

static STOP: AtomicBool = AtomicBool::new(false);
static RESUME_FAILURES: AtomicU64 = AtomicU64::new(0);

const SYMOPT_IGNORE_NT_SYMPATH: u32 = 0x1000;
const SYMOPT_DISABLE_SYMSRV_AUTODETECT: u32 = 0x0200_0000;
const IMAGE_FILE_MACHINE_AMD64: u32 = 0x8664;

// ---------------------------------------------------------------- utilities

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn from_wide(b: &[u16]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..n])
}
unsafe fn from_pwstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}
fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n - 3).collect();
        format!("{t}...")
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Ctx(CONTEXT);

/// Resumes the thread when dropped: guarantees Suspend/Resume pairing on every path
/// (early return, `?`, panic unwinding).
struct Suspended(HANDLE);
impl Drop for Suspended {
    fn drop(&mut self) {
        unsafe {
            if ResumeThread(self.0) == u32::MAX {
                RESUME_FAILURES.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

unsafe extern "system" fn ctrl_handler(kind: u32) -> BOOL {
    STOP.store(true, Ordering::SeqCst);
    if kind >= 2 {
        // close/logoff/shutdown: give the sampling loop time to finish its current thread
        std::thread::sleep(Duration::from_millis(500));
    }
    1
}

// ---------------------------------------------------------------- args

struct Args {
    pid: Option<u32>,
    name: Option<String>,
    seconds: f64,
    hz: f64,
    out: String,
    stacks: bool,
    max_frames: usize,
    stack_bytes: usize,
    sympath: Vec<String>,
    crate_filter: String,
}

const USAGE: &str = "usage: sampler (--pid <pid> | --name <exe>) [--seconds N=10] [--hz H=200] [--out report.txt]
               [--stacks] [--frames 24] [--stack-bytes 65536] [--sympath <dir>]... [--crate victory]
  Writes <out> (text report) and <out-stem>.csv (raw samples).
  --stacks       walk up to --frames frames per busy sample (StackWalk64 over a stack snapshot)
  --sympath dir  extra local directory with PDBs (no symbol server is ever used)
  --crate s      substring selecting 'own' functions for the inclusive/caller tables (default victory)";

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        pid: None,
        name: None,
        seconds: 10.0,
        hz: 200.0,
        out: "sampler_report.txt".into(),
        stacks: false,
        max_frames: 24,
        stack_bytes: 64 * 1024,
        sympath: vec![],
        crate_filter: "victory".into(),
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let val = |i: &mut usize| -> Result<String, String> {
        *i += 1;
        v.get(*i).cloned().ok_or_else(|| format!("missing value for {}", v[*i - 1]))
    };
    while i < v.len() {
        let k = v[i].clone();
        match k.as_str() {
            "--pid" => a.pid = Some(val(&mut i)?.parse().map_err(|e| format!("--pid: {e}"))?),
            "--name" => a.name = Some(val(&mut i)?),
            "--seconds" => a.seconds = val(&mut i)?.parse().map_err(|e| format!("--seconds: {e}"))?,
            "--hz" => a.hz = val(&mut i)?.parse().map_err(|e| format!("--hz: {e}"))?,
            "--out" => a.out = val(&mut i)?,
            "--stacks" => a.stacks = true,
            "--frames" => a.max_frames = val(&mut i)?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--stack-bytes" => a.stack_bytes = val(&mut i)?.parse().map_err(|e| format!("--stack-bytes: {e}"))?,
            "--sympath" => a.sympath.push(val(&mut i)?),
            "--crate" => a.crate_filter = val(&mut i)?,
            "-h" | "--help" => return Err(USAGE.into()),
            _ => return Err(format!("unknown argument {k}\n{USAGE}")),
        }
        i += 1;
    }
    if a.pid.is_none() && a.name.is_none() {
        return Err(USAGE.into());
    }
    if !(a.hz > 0.0 && a.hz <= 5000.0) || !(a.seconds > 0.0) {
        return Err("--hz must be in (0,5000], --seconds > 0".into());
    }
    a.max_frames = a.max_frames.clamp(1, 256);
    a.stack_bytes = a.stack_bytes.clamp(4096, 1 << 20);
    Ok(a)
}

// ---------------------------------------------------------------- processes / threads

fn find_pids(name: &str) -> Vec<(u32, String)> {
    let mut out = vec![];
    unsafe {
        let snap = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snap.0 == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap.0, &mut pe);
        while ok != 0 {
            let n = from_wide(&pe.szExeFile);
            if n.eq_ignore_ascii_case(name) {
                out.push((pe.th32ProcessID, n));
            }
            ok = Process32NextW(snap.0, &mut pe);
        }
    }
    out
}

fn list_tids(pid: u32) -> Vec<u32> {
    let mut out = vec![];
    unsafe {
        let snap = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0));
        if snap.0 == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut ok = Thread32First(snap.0, &mut te);
        while ok != 0 {
            if te.th32OwnerProcessID == pid {
                out.push(te.th32ThreadID);
            }
            ok = Thread32Next(snap.0, &mut te);
        }
    }
    out
}

#[repr(C)]
struct ThreadBasicInformation {
    exit_status: i32,
    teb: *mut c_void,
    client_pid: usize,
    client_tid: usize,
    affinity: usize,
    priority: i32,
    base_priority: i32,
}
type NtQit = unsafe extern "system" fn(HANDLE, u32, *mut c_void, u32, *mut u32) -> i32;

fn nt_query_information_thread() -> Option<NtQit> {
    unsafe {
        let m = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        if m.is_null() {
            return None;
        }
        GetProcAddress(m, c"NtQueryInformationThread".as_ptr() as *const u8).map(|f| std::mem::transmute::<_, NtQit>(f))
    }
}

unsafe fn read_u64(hp: HANDLE, addr: u64) -> Option<u64> {
    let mut v = 0u64;
    let mut got = 0usize;
    if ReadProcessMemory(hp, addr as *const c_void, &mut v as *mut u64 as *mut c_void, 8, &mut got) != 0 && got == 8 {
        Some(v)
    } else {
        None
    }
}
unsafe fn read_u32(hp: HANDLE, addr: u64) -> Option<u32> {
    let mut v = 0u32;
    let mut got = 0usize;
    if ReadProcessMemory(hp, addr as *const c_void, &mut v as *mut u32 as *mut c_void, 4, &mut got) != 0 && got == 4 {
        Some(v)
    } else {
        None
    }
}

fn thread_cpu_100ns(h: HANDLE) -> Option<u64> {
    unsafe {
        let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
        if GetThreadTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 {
            return None;
        }
        let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
        Some(f(k) + f(u))
    }
}

struct Th {
    tid: u32,
    h: OwnedHandle,
    name: String,
    cpu0: u64,
    cpu_last: u64,
    cyc_last: Option<u64>,
    t_last: Option<Instant>,
    stack_base: u64,
    alive: bool,
    fails: u32,
}

// ---------------------------------------------------------------- modules & symbols

const CAT_MAIN: &str = "main exe";
const CAT_TITLE: &str = "title code (victory-image-*)";
const CAT_ANON: &str = "anonymous exec memory";
const CAT_UNKNOWN: &str = "unknown";

fn categorize(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.starts_with("victory-image-") {
        CAT_TITLE
    } else if n == "ntdll.dll" {
        "ntdll"
    } else if n == "kernelbase.dll" || n == "kernel32.dll" {
        "kernelbase/kernel32"
    } else if n == "win32u.dll" {
        "win32u"
    } else if n.starts_with("d3d12") || n.starts_with("dxgi") || n.starts_with("dxcore") || n.starts_with("d3d11") || n.starts_with("d3dscache") {
        "d3d12/dxgi"
    } else if n.starts_with("amd") || n.starts_with("ati") || n.starts_with("nvwgf") || n.starts_with("nvd3d") || n.starts_with("nvldumd") || n.starts_with("igd") || n.starts_with("igc") || n.starts_with("intelocl") {
        "gpu driver"
    } else if n.starts_with("ucrtbase") || n.starts_with("vcruntime") || n.starts_with("msvcp") {
        "crt"
    } else {
        "other"
    }
}

struct Module {
    base: u64,
    size: u64,
    name: String,
    path: String,
    cat: &'static str,
    anon: bool,
    symtype: i32, // -1 unknown
}

struct Mods {
    v: Vec<Module>, // sorted by base
    dev_map: Vec<(String, String)>,
    tried_pages: HashSet<u64>,
}

fn device_map() -> Vec<(String, String)> {
    let mut m = vec![];
    for l in b'A'..=b'Z' {
        let drive = format!("{}:", l as char);
        let mut buf = [0u16; 1024];
        let n = unsafe { QueryDosDeviceW(wide(&drive).as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n > 0 {
            m.push((from_wide(&buf), drive));
        }
    }
    m
}

impl Mods {
    fn find(&self, a: u64) -> Option<usize> {
        let i = self.v.partition_point(|m| m.base <= a);
        if i == 0 {
            return None;
        }
        let m = &self.v[i - 1];
        (a < m.base + m.size).then_some(i - 1)
    }

    fn add(&mut self, hp: HANDLE, mut m: Module) {
        if m.size == 0 || self.find(m.base).is_some() || self.find(m.base + m.size - 1).is_some() {
            return;
        }
        if !m.anon {
            let p = wide(&m.path);
            let n = wide(&m.name);
            unsafe { SymLoadModuleExW(hp, std::ptr::null_mut(), p.as_ptr(), n.as_ptr(), m.base, m.size as u32, std::ptr::null(), 0) };
        }
        m.symtype = -1;
        let i = self.v.partition_point(|x| x.base < m.base);
        self.v.insert(i, m);
    }

    fn refresh_toolhelp(&mut self, hp: HANDLE, pid: u32, first: bool) -> Option<String> {
        let mut main_path = None;
        unsafe {
            let mut snap = INVALID_HANDLE_VALUE;
            for _ in 0..10 {
                snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
                if snap != INVALID_HANDLE_VALUE || GetLastError() != ERROR_BAD_LENGTH {
                    break;
                }
            }
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let snap = OwnedHandle(snap);
            let mut me: MODULEENTRY32W = std::mem::zeroed();
            me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            let mut ok = Module32FirstW(snap.0, &mut me);
            let mut idx = 0;
            while ok != 0 {
                let name = from_wide(&me.szModule);
                let path = from_wide(&me.szExePath);
                let is_main = idx == 0;
                if is_main {
                    main_path = Some(path.clone());
                }
                let cat = if is_main && first { CAT_MAIN } else { categorize(&name) };
                // main exe category must stick even when later refreshes see it again
                self.add(hp, Module { base: me.modBaseAddr as u64, size: me.modBaseSize as u64, name, path, cat, anon: false, symtype: -1 });
                idx += 1;
                ok = Module32NextW(snap.0, &mut me);
            }
        }
        main_path
    }

    /// For addresses outside every loader-listed module (e.g. images mapped by hand).
    fn discover(&mut self, hp: HANDLE, a: u64) {
        if self.find(a).is_some() || !self.tried_pages.insert(a >> 12) {
            return;
        }
        unsafe {
            let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            if VirtualQueryEx(hp, a as *const c_void, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 || mbi.State != MEM_COMMIT {
                return;
            }
            let ab = mbi.AllocationBase as u64;
            if mbi.Type == MEM_IMAGE {
                let size = read_u32(hp, ab + 0x3c)
                    .and_then(|lfanew| read_u32(hp, ab + lfanew as u64 + 24 + 56))
                    .unwrap_or(0) as u64;
                let mut buf = [0u16; 1024];
                let n = K32GetMappedFileNameW(hp, ab as *const c_void, buf.as_mut_ptr(), buf.len() as u32);
                let mut path = if n > 0 { from_wide(&buf[..n as usize]) } else { format!("image@{ab:x}") };
                for (dev, drive) in &self.dev_map {
                    if path.len() > dev.len() && path[..dev.len()].eq_ignore_ascii_case(dev) && path.as_bytes()[dev.len()] == b'\\' {
                        path = format!("{drive}{}", &path[dev.len()..]);
                        break;
                    }
                }
                let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
                let cat = categorize(&name);
                let size = if size == 0 { (mbi.BaseAddress as u64 + mbi.RegionSize as u64) - ab } else { size };
                self.add(hp, Module { base: ab, size, name, path, cat, anon: false, symtype: -1 });
            } else {
                let end = mbi.BaseAddress as u64 + mbi.RegionSize as u64;
                let name = format!("anon@{ab:x}");
                self.add(hp, Module { base: ab, size: end - ab, path: name.clone(), name, cat: CAT_ANON, anon: true, symtype: -1 });
            }
        }
    }
}

struct Symbolizer {
    hp: HANDLE,
    cache: HashMap<u64, (u32, Option<usize>)>,
    names: Vec<String>,
    name_idx: HashMap<String, u32>,
    buf: Vec<u64>,
}

impl Symbolizer {
    fn intern(&mut self, s: String) -> u32 {
        if let Some(&i) = self.name_idx.get(&s) {
            return i;
        }
        let i = self.names.len() as u32;
        self.names.push(s.clone());
        self.name_idx.insert(s, i);
        i
    }

    /// Returns (function-name id, module index).
    fn sym(&mut self, mods: &mut Mods, a: u64) -> (u32, Option<usize>) {
        if let Some(&r) = self.cache.get(&a) {
            return r;
        }
        mods.discover(self.hp, a);
        let mi = mods.find(a);
        let s = match mi {
            None => format!("?!{:#x}", a & !0xFFF),
            Some(i) => {
                let m = &mods.v[i];
                let off = a - m.base;
                if m.anon || m.cat == CAT_TITLE {
                    format!("{}+{:#x}", m.name, off & !0xFF)
                } else {
                    unsafe {
                        let si = self.buf.as_mut_ptr() as *mut SYMBOL_INFOW;
                        std::ptr::write_bytes(self.buf.as_mut_ptr(), 0, self.buf.len());
                        (*si).SizeOfStruct = std::mem::size_of::<SYMBOL_INFOW>() as u32;
                        (*si).MaxNameLen = 2000;
                        let mut disp = 0u64;
                        let ok = SymFromAddrW(self.hp, a, &mut disp, si) != 0;
                        let symtype = {
                            let m = &mut mods.v[i];
                            if m.symtype < 0 || m.symtype == SymDeferred {
                                let mut info: IMAGEHLP_MODULEW64 = std::mem::zeroed();
                                info.SizeOfStruct = std::mem::size_of::<IMAGEHLP_MODULEW64>() as u32;
                                if SymGetModuleInfoW64(self.hp, m.base, &mut info) != 0 {
                                    m.symtype = info.SymType;
                                }
                            }
                            m.symtype
                        };
                        let m = &mods.v[i];
                        if ok {
                            let nlen = ((*si).NameLen as usize).min(2000);
                            let name = String::from_utf16_lossy(std::slice::from_raw_parts((*si).Name.as_ptr(), nlen));
                            if symtype != SymPdb && disp > 0x400 {
                                format!("{}+{:#x} [~{}]", m.name, off & !0xFF, name)
                            } else {
                                format!("{}!{}", m.name, name)
                            }
                        } else {
                            format!("{}+{:#x}", m.name, off & !0xFF)
                        }
                    }
                }
            }
        };
        let id = self.intern(s);
        self.cache.insert(a, (id, mi));
        (id, mi)
    }
}

// ---------------------------------------------------------------- stack walk

thread_local! {
    static SNAP: RefCell<(u64, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
}

unsafe extern "system" fn read_mem(hp: HANDLE, addr: u64, buf: *mut c_void, n: u32, read: *mut u32) -> BOOL {
    let hit = SNAP.with(|s| {
        let s = s.borrow();
        let (base, ref data) = *s;
        if addr >= base && addr.saturating_add(n as u64) <= base + data.len() as u64 {
            std::ptr::copy_nonoverlapping(data.as_ptr().add((addr - base) as usize), buf as *mut u8, n as usize);
            true
        } else {
            false
        }
    });
    if hit {
        if !read.is_null() {
            *read = n;
        }
        return 1;
    }
    let mut got = 0usize;
    let ok = ReadProcessMemory(hp, addr as *const c_void, buf, n as usize, &mut got);
    if !read.is_null() {
        *read = got as u32;
    }
    ok
}

unsafe fn walk(hp: HANDLE, ht: HANDLE, ctx: &Ctx, max: usize, out: &mut Vec<u64>) {
    let mut c = *ctx;
    let mut f: STACKFRAME64 = std::mem::zeroed();
    f.AddrPC.Offset = c.0.Rip;
    f.AddrPC.Mode = AddrModeFlat;
    f.AddrFrame.Offset = c.0.Rbp;
    f.AddrFrame.Mode = AddrModeFlat;
    f.AddrStack.Offset = c.0.Rsp;
    f.AddrStack.Mode = AddrModeFlat;
    let mut last = (0u64, 0u64);
    while out.len() < max {
        let ok = StackWalk64(
            IMAGE_FILE_MACHINE_AMD64,
            hp,
            ht,
            &mut f,
            &mut c.0 as *mut CONTEXT as *mut c_void,
            Some(read_mem),
            Some(SymFunctionTableAccess64),
            Some(SymGetModuleBase64),
            None,
        );
        if ok == 0 || f.AddrPC.Offset == 0 {
            break;
        }
        let cur = (f.AddrPC.Offset, f.AddrStack.Offset);
        if cur == last {
            break;
        }
        last = cur;
        out.push(f.AddrPC.Offset);
    }
}

// ---------------------------------------------------------------- sampling

struct Sample {
    th: u32,
    t_ms: f32,
    rip: u64,
    cyc: u64,
    dt_s: f32,
    busy: bool,
    fr_start: u32,
    fr_len: u16,
}

fn calibrate_tsc() -> f64 {
    unsafe {
        let me = GetCurrentThread();
        let mut c0 = 0u64;
        QueryThreadCycleTime(me, &mut c0);
        let t0 = Instant::now();
        let mut x = 0u64;
        while t0.elapsed() < Duration::from_millis(150) {
            x = std::hint::black_box(x.wrapping_add(1));
        }
        let mut c1 = 0u64;
        QueryThreadCycleTime(me, &mut c1);
        (c1 - c0) as f64 / t0.elapsed().as_secs_f64()
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let pid = match (args.pid, &args.name) {
        (Some(p), _) => p,
        (None, Some(n)) => {
            let v = find_pids(n);
            match v.len() {
                0 => return Err(format!("no process named {n}")),
                1 => v[0].0,
                _ => {
                    eprintln!("warning: {} processes named {n}: {:?}; using the first", v.len(), v.iter().map(|x| x.0).collect::<Vec<_>>());
                    v[0].0
                }
            }
        }
        _ => unreachable!(),
    };
    if pid == std::process::id() {
        return Err("refusing to sample myself".into());
    }
    unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1) };

    let hp = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE, 0, pid) });
    if hp.0.is_null() {
        return Err(format!("OpenProcess({pid}) failed: {}", unsafe { GetLastError() }));
    }
    let hp_raw = hp.0;
    unsafe {
        let mut wow = 0;
        if IsWow64Process(hp_raw, &mut wow) != 0 && wow != 0 {
            return Err("target is a 32-bit (WoW64) process; only x64 is supported".into());
        }
    }

    // ---- dbghelp: local search path only (System32 dbghelp has no symsrv.dll -> no downloads)
    for k in ["_NT_SYMBOL_PATH", "_NT_ALT_SYMBOL_PATH", "_NT_ALTERNATE_SYMBOL_PATH", "_NT_SYMCACHE_PATH"] {
        std::env::remove_var(k);
    }
    let mut mods = Mods { v: vec![], dev_map: device_map(), tried_pages: HashSet::new() };
    unsafe {
        SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS | SYMOPT_IGNORE_NT_SYMPATH | SYMOPT_DISABLE_SYMSRV_AUTODETECT);
        if SymInitializeW(hp_raw, std::ptr::null(), 0) == 0 {
            return Err(format!("SymInitializeW failed: {}", GetLastError()));
        }
    }
    // Set the search path first (exe folder + --sympath) so module loads below use it.
    let main_path = {
        unsafe {
            let snap = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid));
            let mut me: MODULEENTRY32W = std::mem::zeroed();
            me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            if snap.0 != INVALID_HANDLE_VALUE && Module32FirstW(snap.0, &mut me) != 0 {
                from_wide(&me.szExePath)
            } else {
                return Err(format!("cannot enumerate modules of {pid}: {}", GetLastError()));
            }
        }
    };
    let exe_dir = main_path.rsplit_once('\\').map(|x| x.0.to_string()).unwrap_or_default();
    let mut sp = vec![exe_dir.clone()];
    sp.extend(args.sympath.iter().cloned());
    let sympath = sp.join(";");
    unsafe { SymSetSearchPathW(hp_raw, wide(&sympath).as_ptr()) };
    mods.refresh_toolhelp(hp_raw, pid, true);

    let tsc_hz = calibrate_tsc();
    let ntqit = nt_query_information_thread();
    let ctx_flags = if args.stacks { CONTEXT_FULL_AMD64 } else { CONTEXT_CONTROL_AMD64 };

    let mut threads: Vec<Th> = vec![];
    let mut tid_idx: HashMap<u32, usize> = HashMap::new();
    let mut samples: Vec<Sample> = Vec::with_capacity(1 << 16);
    let mut frames: Vec<u64> = vec![];
    let mut pending: HashSet<u64> = HashSet::new();
    let mut susp_max = Duration::ZERO;
    let mut susp_total = Duration::ZERO;
    let mut susp_n = 0u64;
    let mut ctx_fail = 0u64;
    let mut walk_scratch: Vec<u64> = Vec::with_capacity(256);

    let period = Duration::from_secs_f64(1.0 / args.hz);
    let t_start = Instant::now();
    let t_end = t_start + Duration::from_secs_f64(args.seconds);
    let mut next = t_start;
    // high-resolution waitable timer (std sleep can overshoot by ~1 tick on some systems)
    let timer = OwnedHandle(unsafe { CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS) });
    let sleep_for = |d: Duration| unsafe {
        if timer.0.is_null() {
            std::thread::sleep(d);
        } else {
            let due: i64 = -((d.as_nanos() / 100).max(1) as i64);
            if SetWaitableTimer(timer.0, &due, 0, None, std::ptr::null(), 0) != 0 {
                WaitForSingleObject(timer.0, 1000);
            } else {
                std::thread::sleep(d);
            }
        }
    };
    let mut first_enum = true;
    let (tid_tx, tid_rx) = std::sync::mpsc::sync_channel::<Vec<u32>>(1);
    {
        let stop_at = t_end + Duration::from_millis(500);
        std::thread::spawn(move || {
            while Instant::now() < stop_at && !STOP.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
                if tid_tx.send(list_tids(pid)).is_err() {
                    break;
                }
            }
        });
    }
    let mut last_mod_refresh = t_start;
    let mut ticks = 0u64;
    let (mut t_enum, mut t_mods, mut t_samp) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut target_exited = false;
    eprintln!("sampling pid {pid} ({main_path}) for {} s at {} Hz{} ...", args.seconds, args.hz, if args.stacks { " with stacks" } else { "" });

    while !STOP.load(Ordering::SeqCst) {
        let now = Instant::now();
        if now >= t_end {
            break;
        }
        // ---- thread list refresh. The Toolhelp thread snapshot covers the whole system and can
        // take tens of ms, so it runs on a helper thread; the first list is taken synchronously.
        let fresh = if first_enum {
            first_enum = false;
            Some(list_tids(pid))
        } else {
            tid_rx.try_recv().ok()
        };
        if let Some(tids) = fresh {
            if unsafe { WaitForSingleObject(hp_raw, 0) } == WAIT_OBJECT_0 {
                target_exited = true;
                break;
            }
            if tids.is_empty() {
                target_exited = true;
                break;
            }
            let set: HashSet<u32> = tids.iter().copied().collect();
            for t in threads.iter_mut() {
                if t.alive && !set.contains(&t.tid) {
                    t.alive = false;
                }
            }
            for tid in tids {
                if tid_idx.contains_key(&tid) {
                    continue;
                }
                let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION | THREAD_QUERY_LIMITED_INFORMATION, 0, tid) };
                if h.is_null() {
                    continue;
                }
                let name = unsafe {
                    let mut p: *mut u16 = std::ptr::null_mut();
                    if GetThreadDescription(h, &mut p) >= 0 && !p.is_null() {
                        let s = from_pwstr(p);
                        LocalFree(p as *mut c_void);
                        s
                    } else {
                        String::new()
                    }
                };
                let stack_base = ntqit
                    .and_then(|f| unsafe {
                        let mut tbi: ThreadBasicInformation = std::mem::zeroed();
                        if f(h, 0, &mut tbi as *mut _ as *mut c_void, std::mem::size_of::<ThreadBasicInformation>() as u32, std::ptr::null_mut()) >= 0 {
                            read_u64(hp_raw, tbi.teb as u64 + 8)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let cpu = thread_cpu_100ns(h).unwrap_or(0);
                tid_idx.insert(tid, threads.len());
                threads.push(Th { tid, h: OwnedHandle(h), name, cpu0: cpu, cpu_last: cpu, cyc_last: None, t_last: None, stack_base, alive: true, fails: 0 });
            }
        }
        let p0 = Instant::now();
        t_enum += p0 - now;
        // ---- module refresh + discovery of unknown code (every 500 ms)
        if now.duration_since(last_mod_refresh) >= Duration::from_millis(500) {
            last_mod_refresh = now;
            mods.refresh_toolhelp(hp_raw, pid, false);
            for a in pending.drain() {
                mods.discover(hp_raw, a);
            }
        }

        let p1 = Instant::now();
        t_mods += p1 - p0;
        // ---- one sample per live thread, one thread suspended at a time
        let t_ms = t_start.elapsed().as_secs_f64() * 1000.0;
        for (ti, t) in threads.iter_mut().enumerate() {
            if !t.alive {
                continue;
            }
            let h = t.h.0;
            let mut ctx: Ctx = unsafe { std::mem::zeroed() };
            ctx.0.ContextFlags = ctx_flags;
            let mut cyc = 0u64;
            let mut got_snap = false;
            let ok;
            let s0 = Instant::now();
            unsafe {
                if SuspendThread(h) == u32::MAX {
                    t.fails += 1;
                    if t.fails >= 3 {
                        t.alive = false;
                    }
                    continue;
                }
                let _resume = Suspended(h); // ResumeThread on every exit path of this block
                ok = GetThreadContext(h, &mut ctx.0) != 0;
                QueryThreadCycleTime(h, &mut cyc);
                if ok && args.stacks {
                    let dt = t.t_last.map(|x| s0.duration_since(x).as_secs_f64()).unwrap_or(0.0);
                    let busy = t.cyc_last.map(|c| (cyc.saturating_sub(c)) as f64 >= (0.01 * dt).max(25e-6) * tsc_hz).unwrap_or(false);
                    if busy {
                        let rsp = ctx.0.Rsp;
                        let mut len = args.stack_bytes as u64;
                        if t.stack_base > rsp && t.stack_base - rsp < (256 << 20) {
                            len = len.min(t.stack_base - rsp);
                        }
                        SNAP.with(|s| {
                            let mut s = s.borrow_mut();
                            s.0 = rsp;
                            s.1.resize(len as usize, 0);
                            let mut got = 0usize;
                            if ReadProcessMemory(hp_raw, rsp as *const c_void, s.1.as_mut_ptr() as *mut c_void, len as usize, &mut got) == 0 {
                                // partial copies are not reported reliably: fall back to a small window
                                let l2 = (len as usize).min(4096);
                                if ReadProcessMemory(hp_raw, rsp as *const c_void, s.1.as_mut_ptr() as *mut c_void, l2, &mut got) == 0 {
                                    got = 0;
                                }
                            }
                            s.1.truncate(got);
                            got_snap = got > 0;
                        });
                    }
                }
                // _resume dropped here -> ResumeThread
            }
            let d = s0.elapsed();
            susp_max = susp_max.max(d);
            susp_total += d;
            susp_n += 1;
            if !ok {
                ctx_fail += 1;
                continue;
            }
            t.fails = 0;
            let now_t = Instant::now();
            let prev_c = t.cyc_last.replace(cyc);
            let prev_t = t.t_last.replace(now_t);
            let (Some(pc), Some(pt)) = (prev_c, prev_t) else { continue };
            let dt = now_t.duration_since(pt).as_secs_f64();
            let dc = cyc.saturating_sub(pc);
            let busy = dc as f64 >= (0.01 * dt).max(25e-6) * tsc_hz;
            let rip = ctx.0.Rip;
            if mods.find(rip).is_none() {
                pending.insert(rip);
            }
            let fr_start = frames.len() as u32;
            let mut fr_len = 0u16;
            if got_snap {
                walk_scratch.clear();
                unsafe { walk(hp_raw, h, &ctx, args.max_frames, &mut walk_scratch) };
                if walk_scratch.first() != Some(&rip) {
                    walk_scratch.insert(0, rip);
                }
                for &a in &walk_scratch {
                    if mods.find(a).is_none() {
                        pending.insert(a);
                    }
                }
                frames.extend_from_slice(&walk_scratch);
                fr_len = walk_scratch.len() as u16;
            }
            samples.push(Sample { th: ti as u32, t_ms: t_ms as f32, rip, cyc: dc, dt_s: dt as f32, busy, fr_start, fr_len });
        }
        for t in threads.iter_mut() {
            if let Some(c) = thread_cpu_100ns(t.h.0) {
                t.cpu_last = c;
            }
        }
        t_samp += p1.elapsed();
        ticks += 1;
        next += period;
        let now = Instant::now();
        if next > now {
            sleep_for(next - now);
        } else {
            // overrun: do not fire back-to-back ticks (that would make intervals tiny)
            next = now + period / 2;
            sleep_for(period / 2);
        }
    }
    let elapsed = t_start.elapsed().as_secs_f64();
    let interrupted = STOP.load(Ordering::SeqCst);
    eprintln!("collected {} samples in {} ticks; symbolizing...", samples.len(), ticks);

    // ---- post-processing (target keeps running; nothing is suspended from here on)
    mods.refresh_toolhelp(hp_raw, pid, false);
    for a in pending.drain() {
        mods.discover(hp_raw, a);
    }
    let mut sy = Symbolizer { hp: hp_raw, cache: HashMap::new(), names: vec![], name_idx: HashMap::new(), buf: vec![0u64; (std::mem::size_of::<SYMBOL_INFOW>() + 4100) / 8 + 1] };

    struct Res {
        self_id: u32,
        self_mod: Option<usize>,
        stack: Vec<(u32, Option<usize>)>,
    }
    let mut res: Vec<Res> = Vec::with_capacity(samples.len());
    for s in &samples {
        let (id, m) = sy.sym(&mut mods, s.rip);
        let mut st = vec![];
        if s.fr_len > 0 {
            for (k, &a) in frames[s.fr_start as usize..s.fr_start as usize + s.fr_len as usize].iter().enumerate() {
                // return addresses point after the call: look up a-1
                st.push(sy.sym(&mut mods, if k == 0 { a } else { a - 1 }));
            }
        }
        res.push(Res { self_id: id, self_mod: m, stack: st });
    }
    let main_mod = mods.v.iter().position(|m| m.cat == CAT_MAIN);
    let cat_of = |m: Option<usize>| m.map(|i| mods.v[i].cat).unwrap_or(CAT_UNKNOWN);
    let is_syscall_stub = |id: u32, m: Option<usize>| -> bool {
        let Some(i) = m else { return false };
        let mn = mods.v[i].name.to_ascii_lowercase();
        if mn != "ntdll.dll" && mn != "win32u.dll" {
            return false;
        }
        let n = &sy.names[id as usize];
        let f = n.split('!').nth(1).unwrap_or("");
        f.starts_with("Nt") || f.starts_with("Zw")
    };
    let cpu_ms = |s: &Sample| (s.cyc as f64 / tsc_hz).min(s.dt_s as f64) * 1000.0;

    // ---- report
    let mut r = String::new();
    use std::fmt::Write as _;
    let total_busy = samples.iter().filter(|s| s.busy).count().max(1);
    writeln!(r, "sampler report").unwrap();
    writeln!(r, "target           : pid {pid}  {main_path}").unwrap();
    writeln!(r, "duration         : {elapsed:.2} s{}{}", if interrupted { " (interrupted)" } else { "" }, if target_exited { " (target exited)" } else { "" }).unwrap();
    writeln!(r, "rate             : requested {} Hz, achieved {:.1} ticks/s ({} ticks)", args.hz, ticks as f64 / elapsed, ticks).unwrap();
    writeln!(r, "samples          : {} ({} busy = thread consumed >=1% of the interval and >=25us in CPU cycles)", samples.len(), total_busy).unwrap();
    writeln!(r, "suspend window   : avg {:.1} us, max {:.1} us over {} suspensions; resume failures: {}; GetThreadContext failures: {}", if susp_n > 0 { susp_total.as_secs_f64() * 1e6 / susp_n as f64 } else { 0.0 }, susp_max.as_secs_f64() * 1e6, susp_n, RESUME_FAILURES.load(Ordering::SeqCst), ctx_fail).unwrap();
 writeln!(r, "sampler cost     : per tick avg {:.0} us sampling+walks, {:.0} us thread enum, {:.0} us module refresh", t_samp.as_secs_f64() * 1e6 / ticks.max(1) as f64, t_enum.as_secs_f64() * 1e6 / ticks.max(1) as f64, t_mods.as_secs_f64() * 1e6 / ticks.max(1) as f64).unwrap();
    writeln!(r, "cycle clock      : {:.3} GHz (QueryThreadCycleTime calibration)", tsc_hz / 1e9).unwrap();
    writeln!(r, "symbol path      : {sympath}  (local only, no symbol server)").unwrap();
    writeln!(r, "stacks           : {}", if args.stacks { format!("on, up to {} frames, {} byte stack snapshot", args.max_frames, args.stack_bytes) } else { "off".into() }).unwrap();
    writeln!(r).unwrap();
    writeln!(r, "Notes: 'busy%' = share of a thread's samples in which it burned CPU since its previous sample.").unwrap();
    writeln!(r, "'cpu ms' columns charge each busy sample with the thread's measured cycles since its previous sample.").unwrap();
    writeln!(r, "'idle-wait%' = idle samples whose Rip sits on an ntdll/win32u Nt*/Zw* syscall stub (blocked in the kernel).").unwrap();
    writeln!(r).unwrap();

    // threads
    #[derive(Default)]
    struct TStat {
        n: u64,
        busy: u64,
        idle_wait: u64,
        cpu_ms: f64,
        top: HashMap<u32, u64>,
    }
    let mut ts: Vec<TStat> = (0..threads.len()).map(|_| TStat::default()).collect();
    for (s, rr) in samples.iter().zip(&res) {
        let t = &mut ts[s.th as usize];
        t.n += 1;
        if s.busy {
            t.busy += 1;
            t.cpu_ms += cpu_ms(s);
            *t.top.entry(rr.self_id).or_default() += 1;
        } else if is_syscall_stub(rr.self_id, rr.self_mod) {
            t.idle_wait += 1;
        }
    }
    let mut order: Vec<usize> = (0..threads.len()).collect();
    order.sort_by(|&a, &b| (threads[b].cpu_last - threads[b].cpu0).cmp(&(threads[a].cpu_last - threads[a].cpu0)));
    writeln!(r, "== Threads (sorted by CPU time during the run; {} threads seen) ==", threads.len()).unwrap();
    writeln!(r, "{:>7} {:>8} {:>6} {:>7} {:>6} {:>10}  {:<28} top busy self function", "tid", "cpu s", "core%", "samples", "busy%", "idle-wait%", "name").unwrap();
    let mut total_cpu_s = 0.0;
    for &i in &order {
        let t = &threads[i];
        let st = &ts[i];
        let cpu_s = (t.cpu_last - t.cpu0) as f64 / 1e7;
        total_cpu_s += cpu_s;
        let top = st.top.iter().max_by_key(|x| *x.1).map(|(&id, &n)| format!("{} ({:.0}%)", trunc(&sy.names[id as usize], 90), 100.0 * n as f64 / st.busy.max(1) as f64)).unwrap_or_default();
        let pct = |x: u64| if st.n > 0 { 100.0 * x as f64 / st.n as f64 } else { 0.0 };
        writeln!(r, "{:>7} {:>8.3} {:>6.1} {:>7} {:>6.1} {:>10.1}  {:<28} {}", t.tid, cpu_s, 100.0 * cpu_s / elapsed, st.n, pct(st.busy), pct(st.idle_wait), trunc(if t.name.is_empty() { "-" } else { &t.name }, 28), top).unwrap();
    }
    writeln!(r, "total CPU: {:.3} s over {:.2} s = {:.2} cores", total_cpu_s, elapsed, total_cpu_s / elapsed).unwrap();
    writeln!(r).unwrap();

    // module summary
    let mut cat_stat: HashMap<&str, (u64, f64)> = HashMap::new();
    let mut mod_stat: HashMap<Option<usize>, (u64, f64)> = HashMap::new();
    for (s, rr) in samples.iter().zip(&res) {
        if !s.busy {
            continue;
        }
        let c = cat_stat.entry(cat_of(rr.self_mod)).or_default();
        c.0 += 1;
        c.1 += cpu_ms(s);
        let m = mod_stat.entry(rr.self_mod).or_default();
        m.0 += 1;
        m.1 += cpu_ms(s);
    }
    writeln!(r, "== Module summary, busy samples (self) ==").unwrap();
    let mut cv: Vec<_> = cat_stat.into_iter().collect();
    cv.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    writeln!(r, "{:>8} {:>6} {:>10}  category", "samples", "%", "cpu ms").unwrap();
    for (c, (n, ms)) in &cv {
        let label = if *c == CAT_MAIN { format!("{} ({})", c, main_mod.map(|i| mods.v[i].name.as_str()).unwrap_or("?")) } else { c.to_string() };
        writeln!(r, "{:>8} {:>6.2} {:>10.1}  {}", n, 100.0 * *n as f64 / total_busy as f64, ms, label).unwrap();
    }
    writeln!(r).unwrap();
    let mut mv: Vec<_> = mod_stat.into_iter().collect();
    mv.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    writeln!(r, "{:>8} {:>6} {:>10}  {:<9} module (category)", "samples", "%", "cpu ms", "symbols").unwrap();
    for (m, (n, ms)) in mv.iter().take(25) {
        let (name, cat, st) = match m {
            Some(i) => {
                let m = &mods.v[*i];
                let st = match m.symtype {
                    x if x == SymPdb => "pdb",
                    x if x == SymExport => "exports",
                    x if x == SymNone => "none",
                    x if x == SymDeferred => "deferred",
                    -1 => "-",
                    _ => "other",
                };
                (m.name.clone(), m.cat, if m.anon || m.cat == CAT_TITLE { "-" } else { st })
            }
            None => ("<unknown>".to_string(), CAT_UNKNOWN, "-"),
        };
        writeln!(r, "{:>8} {:>6.2} {:>10.1}  {:<9} {} ({})", n, 100.0 * *n as f64 / total_busy as f64, ms, st, name, cat).unwrap();
    }
    writeln!(r).unwrap();

    // flat
    let mut flat: HashMap<u32, (u64, f64)> = HashMap::new();
    for (s, rr) in samples.iter().zip(&res) {
        if s.busy {
            let e = flat.entry(rr.self_id).or_default();
            e.0 += 1;
            e.1 += cpu_ms(s);
        }
    }
    let mut fv: Vec<_> = flat.into_iter().collect();
    fv.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    writeln!(r, "== Top 40 functions, flat (self), busy samples ==").unwrap();
    writeln!(r, "{:>8} {:>6} {:>10}  function", "samples", "%", "cpu ms").unwrap();
    for (id, (n, ms)) in fv.iter().take(40) {
        writeln!(r, "{:>8} {:>6.2} {:>10.1}  {}", n, 100.0 * *n as f64 / total_busy as f64, ms, trunc(&sy.names[*id as usize], 160)).unwrap();
    }
    writeln!(r).unwrap();

    // title code buckets
    let tv: Vec<_> = fv.iter().filter(|(id, _)| sy.names[*id as usize].starts_with("victory-image-")).take(40).collect();
    writeln!(r, "== Title code (victory-image-*) hot offset buckets (0x100 granularity), busy samples ==").unwrap();
    if tv.is_empty() {
        writeln!(r, "(no busy samples in title code)").unwrap();
    }
    for (id, (n, ms)) in tv {
        writeln!(r, "{:>8} {:>6.2} {:>10.1}  {}", n, 100.0 * *n as f64 / total_busy as f64, ms, sy.names[*id as usize]).unwrap();
    }
    writeln!(r).unwrap();

    // inclusive
    if args.stacks {
        let filt = args.crate_filter.to_ascii_lowercase();
        let is_own = |id: u32, m: Option<usize>| m.is_some() && m == main_mod && sy.names[id as usize].split_once('!').map(|x| x.1).unwrap_or("").to_ascii_lowercase().contains(&filt);
        let mut incl: HashMap<u32, (u64, f64)> = HashMap::new();
        let mut caller: HashMap<u32, (u64, f64)> = HashMap::new();
        let (mut walked, mut depth_sum, mut reached_main, mut outside, mut outside_attr) = (0u64, 0u64, 0u64, 0u64, 0u64);
        for (s, rr) in samples.iter().zip(&res) {
            if !s.busy || rr.stack.is_empty() {
                continue;
            }
            walked += 1;
            depth_sum += rr.stack.len() as u64;
            if rr.stack.iter().any(|x| x.1.is_some() && x.1 == main_mod) {
                reached_main += 1;
            }
            let mut seen = HashSet::new();
            for &(id, m) in &rr.stack {
                if is_own(id, m) && seen.insert(id) {
                    let e = incl.entry(id).or_default();
                    e.0 += 1;
                    e.1 += cpu_ms(s);
                }
            }
            if rr.self_mod != main_mod || main_mod.is_none() {
                outside += 1;
                if let Some(&(id, _)) = rr.stack.iter().skip(1).find(|x| is_own(x.0, x.1)) {
                    outside_attr += 1;
                    let e = caller.entry(id).or_default();
                    e.0 += 1;
                    e.1 += cpu_ms(s);
                }
            }
        }
        writeln!(r, "== Stack walk quality ==").unwrap();
        writeln!(r, "busy samples walked: {walked}; avg depth {:.1}; walks reaching the main exe: {:.1}%; outside-main-exe busy samples attributed to an own ('{}') caller: {}/{} ({:.1}%)", depth_sum as f64 / walked.max(1) as f64, 100.0 * reached_main as f64 / walked.max(1) as f64, args.crate_filter, outside_attr, outside, 100.0 * outside_attr as f64 / outside.max(1) as f64).unwrap();
        writeln!(r).unwrap();
        for (title, map) in [
            (format!("== Top 40 inclusive main-exe functions matching '{}' (busy samples with the function anywhere on the stack) ==", args.crate_filter), incl),
            (format!("== Top 40 nearest '{}' callers of busy time spent OUTSIDE the main exe (ntdll/kernelbase/d3d12/driver/title...) ==", args.crate_filter), caller),
        ] {
            let mut v: Vec<_> = map.into_iter().collect();
            v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
            writeln!(r, "{title}").unwrap();
            writeln!(r, "{:>8} {:>6} {:>10}  function", "samples", "%", "cpu ms").unwrap();
            for (id, (n, ms)) in v.iter().take(40) {
                writeln!(r, "{:>8} {:>6.2} {:>10.1}  {}", n, 100.0 * *n as f64 / total_busy as f64, ms, trunc(&sy.names[*id as usize], 160)).unwrap();
            }
            writeln!(r).unwrap();
        }
    }

    std::fs::write(&args.out, &r).map_err(|e| format!("writing {}: {e}", args.out))?;

    // ---- CSV of raw samples
    let csv_path = match args.out.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() && !stem.ends_with(['\\', '/']) => format!("{stem}.csv"),
        _ => format!("{}.csv", args.out),
    };
    let f = std::fs::File::create(&csv_path).map_err(|e| format!("creating {csv_path}: {e}"))?;
    let mut w = std::io::BufWriter::new(f);
    let wr = |w: &mut std::io::BufWriter<std::fs::File>| -> std::io::Result<()> {
        writeln!(w, "tid,thread,t_ms,busy,cpu_us,rip,module,function,offset,stack")?;
        for (s, rr) in samples.iter().zip(&res) {
            let t = &threads[s.th as usize];
            let (mname, off) = match rr.self_mod {
                Some(i) => (mods.v[i].name.as_str(), s.rip - mods.v[i].base),
                None => ("<unknown>", s.rip),
            };
            let stack = rr.stack.iter().skip(1).map(|x| sy.names[x.0 as usize].as_str()).collect::<Vec<_>>().join(" <- ");
            writeln!(w, "{},{},{:.1},{},{:.1},{:#x},{},{},{:#x},{}", t.tid, csv(&t.name), s.t_ms, s.busy as u8, cpu_ms(s) * 1000.0, s.rip, csv(mname), csv(&sy.names[rr.self_id as usize]), off, csv(&stack))?;
        }
        w.flush()
    };
    wr(&mut w).map_err(|e| format!("writing {csv_path}: {e}"))?;
    unsafe { SymCleanup(hp_raw) };
    eprintln!("report: {}\nraw samples: {}", args.out, csv_path);
    if RESUME_FAILURES.load(Ordering::SeqCst) > 0 {
        eprintln!("WARNING: {} ResumeThread calls failed", RESUME_FAILURES.load(Ordering::SeqCst));
    }
    Ok(())
}

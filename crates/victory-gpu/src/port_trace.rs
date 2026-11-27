//! Diagnostics (`VICTORY_PORT_DESCRIPTORS_TRACE=1`, or `=<ms>`, with renderer
//! port #2 on): who writes the view records into the title's heap that the
//! six patched stores do not tell of.
//!
//! Renderer port #2's fallback names slots it found holding a view record no
//! mark told of ([`candidate`]). The title's frame allocator hands the same
//! memory out again frames later, and whatever wrote such a record may write
//! there again. Hardware data breakpoints (the debug registers DR0-DR3, set
//! on every thread of the process) watch the first eight bytes of up to four
//! of the newest such slots; a write there stops the writing thread right
//! after the store, with a single-step exception, where the address after
//! the store, the frames above it and the bytes now there are noted. A thread
//! notes one write per watched slot and stops watching it itself; every
//! period (50 ms, or the milliseconds given) the watch moves to the newest
//! slots, on every thread again. A line a second says what wrote: the patched
//! stores' code (no module: the stubs `victory_loader::descriptor_sites`
//! makes), the title's own code, ours, the runtime's; and, since the start,
//! which places wrote view records, with the title's code that called them.
//!
//! Debug registers are the thread's own: nothing else of the layer or the
//! title uses them. The handler is put ahead of the guards' (`crate::watch`),
//! which would take an exception inside our own copies for a failed copy.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

/// Whether the trace is asked for, and its period in milliseconds.
fn period_ms() -> Option<u64> {
    static MS: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *MS.get_or_init(|| {
        let v = std::env::var("VICTORY_PORT_DESCRIPTORS_TRACE").ok()?;
        match v.trim() {
            "" | "0" => None,
            "1" => Some(50),
            ms => ms.parse::<u64>().ok().map(|ms| ms.clamp(5, 10_000)),
        }
    })
}

/// Slots the fallback found with a view record no mark told of, newest last.
const CANDIDATES: usize = 64;
static CANDIDATE: [AtomicU64; CANDIDATES] = [const { AtomicU64::new(0) }; CANDIDATES];
static NEXT_CANDIDATE: AtomicUsize = AtomicUsize::new(0);

/// A slot at `handle` held a view record no mark told of: one to watch.
pub(crate) fn candidate(handle: u64) {
    if period_ms().is_none() {
        return;
    }
    start();
    let at = NEXT_CANDIDATE.fetch_add(1, Relaxed);
    CANDIDATE[at % CANDIDATES].store(handle & !7, Release);
}

/// The addresses watched now, a debug register each (0: none).
static ARMED: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// `DR7` for watching writes to the four bytes at each nonzero address of
/// `armed` (a record's tag: a write of other data after it, leaving an old
/// tag in place, is not one): a local enable bit each (bit 2n), its condition
/// 01 (writes, bits 16 + 4n) and its length 11 (four bytes, bits 18 + 4n).
pub(crate) fn dr7_for(armed: [u64; 4]) -> u64 {
    let mut dr7 = 0u64;
    for (n, &at) in armed.iter().enumerate() {
        if at != 0 {
            dr7 |= 1 << (2 * n) | 0b01 << (16 + 4 * n) | 0b11 << (18 + 4 * n);
        }
    }
    dr7
}

/// Frames noted per write.
const FRAMES: usize = 8;
/// Words of a noted write: the address, the thread, the sixteen bytes there
/// after the write, the frames (the address after the store first).
const WORDS: usize = 4 + FRAMES;
/// Writes kept until a line reads them.
const EVENTS: usize = 4096;
static EVENT: [[AtomicU64; WORDS]; EVENTS] = [const { [const { AtomicU64::new(0) }; WORDS] }; EVENTS];
/// Of each place, the number of the write in it, plus one, once written whole.
static EVENT_SEQ: [AtomicU64; EVENTS] = [const { AtomicU64::new(0) }; EVENTS];
static NEXT_EVENT: AtomicU64 = AtomicU64::new(0);
/// The writes the lines have read up to.
static READ_UPTO: AtomicU64 = AtomicU64::new(0);

/// The handler and the arming thread, once.
fn start() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        install();
        let period = period_ms().unwrap_or(50);
        let spawned = std::thread::Builder::new().name("victory-port2-trace".into()).spawn(move || arm_every(period));
        if let Err(e) = spawned {
            tracing::warn!("renderer port 2's writer trace: no thread to arm the watch ({e})");
        }
        tracing::info!(period_ms = period, "renderer port 2's writer trace (VICTORY_PORT_DESCRIPTORS_TRACE): debug registers watch slots the fallback found unmarked");
    });
}

/// Puts [`on_step`] ahead of every other vectored handler.
fn install() {
    use windows_sys::Win32::Foundation::{EXCEPTION_SINGLE_STEP, NTSTATUS};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, CONTEXT_DEBUG_REGISTERS_AMD64, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
        EXCEPTION_POINTERS,
    };
    unsafe extern "system" fn on_step(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes the exception being dispatched.
        let (record, context) = unsafe { (&*(*info).ExceptionRecord, &mut *(*info).ContextRecord) };
        if record.ExceptionCode != EXCEPTION_SINGLE_STEP as NTSTATUS {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let hits = context.Dr6 & 0xf;
        let watched = [context.Dr0, context.Dr1, context.Dr2, context.Dr3];
        // A thread whose registers we set and that stops with no hit said:
        // ours too (nothing else here steps threads), taken and gone on from.
        let mut ours = hits == 0 && context.Dr7 & 0xff != 0 && ARMED.iter().any(|a| a.load(Relaxed) != 0);
        let mut noted = [0u64; 4];
        for (n, &at) in watched.iter().enumerate() {
            let armed = ARMED[n].load(Relaxed);
            if hits >> n & 1 == 0 || (at == 0 && armed == 0) {
                continue;
            }
            ours = true;
            noted[n] = if at != 0 { at } else { armed };
            // This thread watches that slot no more until the next arming.
            context.Dr7 &= !(1 << (2 * n));
        }
        if !ours {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        context.Dr6 = 0;
        context.ContextFlags |= CONTEXT_DEBUG_REGISTERS_AMD64;
        // Never inside itself: a write the noting makes (a stack the watch
        // lies on after the title remapped the heap's pages) is let go.
        if !INSIDE.with(|inside| inside.replace(true)) {
            for &at in noted.iter().filter(|&&at| at != 0) {
                // SAFETY: the thread just wrote there; the context is its own.
                unsafe { note(at, context) };
            }
            INSIDE.with(|inside| inside.set(false));
        }
        EXCEPTION_CONTINUE_EXECUTION
    }
    // SAFETY: a handler that only reads the exception it is given and the
    // eight bytes the watched write just wrote.
    unsafe { AddVectoredExceptionHandler(1, Some(on_step)) };
}

thread_local! {
    /// Whether this thread is noting a write ([`install`]'s handler).
    static INSIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Notes the write that stopped `context`'s thread at the watched `at`.
///
/// # Safety
/// `at` was just written by this thread; `context` is its state after the
/// store. Allocates nothing (the writing thread may hold any lock).
unsafe fn note(at: u64, context: &windows_sys::Win32::System::Diagnostics::Debug::CONTEXT) {
    use windows_sys::Win32::System::Threading::{GetCurrentThreadId, GetCurrentThreadStackLimits};
    let n = NEXT_EVENT.fetch_add(1, Relaxed);
    let place = (n % EVENTS as u64) as usize;
    let (mut low, mut high) = (0usize, 0usize);
    // SAFETY: writes the current thread's stack bounds.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    let mut frames = [0u64; FRAMES];
    frames[0] = context.Rip;
    // SAFETY: reads only this thread's stack, between its pointer and its base.
    unsafe { title_words(context.Rsp as usize, low, high, &mut frames[1..]) };
    // The watched slot is 32 bytes on a 32-byte boundary: its sixteen bytes
    // from `at` are on the page just written.
    let slot = at & !31;
    // SAFETY: the bytes of the slot the thread just wrote to.
    let (first, second) = unsafe { ((slot as *const u64).read_volatile(), ((slot + 8) as *const u64).read_volatile()) };
    // SAFETY: on this thread.
    let thread = u64::from(unsafe { GetCurrentThreadId() });
    let words = [at, thread, first, second];
    for (cell, value) in EVENT[place].iter().zip(words.into_iter().chain(frames)) {
        cell.store(value, Relaxed);
    }
    EVENT_SEQ[place].store(n + 1, Release);
}

/// The words on the stack from `rsp` up (2 KiB at most, inside `low..high`)
/// that are addresses in the title's image, likely its return addresses,
/// into `out` in the order found. Not a walk by unwind data: a write the
/// watch stops may be in the system's own exception dispatch, where such a
/// walk can fault inside this handler. Allocates nothing.
///
/// # Safety
/// `low..high` is the current thread's stack and `rsp` its pointer.
unsafe fn title_words(rsp: usize, low: usize, high: usize, out: &mut [u64]) {
    let (base, size) = crate::port_descriptors::title_image();
    if size == 0 || rsp < low || rsp >= high {
        return;
    }
    let (mut at, end) = (rsp & !7, high.min(rsp.saturating_add(0x800)));
    let mut n = 0;
    while at + 8 <= end && n < out.len() {
        // SAFETY: between the stack pointer and the stack's base: committed.
        let word = unsafe { (at as *const usize).read() };
        if word >= base && word < base + size {
            out[n] = word as u64;
            n += 1;
        }
        at += 8;
    }
}

/// Armings so far and threads armed by the last one.
static ARMINGS: AtomicU64 = AtomicU64::new(0);
static THREADS: AtomicU64 = AtomicU64::new(0);

/// Every `period` milliseconds: the newest candidates watched, on every
/// thread of the process but this one.
fn arm_every(period: u64) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(period));
        let newest = NEXT_CANDIDATE.load(Relaxed);
        let mut armed = [0u64; 4];
        let mut taken = 0;
        for back in 1..=CANDIDATES.min(newest) {
            let at = CANDIDATE[(newest - back) % CANDIDATES].load(Acquire);
            if at != 0 && !armed[..taken].contains(&at) {
                armed[taken] = at;
                taken += 1;
                if taken == armed.len() {
                    break;
                }
            }
        }
        if taken == 0 {
            continue;
        }
        for (cell, &at) in ARMED.iter().zip(&armed) {
            cell.store(at, Relaxed);
        }
        let threads = arm_all(armed);
        ARMINGS.fetch_add(1, Relaxed);
        THREADS.store(threads, Relaxed);
    }
}

/// Sets the debug registers of every thread of the process but this one to
/// watch `armed`; how many it set.
fn arm_all(armed: [u64; 4]) -> u64 {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    // SAFETY: plain queries of this process and thread.
    let (process, me) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
    // SAFETY: a snapshot of the system's threads, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return 0;
    }
    let mut ids: Vec<u32> = Vec::with_capacity(512);
    // SAFETY: a zeroed entry with its size set, as the walk wants.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    // SAFETY: the snapshot is open and the entry sized.
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == process && entry.th32ThreadID != me {
            ids.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: opened above.
    unsafe { CloseHandle(snapshot) };
    let dr7 = dr7_for(armed);
    ids.into_iter().filter(|&id| arm_thread(id, armed, dr7)).count() as u64
}

/// Sets thread `id`'s debug registers to watch `armed` (`dr7` its control
/// word): suspended only between reading and writing its context, during
/// which nothing here allocates or locks.
fn arm_thread(id: u32, armed: [u64; 4], dr7: u64) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, CONTEXT_DEBUG_REGISTERS_AMD64, GetThreadContext, SetThreadContext};
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME};
    /// `GetThreadContext` wants a 16-byte aligned context.
    #[repr(C, align(16))]
    struct Aligned(CONTEXT);
    // SAFETY: a thread of this process, closed below.
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT, 0, id) };
    if thread.is_null() {
        return false;
    }
    // SAFETY: a zeroed CONTEXT is valid; the thread is another of this
    // process, resumed right after its debug registers are set.
    unsafe {
        let mut context = Aligned(std::mem::zeroed());
        context.0.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
        let mut set = false;
        if SuspendThread(thread) != u32::MAX {
            if GetThreadContext(thread, &mut context.0) != 0 {
                let c = &mut context.0;
                [c.Dr0, c.Dr1, c.Dr2, c.Dr3] = armed;
                c.Dr6 = 0;
                c.Dr7 = dr7;
                set = SetThreadContext(thread, &context.0) != 0;
            }
            ResumeThread(thread);
        }
        CloseHandle(thread);
        set
    }
}

/// Where an address is: in the title's image, ours, another module (the
/// runtime's), or no module (the patched stores' stubs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Stub,
    Title,
    Ours,
    Runtime,
}

/// `module+offset` (`title+offset` in the title's image), or the bare
/// address, and which [`Place`] it is.
fn locate(address: u64) -> (String, Place) {
    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW,
        GetModuleHandleW,
    };
    let (base, size) = crate::port_descriptors::title_image();
    let at = address as usize;
    if at >= base && at < base + size {
        return (format!("title+{:#x}", at - base), Place::Title);
    }
    let mut module: HMODULE = std::ptr::null_mut();
    // SAFETY: looks up the module holding `address`; no reference taken.
    let found = unsafe {
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, at as *const u16, &mut module)
    };
    if found == 0 {
        return (format!("{at:#x}"), Place::Stub);
    }
    let mut path = [0u16; 260];
    // SAFETY: a loaded module; the buffer's length is passed.
    let len = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&path[..len.min(path.len())]);
    let name = path.rsplit('\\').next().unwrap_or(&path).to_owned();
    // SAFETY: the process's own executable.
    let ours = module == unsafe { GetModuleHandleW(std::ptr::null()) };
    (format!("{name}+{:#x}", at - module as usize), if ours { Place::Ours } else { Place::Runtime })
}

/// Since the start: of the writes that left a view record, how many by each
/// writer and the title's code that called it, with one walk of frames each.
type Writers = std::collections::HashMap<(String, String), (u64, Vec<String>)>;
static WRITERS: std::sync::Mutex<Option<Writers>> = std::sync::Mutex::new(None);

/// The trace's line of the frame lines, per frame (`frames` of them since the last).
pub(crate) fn line(frames: f64) {
    if period_ms().is_none() {
        return;
    }
    let upto = NEXT_EVENT.load(Acquire);
    let from = READ_UPTO.swap(upto, Relaxed).max(upto.saturating_sub(EVENTS as u64));
    // Writes by place (stubs, title, ours, runtime), of them those that left a
    // live view record, and those that left a tag over other data.
    let (mut all, mut views, mut stale) = ([0u64; 4], [0u64; 4], 0u64);
    let mut writers = WRITERS.lock().unwrap_or_else(|e| e.into_inner());
    let writers = writers.get_or_insert_with(Writers::default);
    for n in from..upto {
        let place = (n % EVENTS as u64) as usize;
        if EVENT_SEQ[place].load(Acquire) != n + 1 {
            continue;
        }
        let words: Vec<u64> = EVENT[place].iter().map(|w| w.load(Relaxed)).collect();
        // A live view record now (a tag over words that name no live view
        // is other data over an old record).
        let kind = crate::descriptor::record_kind([words[2] as u32, (words[2] >> 32) as u32, words[3] as u32]);
        if kind == 1 {
            stale += 1;
        }
        let view = kind >= 2;
        let frames: Vec<u64> = words[4..].iter().copied().take_while(|&f| f != 0).collect();
        let Some(&first) = frames.first() else { continue };
        let (writer, at) = locate(first);
        let kind = match at {
            Place::Stub => 0,
            Place::Title => 1,
            Place::Ours => 2,
            Place::Runtime => 3,
        };
        all[kind] += 1;
        if !view {
            continue;
        }
        views[kind] += 1;
        // The title's code the writer was called from, if the writer is not the title's.
        let caller = if at == Place::Title {
            String::new()
        } else {
            frames[1..].iter().map(|&f| locate(f)).find(|(_, p)| *p == Place::Title).map(|(s, _)| s).unwrap_or_default()
        };
        let seen = writers.entry((writer, caller)).or_insert_with(|| (0, Vec::new()));
        seen.0 += 1;
        if seen.1.is_empty() {
            seen.1 = frames.iter().map(|&f| locate(f).0).collect();
        }
    }
    let mut top: Vec<(&(String, String), &(u64, Vec<String>))> = writers.iter().collect();
    top.sort_by_key(|a| std::cmp::Reverse(a.1.0));
    let named: Vec<String> = top
        .iter()
        .take(8)
        .map(|((writer, caller), (n, walk))| {
            let from = if caller.is_empty() { String::new() } else { format!(" called from {caller}") };
            format!("{writer}{from} {n} [{}]", walk.join(" <- "))
        })
        .collect();
    let f = frames.max(1.0);
    tracing::info!(
        "renderer port 2's writer trace (VICTORY_PORT_DESCRIPTORS_TRACE) per frame: {:.2} armings over {} threads; writes caught {:.2} \
         (the patched stores' code {:.2}, the title's {:.2}, ours {:.2}, the runtime's {:.2}), of them leaving a live view record \
         {:.2} (stores {:.2}, title {:.2}, ours {:.2}, runtime {:.2}), a tag over other data {:.2}; since the start, the writers of live view records: {}",
        ARMINGS.swap(0, Relaxed) as f64 / f,
        THREADS.load(Relaxed),
        all.iter().sum::<u64>() as f64 / f,
        all[0] as f64 / f,
        all[1] as f64 / f,
        all[2] as f64 / f,
        all[3] as f64 / f,
        views.iter().sum::<u64>() as f64 / f,
        views[0] as f64 / f,
        views[1] as f64 / f,
        views[2] as f64 / f,
        views[3] as f64 / f,
        stale as f64 / f,
        if named.is_empty() { "none yet".into() } else { named.join("; ") }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dr7_watches_four_byte_writes_where_asked() {
        assert_eq!(dr7_for([0; 4]), 0);
        // DR0: L0 (bit 0), write (01 at 16), four bytes (11 at 18).
        assert_eq!(dr7_for([0x1000, 0, 0, 0]), 0b1 | 0b01 << 16 | 0b11 << 18);
        // DR2: L2 (bit 4), write (01 at 24), four bytes (11 at 26).
        assert_eq!(dr7_for([0, 0, 0x2000, 0]), 0b1 << 4 | 0b01 << 24 | 0b11 << 26);
        assert_eq!(dr7_for([8, 16, 24, 32]), 0x55 | 0xdddd << 16);
    }

    /// A record's slot: 32 bytes on a 32-byte boundary, as in the title's heap.
    #[repr(C, align(32))]
    struct Slot([u64; 4]);

    /// A write to a watched slot's tag by another thread is caught once, with
    /// the bytes it left and the writer's place; the next write is not, nor a
    /// write after the tag.
    #[test]
    fn a_watched_write_is_caught_once_with_its_writer() {
        use std::sync::mpsc::channel;
        install();
        let slot: &'static mut Slot = Box::leak(Box::new(Slot([0u64; 4])));
        let at = slot.0.as_ptr() as u64;
        let target = at as usize;
        let (id_tx, id_rx) = channel();
        let (go_tx, go_rx) = channel::<()>();
        let (done_tx, done_rx) = channel();
        let writer = std::thread::spawn(move || {
            // SAFETY: on this thread.
            id_tx.send(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }).unwrap();
            go_rx.recv().unwrap();
            let p = target as *mut u64;
            // SAFETY: the leaked slot, written by this thread alone now.
            unsafe {
                // After the tag: not watched.
                p.cast::<u32>().add(1).write_volatile(9);
                p.write_volatile(u64::from(crate::descriptor::VIEW_TAG) | 7 << 32);
                p.write_volatile(u64::from(crate::descriptor::VIEW_TAG) | 8 << 32);
            }
            done_tx.send(()).unwrap();
        });
        let id = id_rx.recv().unwrap();
        let before = NEXT_EVENT.load(Acquire);
        ARMED[0].store(at, Relaxed);
        assert!(arm_thread(id, [at, 0, 0, 0], dr7_for([at, 0, 0, 0])));
        go_tx.send(()).unwrap();
        done_rx.recv().unwrap();
        writer.join().unwrap();
        ARMED[0].store(0, Relaxed);
        let after = NEXT_EVENT.load(Acquire);
        let ours: Vec<usize> = (before..after)
            .map(|n| (n % EVENTS as u64) as usize)
            .filter(|&p| EVENT[p][0].load(Relaxed) == at && EVENT[p][1].load(Relaxed) == u64::from(id))
            .collect();
        assert_eq!(ours.len(), 1, "one write caught, the second not");
        let p = ours[0];
        // The first write's bytes: the watch stops the thread right after it.
        assert_eq!(EVENT[p][2].load(Relaxed), u64::from(crate::descriptor::VIEW_TAG) | 7 << 32);
        assert_ne!(EVENT[p][4].load(Relaxed), 0, "the writer's place");
        assert_eq!(slot.0[0], u64::from(crate::descriptor::VIEW_TAG) | 8 << 32);
    }
}

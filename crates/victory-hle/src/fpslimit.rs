//! (`VICTORY_FPS`) The title's own frame limiter, let through in light
//! frames.
//!
//! Besides our frame waits, the title holds its frames to its frame
//! interval itself: 0x140120530, called at the top of each frame of
//! 0x144fd1b20 (the call at 0x144fd1b5f), compares the time since the last
//! frame it let through (QueryPerformanceCounter) with a target of
//! 1000 / trunc(1e6 / [0x1498f84ac]) ms, the interval in microseconds the
//! title set (33,333: 33.3 ms). While more than 2 ms are left it sleeps
//! 2 ms and answers "not yet" (the caller skips the frame and comes back);
//! closer, it spins to the target. It runs while the byte [0x147453ec9] is
//! set, which it is in the loading screens and in the world (read from the
//! running title). That held a loading screen of 14 draws a frame to
//! 30 a second with our waits at 60 (timeline: its thread sleeping in 2 ms
//! steps, then waking the presenting one, whose frames our waits then held
//! to 16.7 ms).
//!
//! While our light frames run (`victory_gpu::frame::light_frames`), the
//! call sees the byte clear for its length and lets the frame through at
//! once; the byte is put back when it returns, so the frame is paced by our
//! waits alone. On the first frame that is not light the limiter runs again,
//! starting over as when the title sets the byte (its first target then
//! takes a frame up to about 55 ms). The world's frames are never let
//! through. The call and the limiter's read of the byte are checked before
//! anything is changed.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use victory_loader::callhook::{self, Regs};

/// Where the title's image was meant to load.
const PREFERRED: usize = 0x1_4000_0000;
/// `call 0x140120530`, in 0x144fd1b20 (RVAs from here on).
const SITE: usize = 0x04fd_1b5f;
/// The limiter.
const LIMITER: usize = 0x0012_0530;
/// In the limiter: `mov bl, byte ptr [rip+disp32]`, the byte read.
const READS_BYTE: usize = 0x0012_0562;
/// The byte that turns the limiter on.
const BYTE: usize = 0x0745_3ec9;

static BASE: AtomicUsize = AtomicUsize::new(0);
static AFTER: AtomicUsize = AtomicUsize::new(0);
/// Calls let through.
static LET_THROUGH: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Return addresses taken, with the byte's value to put back.
    static TAKEN: RefCell<Vec<(u64, u8)>> = const { RefCell::new(Vec::new()) };
}

fn byte() -> *mut u8 {
    (BASE.load(Ordering::Relaxed) + BYTE) as *mut u8
}

unsafe extern "C" fn limiter_called(regs: *mut Regs) {
    if !victory_gpu::frame::light_frames() {
        return;
    }
    let at = byte();
    // SAFETY: the title's byte in its image (checked at install); written
    // by the title's frame thread, this one.
    let was = unsafe { at.read_volatile() };
    if was == 0 {
        return;
    }
    // SAFETY: as above.
    unsafe { at.write_volatile(0) };
    // SAFETY: the thunk's saved registers; `ret` is the return address.
    let regs = unsafe { &mut *regs };
    TAKEN.with(|t| t.borrow_mut().push((regs.ret, was)));
    regs.ret = AFTER.load(Ordering::Relaxed) as u64;
    let n = LET_THROUGH.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 || n.is_multiple_of(10_000) {
        tracing::info!(
            n,
            "VICTORY_FPS: the title's frame limiter let a light frame through"
        );
    }
}

unsafe extern "C" fn limiter_returned(_rax: u64, slot: *mut u64) {
    let Some((ret, was)) = TAKEN.with(|t| t.borrow_mut().pop()) else {
        // Nowhere to return to: cannot happen (each stub return was pushed).
        std::process::abort();
    };
    let at = byte();
    // SAFETY: as in `limiter_called`. The title's value back, unless the
    // title set the byte itself meanwhile.
    unsafe {
        if at.read_volatile() == 0 {
            at.write_volatile(was);
        }
        *slot = ret;
    }
}

/// `VICTORY_FPS`'s rate (`victory_gpu::frame::light_fps`), for the loader's
/// patch of the simulation's shortest step.
pub fn light_fps() -> Option<u32> {
    victory_gpu::frame::light_fps()
}

/// Frames begun so far (for `VICTORY_TIMER_LOG`).
pub fn frames_begun() -> u64 {
    victory_gpu::frame::current()
}

/// The limiter's read of its byte: `mov bl, byte ptr [rip+disp32]`.
fn reads_byte() -> [u8; 6] {
    let disp = (BYTE as i64 - (READS_BYTE + 6) as i64) as i32;
    let d = disp.to_le_bytes();
    [0x8A, 0x1D, d[0], d[1], d[2], d[3]]
}

/// Redirects the limiter's call when `VICTORY_FPS` asks for our pacing of
/// light frames; else changes nothing.
///
/// # Safety
/// `base` is the title's mapped image, and no title code runs yet.
pub unsafe fn install(base: usize) {
    if !victory_gpu::frame::fps_asked() {
        return;
    }
    BASE.store(base, Ordering::Relaxed);
    // SAFETY: inside the mapped image (the limiter is in its .text).
    let found = unsafe { std::slice::from_raw_parts((base + READS_BYTE) as *const u8, 6) };
    if found != reads_byte() {
        tracing::warn!(
            found = format_args!("{found:02x?}"),
            "VICTORY_FPS: the title's frame limiter not let through: its bytes differ (another build?)"
        );
        return;
    }
    let Ok(stub) = callhook::after_stub(limiter_returned) else {
        tracing::warn!("VICTORY_FPS: the title's frame limiter not let through: no stub");
        return;
    };
    AFTER.store(stub, Ordering::Relaxed);
    // SAFETY: the caller's promise; `redirect_call` checks the call.
    match unsafe { callhook::redirect_call(base + SITE, base + LIMITER, limiter_called) } {
        Ok(()) => tracing::info!(
            at = format_args!("{:#x}", PREFERRED + SITE),
            "VICTORY_FPS: the title's frame limiter is let through in light frames"
        ),
        Err(e) => tracing::warn!(
            error = %e,
            "VICTORY_FPS: the title's frame limiter not let through: the call differs (another build?)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_reads_its_byte_as_the_title_has_it() {
        // 0x140120562: 8a 1d 61 39 33 07 (mov bl, [0x147453ec9]).
        assert_eq!(reads_byte(), [0x8A, 0x1D, 0x61, 0x39, 0x33, 0x07]);
    }
}

//! Frame pacing: the title sets a frame interval and waits for each frame
//! to begin. Ours is a clock started by the first interval set, ticking
//! once per interval; the token is the frame number. Each event kind
//! (origin, vblank, flip) happens once per tick, so two waits for the same
//! kind never end on the same tick: the title polls the origin
//! with "token on time-out" (starting its next frame at once) and is paced
//! by a blocking wait for the flip before each present.
//!
//! shortcut: event offsets are ignored (every event is at the tick); tie
//! the clock to presentation if a title paces by them.
//!
//! `VICTORY_FPS=N` (a test switch, off by default): light frames, those after
//! 8 frames in a row of at most `VICTORY_FPS_DRAWS` draws each (100; `all`:
//! every frame), such as the title's videos and loading screens, are only
//! held to at least 1/N s each, with no rounding to the title's ticks (a
//! frame of 20 ms shows at 50 a second, not 30); `VICTORY_FPS=0` does not
//! hold them. The first frame with more draws is back on the title's
//! interval, and the world's frames stay there. A title that steps what it
//! shows by the interval it set runs that much faster in light frames.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::object::{HRESULT, S_FALSE, S_OK, This};

/// What a wait returns when its time is up first.
const TIMEOUT: HRESULT = S_FALSE;
/// The frame-origin event: the one that starts frames and makes tokens.
const ORIGIN: u32 = 0;
/// Make the frame's token even if the wait times out (the title starts the
/// frame early).
const TOKEN_ON_TIMEOUT: u32 = 2;
const SIXTIETH: Duration = Duration::from_micros(16_667);

struct Clock {
    start: Instant,
    interval: Duration,
    /// The tick number at `start` (ticks go on across interval changes).
    first: u64,
}

static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);
static FRAME: AtomicU64 = AtomicU64::new(0);

fn clock() -> std::sync::MutexGuard<'static, Option<Clock>> {
    CLOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `VICTORY_FPS`: our pacing of light frames in place of the title's.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rate {
    /// Unset: the title's interval for every frame.
    Title,
    /// Frames after one of at most `draws` draws are held to at least
    /// `least` each (zero: not held); the others keep the title's interval.
    Light { least: Duration, draws: u32 },
}

/// Light frames: those after a frame of at most this many draws (the
/// title's videos and loading screens drew 3 to 19 a frame, its world and
/// the menus over it thousands).
const LIGHT_DRAWS: u32 = 100;

fn rate_from(fps: Option<&str>, draws: Option<&str>) -> Rate {
    let least = match fps.map(str::trim).map(str::parse::<u32>) {
        Some(Ok(0)) => Duration::ZERO,
        Some(Ok(n)) => Duration::from_nanos(1_000_000_000 / u64::from(n)),
        _ => return Rate::Title,
    };
    let draws = match draws.map(str::trim) {
        Some("all") => u32::MAX,
        Some(d) => d.parse().unwrap_or(LIGHT_DRAWS),
        None => LIGHT_DRAWS,
    };
    Rate::Light { least, draws }
}

fn rate() -> Rate {
    static RATE: OnceLock<Rate> = OnceLock::new();
    *RATE.get_or_init(|| {
        let var = |name: &str| std::env::var(name).ok();
        let rate = rate_from(
            var("VICTORY_FPS").as_deref(),
            var("VICTORY_FPS_DRAWS").as_deref(),
        );
        if rate != Rate::Title {
            tracing::warn!(
                ?rate,
                "VICTORY_FPS: light frames are not paced by the title's interval"
            );
        }
        rate
    })
}

/// Light frames in a row before ours begin: one frame of few draws amid
/// the world's (a cut to black) keeps the title's ticks. The first frame
/// with more draws ends them.
const LIGHT_AFTER: u32 = 8;

/// Light frames in a row so far.
static LIGHT_RUN: AtomicU32 = AtomicU32::new(0);

fn next_run(run: u32, draws: u32, most: u32) -> u32 {
    if draws <= most {
        run.saturating_add(1)
    } else {
        0
    }
}

/// A frame began; the one before drew `draws`.
fn count_light(draws: u32) {
    if let Rate::Light { draws: most, .. } = rate() {
        let run = next_run(LIGHT_RUN.load(Ordering::Relaxed), draws, most);
        LIGHT_RUN.store(run, Ordering::Relaxed);
    }
}

/// For a light frame (the last `LIGHT_AFTER` frames drew at most the
/// switch's draws each), the least time its waits hold it to; `None`: the
/// title's ticks.
fn light() -> Option<Duration> {
    match rate() {
        Rate::Title => None,
        Rate::Light { least, .. } => {
            (LIGHT_RUN.load(Ordering::Relaxed) >= LIGHT_AFTER).then_some(least)
        }
    }
}

/// Whether `VICTORY_FPS` asks for our pacing of light frames (the title's
/// own frame limiter is then let through in them: `victory_hle`'s
/// `fpslimit`).
pub fn fps_asked() -> bool {
    rate() != Rate::Title
}

/// Whether light frames run now (`VICTORY_FPS`, the last `LIGHT_AFTER`
/// frames light).
pub fn light_frames() -> bool {
    light().is_some()
}

/// Whether the last frame wait was a light frame's.
static WAS_LIGHT: AtomicBool = AtomicBool::new(false);
/// Changes between light frames and the title's ticks.
static SWITCHES: AtomicU64 = AtomicU64::new(0);

/// Light frames begin or end (logged, the first 200 times). Light waits take
/// no ticks, so back on the title's the first wait ends at once on the tick
/// that passed, as after any long frame.
fn switched(light: bool) {
    if SWITCHES.fetch_add(1, Ordering::Relaxed) < 200 {
        let draws = PRESENTED_DRAWS.load(Ordering::Relaxed);
        let to = if light {
            "light frames, ours"
        } else {
            "the title's interval"
        };
        tracing::info!(draws, frame = current(), "VICTORY_FPS: {to}");
    }
}

/// Per event kind: when the last light wait for it ended (nanoseconds from
/// the first one; 0: none yet).
static LIGHT_END: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

fn light_now() -> u64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    (BASE.get_or_init(Instant::now).elapsed().as_nanos() as u64).max(1)
}

/// A light frame's wait: it ends `least` after the last one for `kind`
/// ended (at once if that has passed), or at the time-out first. Two waits
/// for one kind do not end on one turn: a loser waits its `least` again.
fn light_wait(kind: u32, least: Duration, limit: Duration, slept: &mut Duration) -> HRESULT {
    let end = &LIGHT_END[(kind as usize).min(LIGHT_END.len() - 1)];
    let least = least.as_nanos() as u64;
    loop {
        let before = end.load(Ordering::Relaxed);
        let now = light_now();
        let due = if before == 0 { now } else { before + least };
        let wait = Duration::from_nanos(due.saturating_sub(now));
        let left = limit.saturating_sub(*slept);
        if wait > left {
            std::thread::sleep(left);
            *slept += left;
            return TIMEOUT;
        }
        if !wait.is_zero() {
            std::thread::sleep(wait);
            *slept += wait;
        }
        if end
            .compare_exchange(before, light_now(), Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return S_OK;
        }
    }
}

/// Device method: a frame every `length` microseconds. The period is how
/// many intervals a frame may take to reach the display (pipeline depth),
/// not part of the rate.
pub(crate) extern "system" fn set_interval(
    _: This,
    _output: This,
    length: u32,
    _period: u32,
    _flags: u32,
) -> HRESULT {
    let interval = match length {
        0 => SIXTIETH,
        us => Duration::from_micros(u64::from(us)),
    };
    tracing::info!(?interval, "frame interval");
    // A new interval starts from now, after the last tick handed out.
    let first = LAST
        .iter()
        .map(|t| t.load(Ordering::Relaxed))
        .max()
        .unwrap_or(0);
    *clock() = Some(Clock {
        start: Instant::now(),
        interval,
        first,
    });
    S_OK
}

/// Device method: events are scheduled at every tick.
pub(crate) extern "system" fn schedule(
    _: This,
    _kind: u32,
    _offset: u32,
    _list: *const u8,
    _flags: u32,
) -> HRESULT {
    S_OK
}

/// Frames begun so far (frame waits that ended).
pub fn current() -> u64 {
    FRAME.load(Ordering::Relaxed)
}

/// For tests of what is done once a frame.
#[cfg(test)]
pub(crate) fn test_advance() {
    FRAME.fetch_add(1, Ordering::Relaxed);
}

/// Per event kind (origin, vblank, flip and later kinds): the last tick a
/// wait for it ended on.
static LAST: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

fn last(kind: u32) -> &'static AtomicU64 {
    &LAST[(kind as usize).min(LAST.len() - 1)]
}

/// The next tick after now and after the last one a wait for `kind` ended
/// on (an event happens once per tick: a wait woken a little early must not
/// let the next one end on the same tick), and how long until it.
fn next_tick(kind: u32) -> (u64, Duration) {
    let mut clock = clock();
    let c = clock.get_or_insert_with(|| Clock {
        start: Instant::now(),
        interval: SIXTIETH,
        first: 0,
    });
    let elapsed = c.start.elapsed().as_nanos();
    let interval = c.interval.as_nanos().max(1);
    let tick =
        (c.first + (elapsed / interval + 1) as u64).max(last(kind).load(Ordering::Relaxed) + 1);
    let at = u128::from(tick - c.first) * interval;
    (
        tick,
        Duration::from_nanos(at.saturating_sub(elapsed) as u64),
    )
}

/// Device method: blocks until the next tick (or the time-out); an origin
/// wait then hands out the new frame's token. Other events keep the token
/// the title passes. Ticks are absolute (start plus whole intervals), so
/// lateness never accumulates.
pub(crate) extern "system" fn wait(
    _: This,
    kind: u32,
    timeout_ms: u32,
    _list: *const u8,
    flags: u32,
    token: *mut u64,
) -> HRESULT {
    let origin = kind == ORIGIN;
    let (mut tick, mut wait) = next_tick(kind);
    let limit = Duration::from_millis(u64::from(timeout_ms));
    // A blocking wait for an event that has happened since the last wait
    // for it ended (a frame took longer than a tick) ends at once, on the
    // tick that passed: sleeping on to the next one cost 17 ms of every
    // 730 ms frame (run 58: "2.0 frame waits sleeping 16.8 ms"). Polls (a
    // time-out shorter than a tick) answer as before.
    // `VICTORY_LATE_WAITS=1` sleeps to the next tick, as before.
    static SLEEP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*SLEEP.get_or_init(|| std::env::var_os("VICTORY_LATE_WAITS").is_some_and(|v| v == "1"))
        && limit >= wait
        && tick > last(kind).load(Ordering::Relaxed) + 1
    {
        tick -= 1;
        wait = Duration::ZERO;
    }
    let _called = crate::timeline::span("the title's frame wait");
    let mut slept = Duration::ZERO;
    let light = light();
    if WAS_LIGHT.swap(light.is_some(), Ordering::Relaxed) != light.is_some() {
        switched(light.is_some());
    }
    let hr = if let Some(least) = light {
        light_wait(kind, least, limit, &mut slept)
    } else {
        // The tick is this waiter's only when it advanced `last`: two waits
        // that wake together otherwise both end on it. A loser waits again
        // on the next tick, until its deadline (measured from the call, so a
        // sleep that woke late does not stretch it).
        loop {
            if wait > limit.saturating_sub(slept) {
                std::thread::sleep(limit.saturating_sub(slept));
                break TIMEOUT;
            }
            std::thread::sleep(wait);
            slept += wait;
            if last(kind).fetch_max(tick, Ordering::Relaxed) < tick {
                break S_OK;
            }
            let (next, until) = next_tick(kind);
            (tick, wait) = (next, until);
        }
    };
    #[cfg(all(windows, feature = "diagnostics"))]
    {
        use crate::copystat::{C, add};
        add(C::PaceWaits, 1);
        add(C::PaceUs, slept.min(limit).as_micros() as u64);
    }
    #[cfg(windows)]
    {
        let us = slept.min(limit).as_micros() as u64;
        crate::framelog::add(crate::framelog::T::PaceUs, us);
        crate::framelog::here(crate::framelog::H::PaceUs, us);
    }
    // A frame begun on a time-out takes no tick: its flip still does.
    if origin && (hr == S_OK || flags & TOKEN_ON_TIMEOUT != 0) {
        // Tokens count frames (consecutive), not ticks.
        let frame = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
        if !token.is_null() {
            // SAFETY: checked non-null; the title passes its token.
            unsafe { token.write(frame) };
        }
        frame_began();
    }
    #[cfg(feature = "diagnostics")]
    tracing::debug!(
        kind,
        timeout_ms,
        flags,
        hr,
        tick,
        wait_us = wait.as_micros() as u64,
        frame = current(),
        "frame wait"
    );
    hr
}

/// Draws of the most recent frame (draws run on the host between two frame
/// beginnings: the per-frame number the window caption averages as "N
/// draws"), set when the title begins a frame; read by the scripted input
/// (`VICTORY_AUTO_INPUT` waits such as `d>=40`).
pub static PRESENTED_DRAWS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The draw total when the last frame began.
static DRAWN_AT_FRAME: AtomicU64 = AtomicU64::new(0);

/// (Fox port A) A frame begins (an origin wait made a new token): the
/// per-frame draw count for the scripted input, the frame log's frame end
/// (`VICTORY_FRAME_LOG`) and the hang watch's start (`VICTORY_HANG_WATCH_S`).
/// The fox base's host has no hooks of ours, so the frame boundary is the
/// title's frame wait, which it makes once a frame.
fn frame_began() {
    #[cfg(windows)]
    {
        let drawn = crate::draw::drawn();
        let before = DRAWN_AT_FRAME.swap(drawn, Ordering::Relaxed);
        let draws = drawn.saturating_sub(before).min(u64::from(u32::MAX)) as u32;
        PRESENTED_DRAWS.store(draws, Ordering::Relaxed);
        count_light(draws);
        crate::framelog::frame();
        crate::hangwatch::start();
    }
}

/// Device method: no statistics are kept, so none are returned.
pub(crate) extern "system" fn statistics(
    _: This,
    _token: u64,
    _kinds: u32,
    count: *mut u32,
    _out: *mut u8,
) -> HRESULT {
    if !count.is_null() {
        // SAFETY: checked non-null.
        unsafe { count.write(0) };
    }
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_switch_values() {
        assert_eq!(rate_from(None, None), Rate::Title);
        assert_eq!(rate_from(Some(""), Some("all")), Rate::Title);
        assert_eq!(rate_from(Some("fast"), None), Rate::Title);
        let light = |least, draws| Rate::Light { least, draws };
        assert_eq!(
            rate_from(Some("0"), None),
            light(Duration::ZERO, LIGHT_DRAWS)
        );
        assert_eq!(
            rate_from(Some(" 60 "), Some("40")),
            light(Duration::from_nanos(16_666_666), 40)
        );
        assert_eq!(
            rate_from(Some("120"), Some(" all ")),
            light(Duration::from_nanos(8_333_333), u32::MAX)
        );
        assert_eq!(
            rate_from(Some("30"), Some("many")),
            light(Duration::from_nanos(33_333_333), LIGHT_DRAWS)
        );
    }

    #[test]
    fn light_frames_begin_after_a_run_and_end_at_once() {
        let mut run = 0;
        for _ in 0..LIGHT_AFTER {
            run = next_run(run, 12, LIGHT_DRAWS);
        }
        assert_eq!(run, LIGHT_AFTER);
        // One frame of the world's draws ends the run.
        assert_eq!(next_run(run, 4_700, LIGHT_DRAWS), 0);
        // At the limit is still light; every frame when the limit is `all`.
        assert_eq!(next_run(0, LIGHT_DRAWS, LIGHT_DRAWS), 1);
        assert_eq!(next_run(0, 4_700, u32::MAX), 1);
        assert_eq!(next_run(u32::MAX, 0, LIGHT_DRAWS), u32::MAX);
    }

    #[test]
    fn light_waits_hold_frames_to_the_least_time() {
        // (The vblank kind: the tick test below uses origin and flip.)
        const VBLANK: u32 = 1;
        let least = Duration::from_millis(20);
        let long = Duration::from_secs(5);
        let mut slept = Duration::ZERO;
        let _ = light_wait(VBLANK, least, long, &mut slept);
        // The next one ends `least` after the last, not on a tick.
        let before = Instant::now();
        assert_eq!(light_wait(VBLANK, least, long, &mut slept), S_OK);
        let took = before.elapsed();
        assert!(took >= Duration::from_millis(15), "{took:?}");
        // A poll at once after it: not due yet.
        assert_eq!(
            light_wait(VBLANK, least, Duration::ZERO, &mut slept),
            TIMEOUT
        );
        // Not held (`VICTORY_FPS=0`): at once, even for a poll.
        let before = Instant::now();
        assert_eq!(
            light_wait(VBLANK, Duration::ZERO, Duration::ZERO, &mut slept),
            S_OK
        );
        assert!(before.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn waits_end_on_ticks_and_count_frames() {
        let null = std::ptr::null();
        assert_eq!(set_interval(null, null, 2_000, 1, 0), S_OK);
        let mut first = 0;
        assert_eq!(wait(null, 0, 1_000, std::ptr::null(), 0, &mut first), S_OK);
        let before = Instant::now();
        let mut second = 0;
        assert_eq!(wait(null, 0, 1_000, std::ptr::null(), 0, &mut second), S_OK);
        assert!(second > first);
        // At the tick, not at the time-out (a busy PC may wake it late).
        assert!(before.elapsed() <= Duration::from_millis(500));
        assert_eq!(wait(null, 0, 0, std::ptr::null(), 0, &mut second), TIMEOUT);
        // Tokens count frames, a frame started early on a time-out too.
        let mut early = 0;
        let flags = TOKEN_ON_TIMEOUT;
        assert_eq!(
            wait(null, 0, 0, std::ptr::null(), flags, &mut early),
            TIMEOUT
        );
        let mut next = 0;
        assert_eq!(wait(null, 0, 1_000, std::ptr::null(), 0, &mut next), S_OK);
        assert!(early == second + 1 && next == early + 1);
        assert!(LAST[0].load(Ordering::Relaxed) >= 2);

        // The title's loop (the clock is global, so in the same test): an
        // origin poll that times out (and starts the frame), then a
        // blocking flip wait. Each flip takes its own tick.
        const FLIP: u32 = 2;
        // (The ticks are far apart, and it may take a few tries: a busy PC
        // wakes a wait tens of milliseconds late at times.)
        assert_eq!(set_interval(null, null, 60_000, 3, 0), S_OK);
        let mut token = 0;
        let mut four = || -> Vec<u64> {
            (0..4)
                .map(|_| {
                    let _ = wait(null, 0, 0, std::ptr::null(), TOKEN_ON_TIMEOUT, &mut token);
                    assert_eq!(
                        wait(null, FLIP, u32::MAX, std::ptr::null(), 0, &mut token),
                        S_OK
                    );
                    LAST[2].load(Ordering::Relaxed)
                })
                .collect()
        };
        let mut flips = four();
        for _ in 0..10 {
            if flips.windows(2).all(|w| w[1] == w[0] + 1) {
                break;
            }
            flips = four();
        }
        assert!(flips.windows(2).all(|w| w[1] == w[0] + 1), "{flips:?}");
        // Even when a flip wait begins on the tick another just ended on.
        let (tick, _) = next_tick(FLIP);
        assert!(tick > flips[3]);

        // Two waits that wake together take two ticks: one does not end on
        // the tick another has claimed, both succeed, and the last tick a
        // wait ended on has moved two on. (Both waits start behind a
        // barrier, so they meet the tick together; the pre-claim code gave
        // both the same one and left the last one short.)
        assert_eq!(set_interval(null, null, 10_000, 2, 0), S_OK);
        let _ = wait(null, 0, 0, std::ptr::null(), TOKEN_ON_TIMEOUT, &mut token);
        let before = LAST[2].load(Ordering::Relaxed);
        let go = std::sync::Barrier::new(2);
        let results: Vec<HRESULT> = std::thread::scope(|s| {
            // `wait` does not dereference `this`; each thread builds its own
            // null (a raw pointer does not cross a closure capture's Send).
            let joins: Vec<_> = (0..2)
                .map(|_| {
                    s.spawn(|| {
                        let this: This = std::ptr::null();
                        let mut token = 0;
                        go.wait();
                        wait(this, FLIP, u32::MAX, std::ptr::null(), 0, &mut token)
                    })
                })
                .collect();
            joins.into_iter().map(|j| j.join().unwrap()).collect()
        });
        assert_eq!(results, [S_OK; 2]);
        assert_eq!(
            LAST[2].load(Ordering::Relaxed),
            before + 2,
            "each wait took its own tick"
        );
    }
}

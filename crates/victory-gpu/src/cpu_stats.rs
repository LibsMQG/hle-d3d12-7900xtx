//! `VICTORY_CPU_STATS=1` (off by default; release builds too): what the
//! submitting thread waits for when it waits for flights to land, and who
//! asks Windows what pages are (`VirtualQuery`) while looking for written
//! view memory. Counting only: nothing the layer does changes. Said per
//! frame every five seconds or so.
//!
//! Ported from the old line (`perf/card-check-63bf0b1`: ae4223c, 0696e77,
//! the counting half of 3562472) onto RYUKI's relay and steps on the card:
//! - each wait by what of the submission first met what flights write, and
//!   why the steps on the card (`host::card_steps`) did not take it instead;
//! - per wait, how many of the bytes it read the flights' shaders really
//!   changed while it waited (what lands, `shadow::put_changed`): a wait
//!   whose bytes nothing changed was only needed because of how wide the
//!   writes are declared;
//! - fills of upload memory by who made them, made and waited for;
//! - `VirtualQuery` askings of the write watch.
//!
//! The old line's snapshot comparison for refused forwards (def9cf9) has no
//! place here: RYUKI has no forward planning.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_CPU_STATS").is_some_and(|v| v == "1"))
}

/// What a wait for flights was for.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Reads {
    /// A title buffer copy's source.
    CopySource,
    /// A title buffer copy's destination.
    CopyDestination,
    /// A fill of upload memory; `from`: who made it ([`FILL_NAMES`]).
    UploadFill { from: u8 },
    Texture,
    MoviePlane,
    /// A buffer copy done at its place.
    PlacedCopy,
    /// A step reads where a copy at its place writes.
    PlacedRead,
    /// A host copy of what a copy done on the card wrote.
    CardCopyRead,
    /// No read named: a settle for another reason (a `WHY_` place of
    /// `crate::host`, as [`settle_name`] names it).
    Settle { why: u8 },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Blocker {
    pub(crate) reads: Reads,
    /// The title range read (0, 0 where not known).
    pub(crate) from: usize,
    pub(crate) len: usize,
}

/// Why the steps on the card left the submission to wait.
pub(crate) const NOT_TRIED: &str = "steps on the card not tried (off, or nothing written)";
pub(crate) const NOTHING_CHOSEN: &str = "steps on the card chose nothing";

thread_local! {
    static MET: Cell<Option<Blocker>> = const { Cell::new(None) };
    static REFUSED: Cell<&'static str> = const { Cell::new(NOT_TRIED) };
    /// This thread's entry in [`OPEN`], if it has one.
    static TOKEN: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The steps on the card are looked at for a new submission.
pub(crate) fn card_steps_begin() {
    if on() {
        REFUSED.set(NOT_TRIED);
    }
}

/// The steps on the card refused this thread's submission, for `why`.
pub(crate) fn card_refused(why: &'static str) {
    if on() {
        REFUSED.set(why);
    }
}

/// Waits being waited for now, for what shaders change under them: (token,
/// what is read, from, to, bytes merged into title memory inside `[from,
/// to)` meanwhile).
type Open = Vec<(u64, Reads, usize, usize, u64)>;
static OPEN: Mutex<Open> = Mutex::new(Vec::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Notes what this thread's preparing met first, for the wait that follows.
pub(crate) fn met(reads: Reads, from: usize, len: usize) {
    if !on() {
        return;
    }
    MET.set(Some(Blocker { reads, from, len }));
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    // A wait met before and not waited for is not followed further.
    if let Some(old) = TOKEN.replace(Some(token)) {
        close(&mut open, old);
    }
    open_wait(&mut open, token, reads, from, len);
}

fn open_wait(open: &mut Open, token: u64, reads: Reads, from: usize, len: usize) {
    open.push((token, reads, from, from.saturating_add(len), 0));
}

/// Merges into title memory over `[from, to)`: counted against every open
/// wait they meet (the bytes they share).
fn changed_under(open: &mut Open, from: usize, to: usize) {
    for e in open.iter_mut().filter(|e| from < e.3 && to > e.2) {
        e.4 += (to.min(e.3) - from.max(e.2)) as u64;
    }
}

/// Takes `token`'s entry out of `open`: (what was read, bytes changed).
fn close(open: &mut Open, token: u64) -> Option<(Reads, u64)> {
    let at = open.iter().position(|e| e.0 == token)?;
    let e = open.swap_remove(at);
    Some((e.1, e.4))
}

/// By what was read, since the last report: waits followed to their end, of
/// them those under which shaders changed the bytes read, and those bytes.
static CHANGED: Mutex<BTreeMap<Reads, [u64; 3]>> = Mutex::new(BTreeMap::new());

fn count_changed(all: &mut BTreeMap<Reads, [u64; 3]>, reads: Reads, bytes: u64) {
    let e = all.entry(reads).or_default();
    e[0] += 1;
    e[1] += u64::from(bytes > 0);
    e[2] += bytes;
}

/// What landing flights stored into title memory: `runs` (offsets, bytes)
/// from `at` on, the dwords that really changed.
pub(crate) fn wrote(at: usize, runs: &[(usize, usize)]) {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if open.is_empty() {
        return;
    }
    for &(off, n) in runs {
        changed_under(&mut open, at + off, at + off + n);
    }
}

#[derive(Default, Clone, Copy)]
struct Waits {
    count: u64,
    time: Duration,
}

/// By what was read and why the steps on the card did not take it: the
/// waits, and the range of the last of them.
type ByCause = BTreeMap<(Reads, &'static str), (Waits, usize, usize)>;
static WAITS: Mutex<ByCause> = Mutex::new(BTreeMap::new());

/// A wait for flights is not needed after all (they had landed).
pub(crate) fn not_waited() {
    if !on() {
        return;
    }
    MET.take();
    if let Some(token) = TOKEN.take() {
        close(&mut OPEN.lock().unwrap_or_else(|e| e.into_inner()), token);
    }
}

/// A wait for flights (settle reason `why`) took `time`; the read noted by
/// [`met`] on this thread, if any, is what it was for.
pub(crate) fn waited(why: usize, time: Duration) {
    if !on() {
        return;
    }
    let blocker = MET.take();
    if let Some(token) = TOKEN.take() {
        let closed = close(&mut OPEN.lock().unwrap_or_else(|e| e.into_inner()), token);
        if let Some((reads, bytes)) = closed {
            count_changed(&mut CHANGED.lock().unwrap_or_else(|e| e.into_inner()), reads, bytes);
        }
    }
    let (reads, refused, from, len) = match blocker {
        Some(b) => (b.reads, REFUSED.get(), b.from, b.len),
        None => (Reads::Settle { why: why.min(255) as u8 }, "", 0, 0),
    };
    let mut waits = WAITS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = waits.entry((reads, refused)).or_default();
    entry.0.count += 1;
    entry.0.time += time;
    (entry.1, entry.2) = (from, len);
}

/// The `WHY_` places of `crate::host`, by number.
fn settle_name(why: u8) -> &'static str {
    match why {
        2 => "a synchronous submission, a present or the sweep",
        3 => "the card had run them",
        4 => "a memory value",
        5 => "title memory changed under shader writes",
        6 => "title memory read",
        7 => "a step of ours",
        8 => "lists in the air",
        9 => "bottom-of-pipe writes",
        10 => "a blocking fence wait",
        _ => "another reason",
    }
}

fn reads_name(reads: Reads) -> String {
    match reads {
        Reads::UploadFill { from } => format!("UploadFill ({})", FILL_NAMES[usize::from(from).min(FILL_KINDS - 1)]),
        Reads::Settle { why } => format!("Settle ({})", settle_name(why)),
        other => format!("{other:?}"),
    }
}

// Who made a fill of upload memory (`crate::host::Step::Fill`).
/// A root view not on 256 bytes (`draw.rs` `root_view`).
pub(crate) const FILL_ROOT_VIEW: u8 = 0;
/// A copy into a texture from a buffer (`list.rs`).
pub(crate) const FILL_TEXTURE_COPY: u8 = 1;
/// A constant view of a descriptor table (`draw.rs`).
pub(crate) const FILL_TABLE_CONSTANTS: u8 = 2;
pub(crate) const FILL_KINDS: usize = 3;
pub(crate) const FILL_NAMES: [&str; FILL_KINDS] = ["root view off 256", "texture copy", "table constants"];

/// By kind: made since the last report ([0]), and waited for at the send as
/// the first thing met that flights write ([1]).
static FILLS: [[AtomicU64; FILL_KINDS]; 2] = [const { [const { AtomicU64::new(0) }; FILL_KINDS] }; 2];

/// A fill of kind `from` was made.
pub(crate) fn fill_made(from: u8) {
    if on() {
        FILLS[0][usize::from(from).min(FILL_KINDS - 1)].fetch_add(1, Ordering::Relaxed);
    }
}

/// A fill of kind `from` is what the submission waits for.
pub(crate) fn fill_waited(from: u8) {
    if on() {
        FILLS[1][usize::from(from).min(FILL_KINDS - 1)].fetch_add(1, Ordering::Relaxed);
    }
}

/// Who asks `VirtualQuery` in `crate::watch`.
#[derive(Clone, Copy)]
pub(crate) enum Query {
    /// Is the range view memory? The answer was yes.
    ViewYes = 0,
    /// The same, answered no.
    ViewNo = 1,
    /// How far is memory noted absent still not committed?
    Absent = 2,
}

/// For [`Query`]: calls of the function, `VirtualQuery` calls in them, ns,
/// and the bytes the answers' regions spanned (what the query walks).
static QUERIES: [[AtomicU64; 4]; 3] = [const { [const { AtomicU64::new(0) }; 4] }; 3];

pub(crate) fn queried(who: Query, calls: u64, bytes: u64, started: Instant) {
    let q = &QUERIES[who as usize];
    for (counter, add) in q.iter().zip([1, calls, started.elapsed().as_nanos() as u64, bytes]) {
        counter.fetch_add(add, Ordering::Relaxed);
    }
}

/// Says the counts, if five seconds have passed. Called once a submission.
pub(crate) fn say() {
    static LAST: Mutex<Option<(Instant, u64)>> = Mutex::new(None);
    if !on() {
        return;
    }
    let Ok(mut last) = LAST.try_lock() else {
        return;
    };
    let frame = crate::frame::current();
    let &mut (since, from) = last.get_or_insert_with(|| (Instant::now(), frame));
    let frames = frame.saturating_sub(from);
    if frames == 0 || since.elapsed().as_secs() < 5 {
        return;
    }
    *last = Some((Instant::now(), frame));
    let per = |v: f64| v / frames as f64;

    let waits = std::mem::take(&mut *WAITS.lock().unwrap_or_else(|e| e.into_inner()));
    let mut kinds: BTreeMap<Reads, Waits> = BTreeMap::new();
    for ((reads, _), (w, ..)) in &waits {
        let k = kinds.entry(*reads).or_default();
        k.count += w.count;
        k.time += w.time;
    }
    let all: Duration = kinds.values().map(|w| w.time).sum();
    tracing::info!(
        "cpu stats, waits for flights over {frames} frames: {:.1} ms a frame in all",
        per(all.as_secs_f64() * 1e3)
    );
    for (reads, w) in &kinds {
        tracing::info!(
            "cpu stats, waits for flights over {frames} frames: {}: {:.2} a frame, {:.2} ms a frame",
            reads_name(*reads),
            per(w.count as f64),
            per(w.time.as_secs_f64() * 1e3)
        );
    }
    let changed = std::mem::take(&mut *CHANGED.lock().unwrap_or_else(|e| e.into_inner()));
    for (reads, [n, with, bytes]) in &changed {
        tracing::info!(
            "cpu stats, waits followed to their end over {frames} frames: {}: {:.2} a frame, {:.2} of them with read bytes \
             shaders changed meanwhile, {:.1} KB of those bytes a frame",
            reads_name(*reads),
            per(*n as f64),
            per(*with as f64),
            per(*bytes as f64 / 1024.0)
        );
    }
    let mut longest: Vec<_> = waits.into_iter().filter(|((r, _), _)| !matches!(r, Reads::Settle { .. })).collect();
    longest.sort_by_key(|(_, (w, ..))| std::cmp::Reverse(w.time));
    for ((reads, refused), (w, at, len)) in longest.into_iter().take(8) {
        tracing::info!(
            "cpu stats, longest: {} (last range {at:#x}+{len:#x}): {:.2} a frame, {:.2} ms a frame; not done on the card: {refused}",
            reads_name(reads),
            per(w.count as f64),
            per(w.time.as_secs_f64() * 1e3)
        );
    }
    let [made, waited] = FILLS.each_ref().map(|k| k.each_ref().map(|n| n.swap(0, Ordering::Relaxed)));
    let said = |counts: &[u64; FILL_KINDS]| -> String {
        let s: Vec<String> = counts
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(i, &n)| format!("{} {:.2}", FILL_NAMES[i], per(n as f64)))
            .collect();
        if s.is_empty() { "none".into() } else { s.join(", ") }
    };
    tracing::info!(
        "cpu stats, fills of upload memory, made (a frame): {}; waited for at the send, first thing met (a frame): {}",
        said(&made),
        said(&waited)
    );
    for (name, q) in ["is it a view: yes", "is it a view: no", "how far is it absent"].iter().zip(&QUERIES) {
        let [asked, calls, ns, bytes] = [0, 1, 2, 3].map(|i| q[i].swap(0, Ordering::Relaxed) as f64);
        tracing::info!(
            "cpu stats, VirtualQuery, {name}: {:.1} askings a frame, {:.1} calls, {:.2} ms a frame, regions of {:.0} MB in all a frame",
            per(asked),
            per(calls),
            per(ns / 1e6),
            per(bytes / (1 << 20) as f64)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Reads, changed_under, close, count_changed, open_wait};

    /// A merge counts against the open waits it meets, by the bytes they
    /// share; a closed wait is counted once, as changed only if bytes were.
    #[test]
    fn merges_count_against_the_waits_they_meet() {
        let fill = Reads::UploadFill { from: 2 };
        let copy = Reads::CopySource;
        let mut open = Vec::new();
        open_wait(&mut open, 1, fill, 0x1000, 480);
        open_wait(&mut open, 2, copy, 0x8000, 0x100);
        changed_under(&mut open, 0x0f00, 0x1010); // 16 bytes of the fill
        changed_under(&mut open, 0x11e0, 0x1300); // just past the fill
        changed_under(&mut open, 0x2000, 0x3000); // neither
        assert_eq!(close(&mut open, 1), Some((fill, 16)));
        assert_eq!(close(&mut open, 1), None, "closed once");
        assert_eq!(close(&mut open, 2), Some((copy, 0)));
        let mut all = std::collections::BTreeMap::new();
        count_changed(&mut all, fill, 16);
        count_changed(&mut all, fill, 0);
        count_changed(&mut all, copy, 0);
        assert_eq!(all[&fill], [2, 1, 16]);
        assert_eq!(all[&copy], [1, 0, 0]);
    }
}

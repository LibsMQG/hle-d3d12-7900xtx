//! `VICTORY_CHANGED_BYTES=1` (diagnostics builds; off by default): of the
//! title memory copied into host copies, how much differs from what the copy
//! already held. Counted per reservation of the title's (with the protection
//! it asked for), apart for copies whose shader side is in the card's memory.
//! Also what would be taken at finer units than a copy's: the bytes of the
//! 4 KiB and 256-byte blocks (of title addresses) that hold a difference.
//!
//! Measurement only: the copy itself is made as before. It reads both sides
//! once more, so frames are slower with it on. The title may write between
//! this look and the copy, and a copy's PC side may lag what shaders wrote on
//! the card until that is merged: "unchanged" here does not by itself make a
//! copy safe to leave out.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_CHANGED_BYTES").is_some_and(|v| v == "1"))
}

/// What the title asked for a reservation: its bytes and protection.
static RESERVED: Mutex<BTreeMap<usize, (usize, u32)>> = Mutex::new(BTreeMap::new());

pub(crate) fn reserved(at: usize, size: usize, protect: u32) {
    RESERVED.lock().unwrap_or_else(|e| e.into_inner()).insert(at, (size, protect));
}

/// The reservation `[start, start + size)` that holds `address`, as
/// `(start, protect)`, among `reserved`.
fn holding(reserved: &BTreeMap<usize, (usize, u32)>, address: usize) -> Option<(usize, u32)> {
    let (&start, &(size, protect)) = reserved.range(..=address).next_back()?;
    (address - start < size).then_some((start, protect))
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct Difference {
    /// Bytes that differ.
    bytes: u64,
    /// Bytes (inside the range) of the 256-byte blocks holding one.
    small: u64,
    /// Bytes (inside the range) of the 4 KiB blocks holding one.
    page: u64,
}

const SMALL: usize = 256;
const KERNEL_PAGE: usize = 4096;

/// How `new` differs from `old`, both the bytes of title memory from `at` on:
/// blocks are cut at multiples of their size in title addresses.
fn difference(old: &[u8], new: &[u8], at: usize) -> Difference {
    let len = old.len().min(new.len());
    let mut out = Difference::default();
    let mut from = 0;
    while from < len {
        let page_end = (from + KERNEL_PAGE - (at + from) % KERNEL_PAGE).min(len);
        if old[from..page_end] != new[from..page_end] {
            out.page += (page_end - from) as u64;
            let mut small = from;
            while small < page_end {
                let end = (small + SMALL - (at + small) % SMALL).min(page_end);
                if old[small..end] != new[small..end] {
                    out.small += (end - small) as u64;
                    out.bytes += old[small..end].iter().zip(&new[small..end]).filter(|(a, b)| a != b).count() as u64;
                }
                small = end;
            }
        }
        from = page_end;
    }
    out
}

#[derive(Default, Clone, Copy)]
struct Totals {
    protect: u32,
    copies: u64,
    copied: u64,
    difference: Difference,
    unread: u64,
}

struct Stats {
    /// By (reservation start, on the card).
    by: BTreeMap<(usize, bool), Totals>,
    since: Instant,
    frame: u64,
    looking_ns: u64,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

/// Before title memory `[from, from + len)` is copied to `copy`, which holds
/// `len` mapped bytes.
pub(crate) fn before_copy(copy: *const u8, from: usize, len: usize, on_card: bool) {
    const CHUNK: usize = 64 << 10;
    thread_local! {
        static TITLE: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let started = Instant::now();
    let (mut total, mut unread) = (Difference::default(), 0u64);
    TITLE.with_borrow_mut(|title| {
        title.resize(CHUNK, 0);
        let mut done = 0;
        while done < len {
            // Chunks end on 64 KiB lines of title addresses, so no block is
            // counted twice.
            let n = (CHUNK - (from + done) % CHUNK).min(len - done);
            if crate::host::read_into((from + done) as *const u8, &mut title[..n]) {
                // SAFETY: the caller's mapping holds `len` bytes.
                let held = unsafe { std::slice::from_raw_parts(copy.add(done), n) };
                let d = difference(held, &title[..n], from + done);
                total.bytes += d.bytes;
                total.small += d.small;
                total.page += d.page;
            } else {
                unread += n as u64;
            }
            done += n;
        }
    });
    let (start, protect) = holding(&RESERVED.lock().unwrap_or_else(|e| e.into_inner()), from).unwrap_or((0, 0));
    let frame = crate::frame::current();
    let mut stats = STATS.lock().unwrap_or_else(|e| e.into_inner());
    let stats = stats.get_or_insert_with(|| Stats { by: BTreeMap::new(), since: Instant::now(), frame, looking_ns: 0 });
    let t = stats.by.entry((start, on_card)).or_default();
    t.protect = protect;
    t.copies += 1;
    t.copied += len as u64;
    t.difference.bytes += total.bytes;
    t.difference.small += total.small;
    t.difference.page += total.page;
    t.unread += unread;
    stats.looking_ns += started.elapsed().as_nanos() as u64;
    let frames = frame.saturating_sub(stats.frame);
    if frames > 0 && stats.since.elapsed().as_secs() >= 5 {
        say(stats, frames);
        *stats = Stats { by: BTreeMap::new(), since: Instant::now(), frame, looking_ns: 0 };
    }
}

fn say(stats: &Stats, frames: u64) {
    let mb = |v: u64| v as f64 / (1 << 20) as f64 / frames as f64;
    tracing::info!(
        "changed bytes over {frames} frames; looking took {:.1} ms a frame (all threads)",
        stats.looking_ns as f64 / 1e6 / frames as f64
    );
    for (&(start, on_card), t) in &stats.by {
        tracing::info!(
            "changed bytes: reservation {start:#x} protect {:#x} {}: per frame {:.1} copies, copied {:.2} MB, differ {:.3} MB, in 256 B blocks {:.3} MB, in 4 KiB blocks {:.3} MB, not read {:.2} MB",
            t.protect,
            if on_card { "to-card" } else { "pc-only" },
            t.copies as f64 / frames as f64,
            mb(t.copied),
            mb(t.difference.bytes),
            mb(t.difference.small),
            mb(t.difference.page),
            mb(t.unread),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Difference, difference, holding};

    #[test]
    fn equal_bytes_differ_nowhere() {
        let a = vec![7u8; 10_000];
        assert_eq!(difference(&a, &a, 0x1_0010), Difference::default());
    }

    #[test]
    fn a_byte_costs_its_blocks() {
        let old = vec![0u8; 12_288];
        let mut new = old.clone();
        new[5_000] = 1;
        assert_eq!(difference(&old, &new, 0), Difference { bytes: 1, small: 256, page: 4096 });
    }

    #[test]
    fn blocks_follow_title_addresses_and_are_cut_to_the_range() {
        // From 100 bytes before a 4 KiB line: a change on each side of it.
        let old = vec![0u8; 1_000];
        let mut new = old.clone();
        (new[99], new[100]) = (1, 1);
        assert_eq!(difference(&old, &new, 0x1000 - 100), Difference { bytes: 2, small: 100 + 256, page: 1_000 });
    }

    #[test]
    fn an_address_is_held_by_the_reservation_around_it() {
        let reserved = [(0x1000, (0x1000, 4)), (0x4000, (0x100, 0x2404))].into_iter().collect();
        assert_eq!(holding(&reserved, 0x1fff), Some((0x1000, 4)));
        assert_eq!(holding(&reserved, 0x2000), None);
        assert_eq!(holding(&reserved, 0x4000), Some((0x4000, 0x2404)));
        assert_eq!(holding(&reserved, 0xfff), None);
    }
}

//! The title's frame rate: for the whole game (`VICTORY_TITLE_FPS=N`, off
//! without it), or only the shortest step of its simulation (with
//! `VICTORY_FPS`, our pacing of light frames). Ported from the victory-amd
//! fork (its `frame_rate`, `VICTORY_TITLE_FPS` there too).
//!
//! What the title does (build 1.0.2.1, image offsets):
//!
//! - Its graphics settings hold a frame rate (+0x210 of the settings
//!   object, 30 here). Two functions that apply the settings, +0x1e0fb30 and
//!   +0x22434a0, make a frame time of it: `frame_us = 1e6 / rate`, 0 and
//!   1000 or more being 1000 (the code's "no limit"), kept at +0x98f84ac.
//! - +0x247ab10 sets the frame interval from it: the display's rate is 60,
//!   or 120 if the output lists a 120 Hz mode (+0x2481ca0); the divisor is
//!   `min(display / rate, 4)`, or 1 for a rate above the display's; the
//!   interval is a table at +0x61ece60 (`[16667, 16667, 33333, 50000,
//!   66667]` at 60 Hz, `[8333, 8333, 16667, 25000, 33333]` at 120 Hz),
//!   handed to the device's frame interval method. At 30 and 60 Hz:
//!   33,333 us.
//! - +0x120530 is a limiter of its own (`victory_hle`'s `fpslimit`): it
//!   waits until `1000 / trunc(1e6 / frame_us)` ms have passed since the
//!   last frame.
//! - +0x3eaf5d0 hands `1e6 / frame_us` to scripts, and +0x234490 scales
//!   per-frame amounts by it (1.0 below 60 except 0.7 at 40, 0.5 from 60).
//! - The simulation: +0x22264d0 hands each of its timers the real time since
//!   the last frame; +0x2226040 makes the step of it, at most 1/15 s and at
//!   least `1 / rate` (+0x6849844, made at +0x22435de and +0x2241190 from
//!   +0x2241fd0, which reads the same setting; read here from the running
//!   title: 0.0333). So at 30 a frame shorter than 33.3 ms still moves game
//!   time 33.3 ms: at 60 a second the game ran at twice its speed (the
//!   fork's run, in the menus).
//!
//! `VICTORY_TITLE_FPS=N` (0: no limit): the two loads of the setting become
//! `mov eax, N ; nop`, and +0x2241fd0 returns N, so the title's frame time,
//! interval, own limiter, scripts' rate and shortest step all follow N: the
//! whole game, the world too; the setting itself is left as it is.
//! `VICTORY_FPS=N` alone takes the shortest step alone: light frames shown
//! up to N a second keep game time to the wall clock, and the world's
//! frames, held to 33.3 ms or more by the title's limiter and interval, step
//! as before. Each site's bytes, from the load to the store of what is made
//! of it, are compared before anything is written; a site whose bytes
//! differ is left alone and said so (another build of the title).

/// `VICTORY_TITLE_FPS`: the rate the whole title is given (0: its "no
/// limit"). `None` if unset or malformed (said why).
pub fn title_fps_asked() -> Option<u32> {
    let spec = std::env::var("VICTORY_TITLE_FPS").ok()?;
    let rate = parse_rate(&spec);
    if rate.is_none() {
        tracing::warn!(
            spec,
            "VICTORY_TITLE_FPS: expected a rate from 0 (no limit) to 1000; the title's own kept"
        );
    }
    rate
}

/// `" 60 "` -> 60; at most 1000 (the title's own "no limit").
pub fn parse_rate(spec: &str) -> Option<u32> {
    spec.trim().parse::<u32>().ok().filter(|r| *r <= 1000)
}

/// The rate the shortest step is made of: the whole title's when given,
/// else our light frames' (`VICTORY_FPS`), the faster of the two when both
/// are (0 being no limit, 1000).
pub fn step_rate(title: Option<u32>, light: Option<u32>) -> Option<u32> {
    let as_rate = |r: u32| if r == 0 { 1000 } else { r };
    match (title, light) {
        (None, None) => None,
        (Some(t), None) => Some(t),
        (None, Some(l)) => Some(as_rate(l)),
        (Some(t), Some(l)) => Some(as_rate(t).max(as_rate(l))),
    }
}

/// The frame time the title makes of a rate, in microseconds, as its code
/// computes it (single-precision division, truncated).
pub fn title_frame_us(rate: u32) -> u32 {
    let clamped = if rate == 0 || rate >= 1000 {
        1000
    } else {
        rate
    };
    (1_000_000.0_f32 / clamped as f32) as u32
}

/// The interval the title then asks for, in microseconds, on a display of
/// 60 Hz (or 120 Hz).
pub fn title_interval_us(rate: u32, display_120: bool) -> u32 {
    const TABLE: [[u32; 5]; 2] = [
        [16667, 16667, 33333, 50000, 66667],
        [8333, 8333, 16667, 25000, 33333],
    ];
    let fps = (1_000_000.0_f32 / title_frame_us(rate) as f32) as u32 & 0xffff;
    let display = if display_120 { 120 } else { 60 };
    let index = if fps <= display {
        (display / fps.max(1)).min(4)
    } else {
        1
    };
    TABLE[usize::from(display_120)][index as usize]
}

/// A read of the setting to change: its offset in the image, what it is,
/// and the bytes there (compared first; the replacement goes at the start).
pub struct Site {
    pub at: usize,
    pub kind: Kind,
    pub bytes: &'static [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `mov eax, [reg+210h]` before the frame time is made of it: becomes
    /// `mov eax, rate ; nop` (6 bytes).
    Load,
    /// +0x2241fd0, the rate the shortest step is made of (`1 / rate`,
    /// +0x6849844): `cmp dword [rax+210h], 0` becomes `mov eax, rate ; jmp`
    /// to its `vcvtsi2ss` (7 bytes). For no limit, 1000.
    StepRate,
}

/// The three reads (build 1.0.2.1, image offsets).
pub const SITES: [Site; 3] = [
    // +0x1e0fb30: mov eax, [rdi+210h] ... mov [+0x98f84ac], eax
    Site {
        at: 0x1e0fc62,
        kind: Kind::Load,
        bytes: &[
            0x8b, 0x87, 0x10, 0x02, 0x00, 0x00, 0xba, 0xe8, 0x03, 0x00, 0x00, 0xc5, 0xfa, 0x10,
            0x0d, 0x9f, 0xa1, 0x34, 0x04, 0xb9, 0xe8, 0x03, 0x00, 0x00, 0x3d, 0xe8, 0x03, 0x00,
            0x00, 0x0f, 0x42, 0xd0, 0x85, 0xc0, 0x0f, 0x44, 0xd1, 0xc5, 0xea, 0x2a, 0xc2, 0xc5,
            0xf2, 0x5e, 0xc0, 0xc4, 0xe1, 0xfa, 0x2c, 0xc0, 0x89, 0x05, 0x12, 0x88, 0xae, 0x07,
        ],
    },
    // +0x22434a0: mov eax, [rsi+210h] ... mov [+0x98f84ac], eax
    Site {
        at: 0x224359a,
        kind: Kind::Load,
        bytes: &[
            0x8b, 0x86, 0x10, 0x02, 0x00, 0x00, 0xba, 0xe8, 0x03, 0x00, 0x00, 0xc5, 0xfa, 0x10,
            0x0d, 0x67, 0x68, 0xf1, 0x03, 0xb9, 0xe8, 0x03, 0x00, 0x00, 0x3d, 0xe8, 0x03, 0x00,
            0x00, 0x0f, 0x42, 0xd0, 0x85, 0xc0, 0x0f, 0x44, 0xd1, 0xb1, 0xff, 0xc5, 0xea, 0x2a,
            0xc2, 0xc5, 0xf2, 0x5e, 0xc0, 0xc4, 0xe1, 0xfa, 0x2c, 0xc0, 0x89, 0x05, 0xd8, 0x4e,
            0x6b, 0x07,
        ],
    },
    // +0x2241fd0: cmp dword [rax+210h], 0 ; je ; call ; mov eax, [rax+210h] ;
    // vcvtsi2ss xmm0, xmm0, rax
    Site {
        at: 0x2241fdb,
        kind: Kind::StepRate,
        bytes: &[
            0x83, 0xb8, 0x10, 0x02, 0x00, 0x00, 0x00, 0x74, 0x12, 0xe8, 0xc7, 0xa1, 0xe0, 0x02,
            0x8b, 0x80, 0x10, 0x02, 0x00, 0x00, 0xc4, 0xe1, 0xfa, 0x2a, 0xc0,
        ],
    },
];

/// The two loads of the setting (`VICTORY_TITLE_FPS`).
pub fn load_sites() -> &'static [Site] {
    &SITES[..2]
}

/// The shortest step's rate (`VICTORY_TITLE_FPS` or `VICTORY_FPS`).
pub fn step_sites() -> &'static [Site] {
    &SITES[2..]
}

/// Title data worth seeing while it runs (`VICTORY_TIMER_LOG=1`): the frame
/// time (+0x98f84ac) and the limiter's byte (+0x7453ec9).
pub const FRAME_US: usize = 0x98f84ac;
pub const LIMITER_ON: usize = 0x7453ec9;
/// The title's timers (each set up at +0x22261a9 with a step of 0.05 s and
/// 50 ms): +0 and +4 times, +0xc the step in seconds, +0x10 in milliseconds.
pub const TIMERS: [usize; 7] = [
    0xa943788, 0x856c6b4, 0xabbef40, 0xa958358, 0xa958380, 0x74fa558, 0xa943768,
];

/// What goes at a site of `kind` for `rate`.
pub fn replacement(kind: Kind, rate: u32) -> Vec<u8> {
    match kind {
        // mov eax, rate ; nop
        Kind::Load => {
            let r = rate.to_le_bytes();
            vec![0xb8, r[0], r[1], r[2], r[3], 0x90]
        }
        // mov eax, rate ; jmp +13 (to the vcvtsi2ss, 20 bytes on)
        Kind::StepRate => {
            let r = (if rate == 0 { 1000 } else { rate }).to_le_bytes();
            vec![0xb8, r[0], r[1], r[2], r[3], 0xeb, 0x0d]
        }
    }
}

/// What became of a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Patched {
    Done,
    /// Its bytes are not the ones expected: left alone.
    Differs,
    Failed,
}

/// Makes each site of `sites` in the image of `size` bytes at `base` take
/// `rate`. What became of each, said in the log too.
///
/// # Safety
/// The image stays mapped and no thread runs its code yet.
#[cfg(windows)]
pub unsafe fn apply(base: usize, size: usize, rate: u32, sites: &[Site]) -> Vec<Patched> {
    sites
        .iter()
        .map(|site| {
            let code = replacement(site.kind, rate);
            let inside = site.bytes.len() >= code.len()
                && site.at.checked_add(site.bytes.len()).is_some_and(|end| end <= size);
            let done = if !inside {
                Patched::Failed
            } else {
                let at = base + site.at;
                // SAFETY: inside the mapped image (checked above).
                let there = unsafe { std::slice::from_raw_parts(at as *const u8, site.bytes.len()) };
                if there != site.bytes {
                    Patched::Differs
                } else {
                    // SAFETY: the caller's promise; the bytes are compared again.
                    match unsafe { crate::callhook::patch_bytes(at, site.bytes, &code) } {
                        Ok(()) => Patched::Done,
                        Err(e) => {
                            tracing::warn!(error = %e, "frame rate: a write to the title's code failed");
                            Patched::Failed
                        }
                    }
                }
            };
            let offset = format_args!("{:#x}", site.at);
            match done {
                Patched::Done => tracing::info!(site = %offset, kind = ?site.kind, rate, "frame rate: the title's read of its frame rate setting patched"),
                Patched::Differs => tracing::warn!(site = %offset, "frame rate: not patched, the title's bytes there are not the ones expected (another build?)"),
                Patched::Failed => tracing::warn!(site = %offset, "frame rate: not patched"),
            }
            done
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_asked() {
        assert_eq!(parse_rate(" 60 "), Some(60));
        assert_eq!(parse_rate("0"), Some(0));
        assert_eq!(parse_rate("1000"), Some(1000));
        assert_eq!(parse_rate("1001"), None);
        assert_eq!(parse_rate("fast"), None);
        assert_eq!(step_rate(None, None), None);
        assert_eq!(step_rate(None, Some(60)), Some(60));
        assert_eq!(step_rate(None, Some(0)), Some(1000));
        assert_eq!(step_rate(Some(30), Some(60)), Some(60));
        assert_eq!(step_rate(Some(120), Some(60)), Some(120));
        assert_eq!(step_rate(Some(0), None), Some(0));
    }

    /// The title's own arithmetic: its 30 is the 33,333 us the log shows,
    /// on a 60 Hz display and on a 120 Hz one.
    #[test]
    fn the_titles_intervals() {
        assert_eq!(title_frame_us(30), 33333);
        assert_eq!(title_frame_us(60), 16666);
        assert_eq!(title_frame_us(0), 1000);
        assert_eq!(title_frame_us(5000), 1000);
        assert_eq!(title_interval_us(30, false), 33333);
        assert_eq!(title_interval_us(30, true), 33333);
        assert_eq!(title_interval_us(40, true), 25000);
        assert_eq!(title_interval_us(60, false), 16667);
        assert_eq!(title_interval_us(60, true), 16667);
        assert_eq!(title_interval_us(120, true), 8333);
        assert_eq!(title_interval_us(120, false), 16667);
        assert_eq!(title_interval_us(0, false), 16667);
        assert_eq!(title_interval_us(0, true), 8333);
        assert_eq!(title_interval_us(20, false), 50000);
    }

    #[test]
    fn the_groups_of_sites() {
        assert!(load_sites().iter().all(|s| s.kind == Kind::Load));
        assert!(step_sites().iter().all(|s| s.kind == Kind::StepRate));
        assert_eq!(load_sites().len() + step_sites().len(), SITES.len());
    }

    #[test]
    fn the_sites_are_reads_of_the_setting() {
        for site in SITES.iter().filter(|s| s.kind == Kind::Load) {
            // mov eax, [rdi/rsi+210h]
            assert_eq!(site.bytes[0], 0x8b);
            assert!(matches!(site.bytes[1], 0x86 | 0x87));
            assert_eq!(&site.bytes[2..6], &[0x10, 0x02, 0x00, 0x00]);
            // mov edx, 1000 follows; the window ends with the store of eax.
            assert_eq!(&site.bytes[6..11], &[0xba, 0xe8, 0x03, 0x00, 0x00]);
            assert_eq!(
                &site.bytes[site.bytes.len() - 6..site.bytes.len() - 4],
                &[0x89, 0x05]
            );
            // RIP-relative: the store is of +0x98f84ac.
            let disp = i32::from_le_bytes(site.bytes[site.bytes.len() - 4..].try_into().unwrap());
            let end = site.at + site.bytes.len();
            assert_eq!(end as i64 + i64::from(disp), FRAME_US as i64);
        }
        let step = SITES.iter().find(|s| s.kind == Kind::StepRate).unwrap();
        // cmp dword [rax+210h], 0 ; ... ; mov eax, [rax+210h] at 14 ;
        // vcvtsi2ss at 20
        assert_eq!(
            &step.bytes[..7],
            &[0x83, 0xb8, 0x10, 0x02, 0x00, 0x00, 0x00]
        );
        assert_eq!(&step.bytes[14..20], &[0x8b, 0x80, 0x10, 0x02, 0x00, 0x00]);
        assert_eq!(&step.bytes[20..], &[0xc4, 0xe1, 0xfa, 0x2a, 0xc0]);
        let code = replacement(Kind::StepRate, 60);
        assert_eq!(
            code.len() + usize::from(code[6]),
            20,
            "the jump lands on the vcvtsi2ss"
        );
        assert_eq!(replacement(Kind::Load, 60), [0xb8, 60, 0, 0, 0, 0x90]);
        assert_eq!(replacement(Kind::StepRate, 0)[1..5], 1000u32.to_le_bytes());
    }

    /// The sites patched in memory of our own, and the step's rate function
    /// run: it returns the rate given.
    #[cfg(windows)]
    #[test]
    fn sites_take_the_rate_and_others_are_left_alone() {
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc, VirtualFree,
        };
        // SAFETY: fresh memory of our own, written and run inside it.
        unsafe {
            let place = VirtualAlloc(
                std::ptr::null(),
                0x10000,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            ) as *mut u8;
            assert!(!place.is_null());
            let bytes = SITES[0].bytes;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), place.add(0x100), bytes.len());
            let mut other = SITES[1].bytes.to_vec();
            other[2] = 0x14; // [rsi+214h]
            std::ptr::copy_nonoverlapping(other.as_ptr(), place.add(0x200), other.len());
            // The step's rate as a function: rax = settings (+0x210 = 30),
            // the window, ret.
            let settings = [30u32; 0x100];
            let step = SITES[2].bytes;
            let mut f = vec![0x48, 0xb8];
            f.extend((settings.as_ptr() as u64).to_le_bytes());
            let window_at = f.len();
            f.extend(step);
            f.push(0xc3);
            std::ptr::copy_nonoverlapping(f.as_ptr(), place.add(0x300), f.len());
            let mine = [
                Site {
                    at: 0x100,
                    kind: Kind::Load,
                    bytes,
                },
                Site {
                    at: 0x200,
                    kind: Kind::Load,
                    bytes: SITES[1].bytes,
                },
                Site {
                    at: 0x300 + window_at,
                    kind: Kind::StepRate,
                    bytes: step,
                },
                Site {
                    at: 0xfff0,
                    kind: Kind::Load,
                    bytes,
                },
            ];
            let done = apply(place as usize, 0x10000, 60, &mine);
            assert_eq!(
                done,
                [
                    Patched::Done,
                    Patched::Differs,
                    Patched::Done,
                    Patched::Failed
                ]
            );
            let after = std::slice::from_raw_parts(place.add(0x100), bytes.len());
            assert_eq!(after[..6], replacement(Kind::Load, 60)[..]);
            assert_eq!(after[6..], bytes[6..]);
            assert_eq!(
                std::slice::from_raw_parts(place.add(0x200), other.len()),
                &other[..]
            );
            let rate =
                std::mem::transmute::<*mut u8, extern "system" fn() -> f32>(place.add(0x300));
            assert_eq!(rate(), 60.0);
            VirtualFree(place.cast(), 0, MEM_RELEASE);
        }
    }
}

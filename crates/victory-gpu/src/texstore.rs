//! A store of the larger levels of textures the title streams
//! (`VICTORY_TEXTURE_STORE=<folder>`; off).
//!
//! The title streams a texture's levels smallest first, and views it from
//! the first level it holds: a new texture is blurred until its larger
//! levels have been read (its streaming, and ours of its files) and shown,
//! seconds later, and again each time it is streamed out and back in.
//! Measured (run light-1, `crate::texlate`): a slot written while its
//! memory was not there waited 8.7 s on average to be shown.
//!
//! Here, once a texture has been uploaded whole (all levels, from level 0),
//! its levels as the host lays them out are kept, in memory and in the
//! folder, under keys made of its shape and of the bytes of its smaller
//! levels: for each level `k` from which on its levels fill at least
//! `MIN_PREFIX` bytes, the digest of the memory those levels take (the
//! title lays a texture's smaller levels out first, so that is the start of
//! its memory). When a texture of that shape is later first viewed from
//! level `k` with those very bytes there, in this run or a later one, its
//! host copy is made whole at once: levels from `k` on from title memory as
//! always, the larger ones from the store, and its views show them all.
//!
//! What it assumes: two textures of one shape whose smaller levels hold the
//! same bytes (at least `MIN_PREFIX` of them) have the same larger levels.
//! A texture shown wrong would be one of two such. Files are never trimmed.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

/// Least bytes of the smaller levels a key is made of.
pub(crate) const MIN_PREFIX: u64 = 32 << 10;
/// Bytes of levels kept in memory at most (the rest is read from the folder
/// when asked).
const KEPT: u64 = 4 << 30;

// T-0039 (original code, GPL-3.0-or-later): the kept levels are a cache of
// files the store always writes, so under host-memory pressure they can be
// let go and read back. `VICTORY_TEXSTORE_BOUND=0` keeps the old fixed bound.
const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// Bytes let go at most per [`TRIM_WINDOW_MS`], however often
/// [`Kept::trim`] is called: a sudden drop of the limit is spread over
/// several frames instead of one long stall (T-0044).
const TRIM_STEP: u64 = 64 * MIB;
const TRIM_WINDOW_MS: u64 = 16;

/// Bytes of levels kept in memory at most for a host with `total` bytes of
/// RAM whose smaller of free RAM and free commit is `headroom`: a quarter of
/// [`KEPT`] on PCs with 32 GiB or less, at most an eighth of the headroom,
/// and from 6 GiB of headroom down to 2 GiB a ramp from that eighth
/// (768 MiB) to nothing, with no step a frame would pay for at once.
pub(crate) fn kept_limit(bounded: bool, total: u64, headroom: u64) -> u64 {
    if !bounded {
        return KEPT;
    }
    let base = if total <= 32 * GIB { KEPT / 4 } else { KEPT };
    if headroom < 2 * GIB {
        0
    } else if headroom < 6 * GIB {
        base.min((headroom - 2 * GIB) / 16 * 3)
    } else {
        base.min(headroom / 8)
    }
}

fn bounded() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_TEXSTORE_BOUND").is_none_or(|v| v != "0"))
}

/// [`kept_limit`] for this host, sampled at most every 250 ms.
fn live_limit() -> u64 {
    static LIMIT: AtomicU64 = AtomicU64::new(KEPT);
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if !bounded() {
        return KEPT;
    }
    let now = crate::host::coarse_ms();
    let next = NEXT.load(Relaxed);
    if now >= next && NEXT.compare_exchange(next, now + 250, Relaxed, Relaxed).is_ok() {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: the API fills a correctly sized structure.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            let headroom = status.ullAvailPhys.min(status.ullAvailPageFile);
            LIMIT.store(kept_limit(true, status.ullTotalPhys, headroom), Relaxed);
        }
    }
    LIMIT.load(Relaxed)
}

/// Levels kept in memory by blob, least recently used let go first.
#[derive(Default)]
struct Kept {
    map: HashMap<u128, (Arc<Vec<u8>>, u64)>,
    bytes: u64,
    tick: u64,
    /// Blobs let go to stay within the limit, since the start.
    evicted: u64,
    /// The current [`TRIM_WINDOW_MS`] window and the bytes let go in it.
    window: u64,
    freed: u64,
}

/// Levels let go by [`Kept::trim`]: dropped by the caller once the store's
/// lock is released, so freeing them never holds up other lookups.
type Gone = Vec<Arc<Vec<u8>>>;

impl Kept {
    fn get(&mut self, blob: u128) -> Option<Arc<Vec<u8>>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(&blob).map(|(bytes, used)| {
            *used = tick;
            bytes.clone()
        })
    }

    /// Lets the least recently used go until at most `limit` bytes are kept,
    /// [`TRIM_STEP`] bytes at most per [`TRIM_WINDOW_MS`].
    fn trim(&mut self, limit: u64) -> Gone {
        self.trim_at(limit, crate::host::coarse_ms())
    }

    fn trim_at(&mut self, limit: u64, now_ms: u64) -> Gone {
        let mut gone = Gone::new();
        if self.bytes <= limit {
            return gone;
        }
        let window = now_ms / TRIM_WINDOW_MS;
        if window != self.window {
            (self.window, self.freed) = (window, 0);
        }
        while self.bytes > limit && self.freed < TRIM_STEP {
            let Some(len) = self.evict_oldest(&mut gone) else { break };
            self.freed += len;
        }
        gone
    }

    /// Lets the least recently used go; its size, or `None` if none is kept.
    fn evict_oldest(&mut self, gone: &mut Gone) -> Option<u64> {
        let (&blob, _) = self.map.iter().min_by_key(|(_, (_, used))| *used)?;
        let (bytes, _) = self.map.remove(&blob)?;
        let len = bytes.len() as u64;
        self.bytes -= len;
        self.evicted += 1;
        gone.push(bytes);
        Some(len)
    }

    /// Keeps `bytes` if room for them within `limit` is made by letting older
    /// ones go ([`trim`](Self::trim)); otherwise they are only on disk.
    fn insert(&mut self, blob: u128, bytes: &Arc<Vec<u8>>, limit: u64) -> Gone {
        let len = bytes.len() as u64;
        if self.map.contains_key(&blob) || len > limit {
            return self.trim(limit);
        }
        // What is over the limit goes at the trim's pace; room for the new
        // levels within it is made at once (one for one, no stall to spread).
        let mut gone = self.trim(limit);
        while self.bytes <= limit && self.bytes + len > limit {
            if self.evict_oldest(&mut gone).is_none() {
                break;
            }
        }
        if self.bytes + len <= limit {
            self.tick += 1;
            self.bytes += len;
            self.map.insert(blob, (bytes.clone(), self.tick));
        }
        gone
    }
}

/// A texture's shape: width, height, format, levels.
pub(crate) type Shape = [u32; 4];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Key {
    shape: Shape,
    level: u32,
    digest: u128,
}

/// Levels `0..levels` of a texture as the host lays them out, by the
/// digest of the whole texture they came from.
#[derive(Clone, Copy)]
struct Entry {
    blob: u128,
    levels: u32,
}

struct Store {
    folder: PathBuf,
    index: Mutex<HashMap<Key, Entry>>,
    kept: Mutex<Kept>,
    jobs: Mutex<mpsc::Sender<Job>>,
    /// Contents given to it this run (digests of whole textures).
    seen: Mutex<std::collections::HashSet<u128>>,
}

/// Since the start: entries found at startup, textures kept, keys asked,
/// found, blobs read from the folder, bytes written.
pub(crate) static STATS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

fn line_of(key: &Key, entry: &Entry) -> String {
    format!(
        "{} {} {} {} {} {:032x} {:032x} {}",
        key.shape[0], key.shape[1], key.shape[2], key.shape[3], key.level, key.digest, entry.blob, entry.levels
    )
}

fn parse_line(line: &str) -> Option<(Key, Entry)> {
    let mut p = line.split_whitespace();
    let mut n = || p.next();
    let shape = [n()?.parse().ok()?, n()?.parse().ok()?, n()?.parse().ok()?, n()?.parse().ok()?];
    let level = n()?.parse().ok()?;
    let digest = u128::from_str_radix(n()?, 16).ok()?;
    let blob = u128::from_str_radix(n()?, 16).ok()?;
    let levels = n()?.parse().ok()?;
    Some((Key { shape, level, digest }, Entry { blob, levels }))
}

fn store() -> Option<&'static Store> {
    static STORE: OnceLock<Option<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let folder = PathBuf::from(std::env::var_os("VICTORY_TEXTURE_STORE").filter(|v| !v.is_empty() && v != "0")?);
            std::fs::create_dir_all(&folder).ok()?;
            let mut index = HashMap::new();
            if let Ok(text) = std::fs::read_to_string(folder.join("index.txt")) {
                for (key, entry) in text.lines().filter_map(parse_line) {
                    // (A blob whose file is gone is no entry.)
                    index.insert(key, entry);
                }
            }
            STATS[0].store(index.len() as u64, Relaxed);
            tracing::info!(
                folder = %folder.display(),
                keys = index.len(),
                "texture store: larger levels kept from earlier runs (VICTORY_TEXTURE_STORE)"
            );
            let (send, receive) = mpsc::channel::<Job>();
            let to = folder.clone();
            std::thread::Builder::new()
                .name("texture store".into())
                .spawn(move || {
                    let Ok(mut index) =
                        std::fs::OpenOptions::new().create(true).append(true).open(to.join("index.txt"))
                    else {
                        return;
                    };
                    for job in receive {
                        // (The store is made before this thread's first job.)
                        let Some(store) = store() else {
                            continue;
                        };
                        let Some((blob, bytes, keys)) = kept(store, job) else {
                            continue;
                        };
                        let path = to.join(format!("{blob:032x}.lv"));
                        if !path.exists() {
                            let part = to.join(format!("{blob:032x}.part"));
                            if std::fs::write(&part, &bytes[..]).is_err() || std::fs::rename(&part, &path).is_err() {
                                continue;
                            }
                            STATS[5].fetch_add(bytes.len() as u64, Relaxed);
                        }
                        let text: String = keys.iter().map(|(k, e)| line_of(k, e) + "\n").collect();
                        let _ = index.write_all(text.as_bytes());
                        let _ = index.flush();
                    }
                })
                .ok()?;
            Some(Store {
                folder,
                index: Mutex::new(index),
                kept: Mutex::new(Kept::default()),
                jobs: Mutex::new(send),
                seen: Mutex::new(std::collections::HashSet::new()),
            })
        })
        .as_ref()
}

/// Whether the store is on.
pub(crate) fn on() -> bool {
    store().is_some()
}

/// The levels a key can be made from: those from which on the levels fill
/// at least [`MIN_PREFIX`] bytes and are the start of the memory, with
/// those bytes (`level`, bytes). Not level 0: that is the whole texture.
pub(crate) fn key_levels(layout: &victory_formats::texture::Layout, mips: u32) -> Vec<(u32, u64)> {
    if layout.slices != 1 {
        return Vec::new();
    }
    (1..mips)
        .filter(|&k| crate::resource::read_from(layout, k) == k)
        .map(|k| (k, layout.bytes_from(k)))
        .filter(|&(_, bytes)| bytes >= MIN_PREFIX && bytes < layout.size)
        .collect()
}

/// A texture of `shape` was read whole: `tiled` is its memory (all levels),
/// `parts` where the host lays out each of its levels (offset, row pitch,
/// rows). Detiled and kept on the store's thread, unless that content is
/// known.
pub(crate) fn keep(shape: Shape, layout: &victory_formats::texture::Layout, tiled: &[u8], parts: Vec<(u64, u32, u32)>) {
    let Some(store) = store() else {
        return;
    };
    let blob = crate::texture::digest(tiled).0;
    if !store.seen.lock().unwrap_or_else(|e| e.into_inner()).insert(blob) {
        return;
    }
    let job = Job { shape, layout: layout.clone(), tiled: tiled.to_vec(), parts, blob };
    let _ = store.jobs.lock().unwrap_or_else(|e| e.into_inner()).send(job);
}

/// A texture to keep ([`keep`]).
struct Job {
    shape: Shape,
    layout: victory_formats::texture::Layout,
    tiled: Vec<u8>,
    parts: Vec<(u64, u32, u32)>,
    blob: u128,
}

/// [`keep`]'s work, on the store's thread: the levels detiled as the host
/// lays them out, the keys, memory, and what goes to the folder.
fn kept(store: &Store, job: Job) -> Option<(u128, Arc<Vec<u8>>, Vec<(Key, Entry)>)> {
    let Job { shape, layout, tiled, parts, blob } = job;
    let keys = key_levels(&layout, shape[3]);
    let &(top, _) = keys.iter().max_by_key(|(k, _)| *k)?;
    let end = parts.get(top as usize)?.0 as usize;
    let made: Vec<Key> = keys
        .iter()
        .filter(|&&(_, bytes)| bytes as usize <= tiled.len())
        .map(|&(level, bytes)| Key { shape, level, digest: crate::texture::digest(&tiled[..bytes as usize]).0 })
        .collect();
    {
        let index = store.index.lock().unwrap_or_else(|e| e.into_inner());
        if made.iter().all(|k| index.contains_key(k)) {
            return None;
        }
    }
    // Levels `0..top`, the ones a key can stand for.
    let mut levels = vec![0u8; end];
    for (i, &(at, pitch, rows)) in parts.iter().take(top as usize).enumerate() {
        let (at, pitch, rows) = (at as usize, pitch as usize, rows as usize);
        let len = (pitch * rows).min(end.saturating_sub(at));
        layout.detile(&tiled, i as u32, 0, &mut levels[at..at + len], pitch, len).ok()?;
    }
    let bytes = Arc::new(levels);
    let entries: Vec<(Key, Entry)> = made.into_iter().map(|k| (k, Entry { blob, levels: k.level })).collect();
    {
        let mut index = store.index.lock().unwrap_or_else(|e| e.into_inner());
        for (k, e) in &entries {
            index.insert(*k, *e);
        }
    }
    let gone = store.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(blob, &bytes, live_limit());
    drop(gone);
    STATS[1].fetch_add(1, Relaxed);
    Some((blob, bytes, entries))
}

/// The larger levels of a texture of `shape` first viewed from `level`,
/// whose memory from that level on holds `smaller`: levels `0..level` in
/// the host's layout, if the store has them.
pub(crate) fn find(shape: Shape, level: u32, smaller: &[u8]) -> Option<Arc<Vec<u8>>> {
    let store = store()?;
    STATS[2].fetch_add(1, Relaxed);
    let key = Key { shape, level, digest: crate::texture::digest(smaller).0 };
    let entry = *store.index.lock().unwrap_or_else(|e| e.into_inner()).get(&key)?;
    if entry.levels != level {
        return None;
    }
    let (kept, gone) = {
        let mut kept = store.kept.lock().unwrap_or_else(|e| e.into_inner());
        let hit = kept.get(entry.blob);
        // (A hit also lets go what no longer fits when memory got short.)
        (hit, kept.trim(live_limit()))
    };
    drop(gone);
    let bytes = match kept {
        Some(bytes) => bytes,
        None => {
            let bytes = Arc::new(std::fs::read(store.folder.join(format!("{:032x}.lv", entry.blob))).ok()?);
            STATS[4].fetch_add(1, Relaxed);
            let gone = store.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(entry.blob, &bytes, live_limit());
            drop(gone);
            bytes
        }
    };
    STATS[3].fetch_add(1, Relaxed);
    Some(bytes)
}

/// The line for the memory lines.
pub(crate) fn line() -> Option<String> {
    let store = store()?;
    let [at_start, kept, asked, found, read, written] = [0, 1, 2, 3, 4, 5].map(|i| STATS[i].load(Relaxed));
    let (kept_bytes, evicted) = {
        let held = store.kept.lock().unwrap_or_else(|e| e.into_inner());
        (held.bytes, held.evicted)
    };
    Some(format!(
        "texture store (VICTORY_TEXTURE_STORE), since the start: {at_start} keys from earlier runs, {kept} textures kept \
         ({} MB written, {} MB held in memory, limit {} MB, {} let go), {asked} first views from a smaller level asked, \
         {found} found ({read} read from the folder), {} host copies made whole from it",
        written >> 20,
        kept_bytes >> 20,
        live_limit() >> 20,
        evicted,
        PREFILLED.load(Relaxed)
    ))
}

/// Host copies whose larger levels a list copied from the store.
pub(crate) static PREFILLED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_lines_read_back() {
        let key = Key { shape: [1024, 512, 71, 11], level: 3, digest: 0x0123_4567_89ab_cdef_0011_2233_4455_6677 };
        let entry = Entry { blob: 0xffee_ddcc_bbaa_9988_7766_5544_3322_1100, levels: 3 };
        let (k, e) = parse_line(&line_of(&key, &entry)).expect("a line");
        assert_eq!(k, key);
        assert_eq!((e.blob, e.levels), (entry.blob, entry.levels));
        assert!(parse_line("1 2 3").is_none());
    }

    #[test]
    fn kept_limit_follows_host_memory() {
        // Off: the old fixed bound whatever the host.
        assert_eq!(kept_limit(false, 32 * GIB, GIB), KEPT);
        // A 64 GiB host with plenty free keeps the full bound, a 32 GiB one a quarter.
        assert_eq!(kept_limit(true, 64 * GIB, 40 * GIB), KEPT);
        assert_eq!(kept_limit(true, 32 * GIB, 20 * GIB), GIB);
        // At most an eighth of the headroom.
        assert_eq!(kept_limit(true, 64 * GIB, 8 * GIB), GIB);
        // Watch and high pressure.
        assert_eq!(kept_limit(true, 64 * GIB, 5 * GIB), 576 * MIB);
        assert_eq!(kept_limit(true, 32 * GIB, 2 * GIB - 1), 0);
        // No step between the ramp and an eighth of the headroom, nor at 2 GiB.
        assert_eq!(kept_limit(true, 64 * GIB, 6 * GIB - 16), kept_limit(true, 64 * GIB, 6 * GIB) - 3);
        assert_eq!(kept_limit(true, 64 * GIB, 2 * GIB), 0);
        // Falling headroom never raises the limit.
        let mut last = u64::MAX;
        for h in (0..=12 * 16).rev().map(|n| n * GIB / 16) {
            let now = kept_limit(true, 64 * GIB, h);
            assert!(now <= last, "{h}: {now} > {last}");
            last = now;
        }
    }

    #[test]
    fn kept_evicts_least_recent_first() {
        let mut kept = Kept::default();
        let blob = |n: usize| Arc::new(vec![0u8; n]);
        kept.insert(1, &blob(100), 300);
        kept.insert(2, &blob(100), 300);
        kept.insert(3, &blob(100), 300);
        assert!(kept.get(1).is_some()); // 2 is now the least recent
        kept.insert(4, &blob(100), 300);
        assert!(kept.get(2).is_none());
        assert!(kept.get(1).is_some() && kept.get(3).is_some() && kept.get(4).is_some());
        assert_eq!((kept.bytes, kept.evicted), (300, 1));
        // Too large alone: not kept, and nothing else is let go for it.
        kept.insert(5, &blob(400), 300);
        assert!(kept.get(5).is_none());
        assert_eq!(kept.bytes, 300);
        // Memory got short: trimming lets go down to the new limit.
        kept.trim(150);
        assert_eq!((kept.bytes, kept.map.len()), (100, 1));
        kept.trim(0);
        assert_eq!((kept.bytes, kept.map.len()), (0, 0));
    }

    #[test]
    fn kept_trims_a_large_drop_in_steps() {
        let mut kept = Kept::default();
        let mib = Arc::new(vec![0u8; MIB as usize]);
        for blob in 0..200 {
            kept.insert(blob, &mib, GIB);
        }
        assert_eq!(kept.bytes, 200 * MIB);
        // One step per window however many calls (T-0044: a lookup calls
        // trim, and a frame makes many lookups).
        let t = 1000 * TRIM_WINDOW_MS;
        let gone = kept.trim_at(0, t);
        assert_eq!((gone.len() as u64, kept.bytes), (TRIM_STEP / MIB, 200 * MIB - TRIM_STEP));
        for _ in 0..10 {
            assert!(kept.trim_at(0, t + TRIM_WINDOW_MS - 1).is_empty(), "same window");
        }
        assert_eq!(kept.bytes, 200 * MIB - TRIM_STEP);
        // Levels let go are handed back, not freed under the lock.
        assert!(Arc::strong_count(&mib) > 1 + kept.map.len());
        drop(gone);
        assert_eq!(Arc::strong_count(&mib), 1 + kept.map.len());
        // While above the limit a new texture is not kept.
        kept.insert(1000, &mib, 8 * MIB);
        assert!(kept.get(1000).is_none());
        for w in 1..=4 {
            kept.trim_at(8 * MIB, t + w * TRIM_WINDOW_MS);
        }
        assert_eq!(kept.bytes, 8 * MIB);
    }

    fn process_memory() -> (u64, u64, u64) {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: a correctly sized structure for the current process.
        let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut c).cast(), c.cb) };
        (c.PrivateUsage as u64, c.PeakPagefileUsage as u64, c.PeakWorkingSetSize as u64)
    }

    fn process_cpu_ns() -> u64 {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let mut t: [FILETIME; 4] = unsafe { std::mem::zeroed() };
        let [a, b, c, d] = &mut t;
        // SAFETY: four valid FILETIME outputs for the current process.
        unsafe { GetProcessTimes(GetCurrentProcess(), a, b, c, d) };
        let ft = |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32 | u64::from(f.dwLowDateTime)) * 100;
        ft(&t[2]) + ft(&t[3])
    }

    /// Synthetic texture-streaming stress (T-0039): 768 frames each keep a
    /// new 4 MiB texture's levels and look up four earlier ones, on a
    /// simulated 32 GiB host whose headroom falls from 10 GiB as the title's
    /// own memory grows (8 MiB a frame) and as the store keeps levels. A miss
    /// is made again as the store would after reading the folder (no disk
    /// I/O here). The bound is `VICTORY_TEXSTORE_BOUND` (default on). Prints
    /// one `texstore-stress {json}` line; run by `resource_pressure.py measure`.
    #[test]
    #[ignore = "synthetic stress workload (resource_pressure.py measure)"]
    fn texstore_pressure_synthetic_stream() {
        const FRAMES: usize = 768;
        const SIZE: usize = 4 << 20;
        let bound = bounded();
        let make = |frame: usize| {
            let mut v = vec![0u8; SIZE];
            v.iter_mut().step_by(4096).for_each(|b| *b = frame as u8 | 1);
            Arc::new(v)
        };
        let (start_private, _, _) = process_memory();
        let start_cpu = process_cpu_ns();
        let mut kept = Kept::default();
        let (mut peak_private, mut peak_kept, mut hits, mut misses) = (0u64, 0u64, 0u64, 0u64);
        let mut frame_ns = Vec::with_capacity(FRAMES);
        for frame in 0..FRAMES {
            let began = std::time::Instant::now();
            let headroom = (10 * GIB).saturating_sub(frame as u64 * 8 * MIB).saturating_sub(kept.bytes);
            let limit = kept_limit(bound, 32 * GIB, headroom);
            kept.insert(frame as u128, &make(frame), limit);
            for back in [1usize, 2, 8, 200] {
                let Some(earlier) = frame.checked_sub(back) else { continue };
                let hit = kept.get(earlier as u128);
                kept.trim(limit);
                match hit {
                    Some(bytes) => {
                        hits += 1;
                        std::hint::black_box(bytes[0]);
                    }
                    None => {
                        misses += 1;
                        kept.insert(earlier as u128, &make(earlier), limit);
                    }
                }
            }
            frame_ns.push(began.elapsed().as_nanos() as u64);
            peak_kept = peak_kept.max(kept.bytes);
            peak_private = peak_private.max(process_memory().0);
        }
        let cpu = process_cpu_ns() - start_cpu;
        let (_, peak_commit, peak_working_set) = process_memory();
        frame_ns.sort_unstable();
        let pct = |p: f64| frame_ns[((FRAMES as f64 - 1.0) * p) as usize] as f64 / 1e6;
        println!(
            "texstore-stress {{\"bound\": {bound}, \"frames\": {FRAMES}, \"peak_kept_mib\": {}, \
             \"peak_private_mib\": {}, \"private_growth_mib\": {}, \"peak_commit_mib\": {}, \
             \"peak_working_set_mib\": {}, \"cpu_ms_per_frame\": {:.4}, \"frame_ms_p50\": {:.4}, \
             \"frame_ms_p99\": {:.4}, \"frame_ms_max\": {:.4}, \"hits\": {hits}, \"misses\": {misses}, \
             \"evicted\": {}}}",
            peak_kept >> 20,
            peak_private >> 20,
            peak_private.saturating_sub(start_private) >> 20,
            peak_commit >> 20,
            peak_working_set >> 20,
            cpu as f64 / 1e6 / FRAMES as f64,
            pct(0.5),
            pct(0.99),
            pct(1.0),
            kept.evicted
        );
        assert!(!bound || peak_kept <= GIB, "the bound held");
    }
}

//! Everything specific to the title: its container formats, build
//! identities, and the hooks we place in its code (located by per-build
//! offsets or byte-pattern signatures). What may and may not be stored here
//! is defined in docs/asset-policy.md.

pub mod sgd5;

/// A specific build of the title executable we know how to run.
#[derive(Debug, Clone, Copy)]
pub struct KnownBuild {
    /// Our own label, e.g. "1.0.0 retail".
    pub label: &'static str,
    /// BLAKE3 of the original executable, hex.
    pub blake3: &'static str,
    /// Our bytes written over the title's code before it runs.
    pub patches: &'static [CodePatch],
}

/// Our own bytes over an instruction of a known build, located by its
/// per-build offset (allowed by docs/asset-policy.md). Only what we write is
/// stored here, never the title's bytes.
#[derive(Debug, Clone, Copy)]
pub struct CodePatch {
    /// Our name for what is patched.
    pub name: &'static str,
    /// Offset in the executable's image.
    pub rva: u32,
    /// What we write there.
    pub bytes: &'static [u8],
}

/// The title's stall watchdog. At the frame rates a host reaches now (well
/// under 1 fps in the world) it decides progress stopped and ends the process
/// itself: a work scope open 300 s or more, or 30 one-second wakes without a
/// heartbeat (Marvisak/VIctory#630). Off unless `VICTORY_KEEP_WATCHDOG=1`.
/// The two sites and the bytes are those of XCL's loader for the same
/// executable (anoyarou/xcl `8a62b6f`, after ivall/xcl#1298 and #505).
const WATCHDOG: &[CodePatch] = &[
    CodePatch {
        name: "watchdog thread returns at once",
        rva: 0x0221_5a60,
        bytes: &[0x31, 0xc0, 0xc3], // xor eax, eax; ret
    },
    CodePatch {
        name: "watchdog timeout report returns",
        rva: 0x023c_f620,
        bytes: &[0xc3], // ret
    },
];

/// Executables identified from a local package.
pub const KNOWN_BUILDS: &[KnownBuild] = &[KnownBuild {
    label: "Series 1.0.2.1",
    blake3: "b99f876b74d4a77f03af5f878223b27bd647b5c35a79d5decd9f716504239e93",
    patches: WATCHDOG,
}];

/// Looks up a build by the BLAKE3 hash of its executable.
pub fn identify(blake3_hex: &str) -> Option<&'static KnownBuild> {
    KNOWN_BUILDS
        .iter()
        .find(|b| b.blake3.eq_ignore_ascii_case(blake3_hex))
}

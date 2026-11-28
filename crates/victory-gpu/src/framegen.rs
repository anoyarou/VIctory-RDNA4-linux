//! AMD FidelityFX frame generation for frames the title upscales
//! (diagnostics, `VICTORY_FIDELITYFX_FRAMEGEN=1` beside
//! `VICTORY_FIDELITYFX_DIR`). The title asks for no frame generation; the
//! inputs are its upscale's: at each host upscale (`external::record`
//! through the HLE FidelityFX module) the depth and motion vectors and the
//! frame's constants are kept, and those of the last submitted list feed
//! the next present. The presenter's swap chain is then AMD's frame
//! generation proxy (`amd_fidelityfx_framegeneration_dx12.dll`), which
//! inserts the generated frames.
//!
//! Mode 1 uses the analytical provider. Experimental mode 4 supplies the
//! title's world-space camera from its velocity pass when it matches the
//! current upscale.
//!
//! The swap chain's buffer as it is before the presenter's panel is drawn
//! over it is the HUD-less colour (`Presentation::keep_hudless`), so the
//! panel is kept out of the generated frames instead of being blended
//! between two (its text flickered and smeared). The title's own UI is in
//! the frame before it reaches the presenter and is still interpolated.
//!
//! shortcut: an RGBA8 swap chain only.

// Ported to rx9070xt-wine: the presenter path only (HUD-less colour from the
// back buffer). The UI-layer hooks in `draw.rs` and the ML camera
// (`camera.rs`, mode 4) are not wired here yet.
#![allow(dead_code)]

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE,
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, ID3D12CommandQueue, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{DXGI_SWAP_CHAIN_DESC1, IDXGIFactory2, IDXGISwapChain3, IDXGISwapChain4};
use windows::core::Interface;

use crate::host::HostList;
use crate::resource::HostTexture;

pub(crate) mod camera;

const DLL: &str = "amd_fidelityfx_framegeneration_dx12.dll";
const OK: u32 = 0;

// FidelityFX SDK v2.4.0: ffx_api.h, ffx_framegeneration.h,
// ffx_api_dx12.h, and dx12/ffx_api_framegeneration_dx12.h.
const BACKEND_DX12: u64 = 2;
const GET_VERSIONS: u64 = 4;
const OVERRIDE_VERSION: u64 = 5;
const PROVIDER_VERSION: u64 = 6;
const CREATE: u64 = 0x20001;
const CONFIGURE: u64 = 0x20002;
const PREPARE: u64 = 0x2000c;
const VERSION: u64 = 0x2000e;
const SWAPCHAIN_CREATE: u64 = 0x30006;
const SWAPCHAIN_WAIT: u64 = 0x30007;
/// `FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_REGISTERUIRESOURCE_DX12`.
const SWAPCHAIN_REGISTER_UI: u64 = 0x30002;
/// `FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_USE_PREMUL_ALPHA` and
/// `..._ENABLE_INTERNAL_UI_DOUBLE_BUFFERING`.
const UI_PREMULTIPLIED: u32 = 1 << 0;
const UI_DOUBLE_BUFFERED: u32 = 1 << 1;
/// `FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ`.
const FFX_PIXEL_COMPUTE_READ: u32 = (1 << 3) | (1 << 2);
const SWAPCHAIN_VERSION: u64 = 0x3000b;
/// `FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_KEYVALUE_DX12`
/// (`FFX_API_MAKE_BACKEND_EFFECT_SUB_ID(DX12, FRAMEGENERATIONSWAPCHAIN, 0x08)`).
const SWAPCHAIN_KEY_VALUE: u64 = 0x30008;
/// `FFX_API_CONFIGURE_FG_SWAPCHAIN_KEY_FRAMEPACINGTUNING`.
const KEY_FRAME_PACING: u64 = 2;
const RGBA8_UNORM: u32 = 10;
/// `FFX_API_RESOURCE_TYPE_TEXTURE2D`.
const TEXTURE_2D: u32 = 2;
/// Display-resolution motion vectors, motion-vector jitter cancellation,
/// inverted depth, and infinite depth. The upscale HDR flag is separate.
const MOTION_DEPTH_FLAGS: u32 = 0b11110;
/// The FidelityFX resource state of compute reads, and the host's.
const FFX_COMPUTE_READ: u32 = 1 << 2;
const HOST_NON_PIXEL_SHADER_RESOURCE: u32 = 0x40;

#[repr(C)]
struct Header {
    kind: u64,
    next: *mut Header,
}

fn header(kind: u64) -> Header {
    Header { kind, next: null_mut() }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Resource {
    resource: *mut c_void,
    description: [u32; 8],
    state: u32,
}

#[repr(C)]
struct BackendDx12 {
    header: Header,
    device: *mut c_void,
}

#[repr(C)]
struct Version {
    header: Header,
    version: u32,
}

#[repr(C)]
struct Versions {
    header: Header,
    effect: u64,
    device: *mut c_void,
    count: *mut u64,
    ids: *mut u64,
    names: *mut *const std::ffi::c_char,
}

#[repr(C)]
struct ProviderOverride {
    header: Header,
    id: u64,
}

#[repr(C)]
struct SwapchainDesc {
    header: Header,
    output: *mut *mut c_void,
    hwnd: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen_desc: *const c_void,
    factory: *mut c_void,
    queue: *mut c_void,
}

#[repr(C)]
struct Create {
    header: Header,
    flags: u32,
    display: [u32; 2],
    max_render: [u32; 2],
    format: u32,
}

type Callback = unsafe extern "C" fn(*mut Generate, *mut c_void) -> u32;

#[repr(C)]
struct Configure {
    header: Header,
    swapchain: *mut c_void,
    present_callback: *const c_void,
    present_user: *mut c_void,
    generate_callback: Option<Callback>,
    generate_user: *mut c_void,
    enabled: bool,
    allow_async: bool,
    hudless: Resource,
    flags: u32,
    only_generated: bool,
    rect: [i32; 4],
    id: u64,
}

/// The proxy swap chain's UI resource: laid over every frame it shows,
/// generated or not, and never interpolated.
#[repr(C)]
struct RegisterUi {
    header: Header,
    ui: Resource,
    flags: u32,
}

/// `ffxConfigureDescFrameGenerationSwapChainKeyValueDX12`.
#[repr(C)]
struct KeyValue {
    header: Header,
    key: u64,
    value: u64,
    ptr: *mut c_void,
}

/// `FfxApiSwapchainFramePacingTuning` (SDK 2.4.0,
/// `ffx_framegeneration_api_types.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct PacingTuning {
    safety_margin_ms: f32,
    variance_factor: f32,
    allow_hybrid_spin: bool,
    hybrid_spin_time: u32,
    allow_wait_for_single_object_on_fence: bool,
}

impl PacingTuning {
    /// The SDK's defaults (its threads spin for fence values and for the
    /// pacing target), or with fence waits: the threads that wait for the
    /// title's frames to finish on the card wait on the fence's event
    /// instead of spinning (under Wine each turn of that spin is a system
    /// call: about one core of the CPU went to it, run perf-s70). The
    /// pacing itself still spins: the shown frames keep their timing.
    fn with_fence_waits(on: bool) -> Self {
        Self {
            safety_margin_ms: 0.1,
            variance_factor: 0.1,
            allow_hybrid_spin: false,
            hybrid_spin_time: 2,
            allow_wait_for_single_object_on_fence: on,
        }
    }
}

/// Whether the proxy's threads wait on fence events instead of spinning
/// (`VICTORY_FG_FENCE_WAIT=1`; off by default until measured; A/B-able).
fn fence_waits() -> bool {
    if let Some(on) = crate::ab::pick("VICTORY_FG_FENCE_WAIT") {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_FG_FENCE_WAIT").is_some_and(|v| v == "1"))
}

#[repr(C)]
struct Prepare {
    header: Header,
    id: u64,
    flags: u32,
    list: *mut c_void,
    render: [u32; 2],
    jitter: [f32; 2],
    scale: [f32; 2],
    time_ms: f32,
    reset: bool,
    camera: [f32; 4],
    depth: Resource,
    motion: Resource,
    camera_position: [f32; 3],
    camera_up: [f32; 3],
    camera_right: [f32; 3],
    camera_forward: [f32; 3],
}

#[repr(C)]
struct Generate {
    header: Header,
    list: *mut c_void,
    color: Resource,
    output: [Resource; 4],
    count: u32,
}

#[repr(C)]
struct ProviderVersion {
    header: Header,
    id: u64,
    name: *const std::ffi::c_char,
}

const _: () = {
    use std::mem::offset_of;

    // These x64 offsets catch reordered fields even when the size is unchanged.
    assert!(size_of::<Resource>() == 48);
    assert!(offset_of!(Resource, description) == 8);
    assert!(offset_of!(Resource, state) == 40);
    assert!(size_of::<Versions>() == 56);
    assert!(offset_of!(Versions, count) == 32);
    assert!(offset_of!(Versions, ids) == 40);
    assert!(offset_of!(Versions, names) == 48);
    assert!(size_of::<ProviderOverride>() == 24);
    assert!(offset_of!(ProviderOverride, id) == 16);
    assert!(size_of::<SwapchainDesc>() == 64);
    assert!(offset_of!(SwapchainDesc, output) == 16);
    assert!(offset_of!(SwapchainDesc, desc) == 32);
    assert!(offset_of!(SwapchainDesc, queue) == 56);
    assert!(size_of::<Create>() == 40);
    assert!(offset_of!(Create, display) == 20);
    assert!(offset_of!(Create, max_render) == 28);
    assert!(size_of::<Configure>() == 144);
    // ffxConfigureDescFrameGenerationSwapChainRegisterUiResourceDX12.
    assert!(offset_of!(RegisterUi, ui) == 16);
    assert!(offset_of!(RegisterUi, flags) == 64);
    assert!(size_of::<RegisterUi>() == 72);
    // ffxConfigureDescFrameGenerationSwapChainKeyValueDX12 and
    // FfxApiSwapchainFramePacingTuning (C bools are a byte).
    assert!(offset_of!(KeyValue, key) == 16);
    assert!(offset_of!(KeyValue, ptr) == 32);
    assert!(size_of::<KeyValue>() == 40);
    assert!(offset_of!(PacingTuning, allow_hybrid_spin) == 8);
    assert!(offset_of!(PacingTuning, hybrid_spin_time) == 12);
    assert!(offset_of!(PacingTuning, allow_wait_for_single_object_on_fence) == 16);
    assert!(size_of::<PacingTuning>() == 20);
    assert!(offset_of!(Configure, generate_callback) == 40);
    assert!(offset_of!(Configure, generate_user) == 48);
    assert!(offset_of!(Configure, enabled) == 56);
    assert!(offset_of!(Configure, id) == 136);
    assert!(size_of::<Prepare>() == 232);
    assert!(offset_of!(Prepare, render) == 40);
    assert!(offset_of!(Prepare, jitter) == 48);
    assert!(offset_of!(Prepare, scale) == 56);
    assert!(offset_of!(Prepare, depth) == 88);
    assert!(offset_of!(Prepare, motion) == 136);
    assert!(offset_of!(Generate, color) == 24);
    assert!(offset_of!(Generate, output) == 72);
    assert!(offset_of!(Generate, count) == 264);
};

type CreateFn = unsafe extern "C" fn(*mut *mut c_void, *mut Header, *const c_void) -> u32;
type DestroyFn = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> u32;
type CallFn = unsafe extern "C" fn(*mut *mut c_void, *mut Header) -> u32;

struct Api {
    create: CreateFn,
    destroy: DestroyFn,
    configure: CallFn,
    query: CallFn,
    dispatch: CallFn,
    legacy_providers: Option<Providers>,
}

#[derive(Clone, Copy)]
struct Providers {
    generator: u64,
    swapchain: u64,
}

static API: OnceLock<Api> = OnceLock::new();

/// A provider for an effect. A null device enumerates the SDK's built-in
/// providers. The actual DX12 device also exposes its driver providers.
fn find_provider(query: CallFn, effect: u64, device: *mut c_void, version: &[u8]) -> Option<u64> {
    let mut count = 0;
    let mut versions = Versions {
        header: header(GET_VERSIONS),
        effect,
        device,
        count: &mut count,
        ids: null_mut(),
        names: null_mut(),
    };
    // SAFETY: a null context requests version enumeration. The descriptor
    // and its count remain live for both calls.
    if unsafe { query(null_mut(), &mut versions.header) } != OK || !(1..=64).contains(&count) {
        return None;
    }
    let mut ids = vec![0; count as usize];
    let mut names = vec![null(); count as usize];
    versions.ids = ids.as_mut_ptr();
    versions.names = names.as_mut_ptr();
    // SAFETY: both arrays have the capacity in the count slot.
    if unsafe { query(null_mut(), &mut versions.header) } != OK {
        return None;
    }
    ids.into_iter().zip(names).take(count as usize).find_map(|(id, name)| {
        if id == 0 || name.is_null() {
            return None;
        }
        // SAFETY: the API returns static NUL-terminated version names.
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        let matches = if version.ends_with(b".") { name.starts_with(version) } else { name == version };
        matches.then_some(id)
    })
}

/// Loads AMD's frame generation from `dir` (the FidelityFX SDK's
/// `signedbin`, as for the upscaler); its presence turns frame generation
/// on. The DLL's version, or why it did not load.
pub(crate) fn load(dir: &Path, ml: bool) -> Result<String, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    let path = std::path::absolute(dir.join(DLL)).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err(format!("no {DLL} there"));
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a NUL-terminated path; its own folder and System32 only.
    let module = unsafe { LoadLibraryExW(wide.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return Err(format!("{DLL} did not load ({})", std::io::Error::last_os_error()));
    }
    let find = |name: &CStr| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
            .map(|f| f as *const c_void)
            .ok_or_else(|| format!("{DLL} has no {}", name.to_string_lossy()))
    };
    // SAFETY: the API's query export, with its signature.
    let query = unsafe { std::mem::transmute::<*const c_void, CallFn>(find(c"ffxQuery")?) };
    let legacy_providers = if ml {
        None
    } else {
        Some(Providers {
            generator: find_provider(query, CREATE, null_mut(), b"3.")
                .ok_or("no built-in Frame Generation 3 provider")?,
            swapchain: find_provider(query, SWAPCHAIN_CREATE, null_mut(), b"3.")
                .ok_or("no built-in Frame Generation 3 swap chain provider")?,
        })
    };
    // SAFETY: the API's exports, with their signatures.
    let api = unsafe {
        Api {
            create: std::mem::transmute::<*const c_void, CreateFn>(find(c"ffxCreateContext")?),
            destroy: std::mem::transmute::<*const c_void, DestroyFn>(find(c"ffxDestroyContext")?),
            configure: std::mem::transmute::<*const c_void, CallFn>(find(c"ffxConfigure")?),
            query,
            dispatch: std::mem::transmute::<*const c_void, CallFn>(find(c"ffxDispatch")?),
            legacy_providers,
        }
    };
    let _ = API.set(api);
    Ok(path.display().to_string())
}

fn api() -> Option<&'static Api> {
    API.get()
}

/// Whether frame generation is on (its DLL loaded).
pub(crate) fn enabled() -> bool {
    API.get().is_some()
}

pub(crate) fn ml_enabled() -> bool {
    api().is_some_and(|api| api.legacy_providers.is_none())
}

/// What frame generation needs of an upscaled frame (see
/// `external::FrameInputs`), with the host copies of its depth and motion
/// vectors, which it holds.
#[derive(Clone)]
pub(crate) struct Inputs {
    pub(crate) depth: Arc<HostTexture>,
    pub(crate) motion: Arc<HostTexture>,
    pub(crate) frame: crate::external::FrameInputs,
    pub(crate) pose: Option<camera::Pose>,
}

// SAFETY: D3D12 resources are free-threaded; the rest is plain data.
unsafe impl Send for Inputs {}
// SAFETY: as above.
unsafe impl Sync for Inputs {}

static SUBMITTED: Mutex<Option<Inputs>> = Mutex::new(None);

/// Counts for the periodic report: upscales given over before a present
/// took the one before (`OVERWRITTEN`), presents with no upscale
/// (`WITHOUT`), presents generated for, resets, and the time the proxy's
/// waits took (µs).
static OVERWRITTEN: AtomicU64 = AtomicU64::new(0);
static WITHOUT: AtomicU64 = AtomicU64::new(0);
static GENERATED_FOR: AtomicU64 = AtomicU64::new(0);
static RESETS: AtomicU64 = AtomicU64::new(0);
static WAITED_US: AtomicU64 = AtomicU64::new(0);
/// Presents whose HUD-less colour was the frame before the UI layer.
static BEFORE_UI_USED: AtomicU64 = AtomicU64::new(0);
/// Presents the title's UI layer went to the proxy with.
static UI_LAYERS: AtomicU64 = AtomicU64::new(0);
/// Frames the proxy put on the screen since the start, generated ones too
/// (the stats panel's final frame rate).
static SHOWN: AtomicU64 = AtomicU64::new(0);

/// The title's frame before its UI layer (see [`keep_before_ui`]): the
/// texture, its size, whether it was ever copied into (else it is still in
/// the copy-destination state it was made in), and the copies in lists that
/// no present has taken yet. Counted, not marked: the title records the next
/// frame's UI draw before this frame is presented, and a mark taken by the
/// present before left every fourth present without one (it then fell back to
/// the buffer with the text in it, and the text shook).
struct BeforeUi {
    texture: ID3D12Resource,
    size: [u32; 2],
    used: bool,
    pending: u32,
}

static BEFORE_UI: Mutex<Option<BeforeUi>> = Mutex::new(None);

/// At a draw that lays the title's UI layer over its finished frame (a
/// pipeline that blends premultiplied colour over an RGBA8 target, one
/// full-screen triangle): the target `target` (a host texture, in the copy
/// source state in `list`) is copied as it is before it, into the HUD-less
/// colour the next present gives frame generation. With it the generated
/// frames take the UI (text, prompts, the phone) from the real frames
/// instead of blending it between two: it shook and smeared. One texture
/// does: the present that reads it runs on the queue after this list, and
/// the next frame's copy after that present.
pub(crate) fn keep_before_ui(device: &ID3D12Device, list: &HostList, target: &ID3D12Resource) {
    use crate::host::transition;
    if !enabled() {
        return;
    }
    // SAFETY: a live resource.
    let desc = unsafe { target.GetDesc() };
    if desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM || desc.SampleDesc.Count != 1 {
        return;
    }
    let size = [desc.Width as u32, desc.Height];
    let mut kept = BEFORE_UI.lock().unwrap_or_else(|e| e.into_inner());
    if kept.as_ref().is_none_or(|k| k.size != size) {
        let Some(texture) = hudless_texture(device, size) else { return };
        *kept = Some(BeforeUi { texture, size, used: false, pending: 0 });
    }
    let Some(k) = kept.as_mut() else { return };
    let mut before = Vec::new();
    if !k.used {
        k.used = true;
    } else {
        before.push(transition(&k.texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST));
    }
    // SAFETY: an open list; both live and of one size and format.
    unsafe {
        if !before.is_empty() {
            list.list.ResourceBarrier(&before);
        }
        list.list.CopyResource(&k.texture, target);
        list.list.ResourceBarrier(&[transition(
            &k.texture,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        )]);
    }
    list.keep(&k.texture);
    // (At most two ahead: a frame drawn without the UI layer leaves none.)
    k.pending = (k.pending + 1).min(2);
}

/// Whether frame generation runs for the coming presents (the presenter
/// armed it): the title's UI layer then goes to the proxy swap chain
/// ([`keep_ui_layer`]) instead of being drawn into the frame.
static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the proxy was last told to wait on fence events ([`fence_waits`]).
static PACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The title's UI layer for the proxy swap chain (`VICTORY_FG_UI_LAYER=0`:
/// not, the frame before the UI as HUD-less colour only): our copy of it,
/// its size, whether it was ever copied into, the copies no present took.
struct UiLayer {
    texture: ID3D12Resource,
    size: [u32; 2],
    used: bool,
    pending: u32,
}

static UI_LAYER: Mutex<Option<UiLayer>> = Mutex::new(None);

fn ui_layer_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_FG_UI_LAYER").is_none_or(|v| v != "0"))
}

/// At the draw that lays the title's UI layer `layer` (premultiplied, the
/// frame's size, in a shader-read state in `list`) over its frame: while
/// frame generation runs, a copy of it for the proxy swap chain to lay over
/// every frame it shows, and true: the draw is left out, the frame goes to
/// the present without its UI. Interpolated, even with a HUD-less colour,
/// the text shook. False (the title draws it) when frame generation does
/// not run, or the layer is not of the frame's size and format.
pub(crate) fn keep_ui_layer(device: &ID3D12Device, list: &HostList, layer: &ID3D12Resource, frame_size: [u32; 2]) -> bool {
    use crate::host::transition;
    if !enabled() || !ui_layer_on() || !ARMED.load(Ordering::Acquire) {
        return false;
    }
    // SAFETY: a live resource.
    let desc = unsafe { layer.GetDesc() };
    let size = [desc.Width as u32, desc.Height];
    if desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM || desc.SampleDesc.Count != 1 || size != frame_size {
        return false;
    }
    let mut kept = UI_LAYER.lock().unwrap_or_else(|e| e.into_inner());
    if kept.as_ref().is_none_or(|k| k.size != size) {
        let Some(texture) = hudless_texture(device, size) else { return false };
        *kept = Some(UiLayer { texture, size, used: false, pending: 0 });
    }
    let Some(k) = kept.as_mut() else { return false };
    // SAFETY: an open list; both live and of one size and format.
    unsafe {
        if k.used {
            list.list.ResourceBarrier(&[transition(&k.texture, UI_READ, D3D12_RESOURCE_STATE_COPY_DEST)]);
        }
        list.list.CopyResource(&k.texture, layer);
        list.list.ResourceBarrier(&[transition(&k.texture, D3D12_RESOURCE_STATE_COPY_DEST, UI_READ)]);
    }
    k.used = true;
    list.keep(&k.texture);
    k.pending = (k.pending + 1).min(2);
    true
}

/// The state our UI layer copy is read in (by the proxy's composition).
const UI_READ: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES =
    windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES(
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
            | windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0,
    );

/// The UI layer copy for this present, if one of `size` is in a list no
/// present took (one is taken).
fn take_ui_layer(size: [u32; 2]) -> Option<ID3D12Resource> {
    let mut kept = UI_LAYER.lock().unwrap_or_else(|e| e.into_inner());
    let k = kept.as_mut()?;
    if k.pending == 0 || k.size != size {
        return None;
    }
    k.pending -= 1;
    Some(k.texture.clone())
}

/// The UI layer copy the next present will take ([`take_ui_layer`]), and
/// the state it is in between lists: for the panel, drawn onto it so the
/// layer does not cover it.
pub(crate) fn ui_layer_ahead(size: [u32; 2]) -> Option<(ID3D12Resource, windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES)> {
    let kept = UI_LAYER.lock().unwrap_or_else(|e| e.into_inner());
    let k = kept.as_ref()?;
    (k.pending > 0 && k.size == size).then(|| (k.texture.clone(), UI_READ))
}

/// The frame before the UI layer for this present, if a copy of it of
/// `size` is in a list no present took (one is taken).
fn take_before_ui(size: [u32; 2]) -> Option<ID3D12Resource> {
    let mut kept = BEFORE_UI.lock().unwrap_or_else(|e| e.into_inner());
    let k = kept.as_mut()?;
    if k.pending == 0 || k.size != size {
        return None;
    }
    k.pending -= 1;
    Some(k.texture.clone())
}

/// [`SHOWN`].
pub(crate) fn shown_total() -> u64 {
    SHOWN.load(Ordering::Relaxed)
}

/// The proxy's wait for its queued frames, timed into `WAITED_US`.
fn wait_queued(proxy: &Context) -> u32 {
    let started = std::time::Instant::now();
    let code = proxy.dispatch(&mut header(SWAPCHAIN_WAIT));
    WAITED_US.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    code
}

/// At an upscale recorded into `list`: its inputs feed the next present
/// once the list is submitted (the present runs after it on the queue).
pub(crate) fn record(list: &HostList, inputs: Inputs) {
    list.defer_host(move || {
        let mut submitted = inputs.clone();
        if ml_enabled() {
            submitted.pose = camera::take_matching(submitted.frame.render, submitted.frame.jitter);
        }
        if SUBMITTED.lock().unwrap_or_else(|e| e.into_inner()).replace(submitted).is_some() {
            OVERWRITTEN.fetch_add(1, Ordering::Relaxed);
        }
    });
}

fn take_inputs() -> Option<Inputs> {
    SUBMITTED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

struct Context {
    api: &'static Api,
    handle: *mut c_void,
}

// SAFETY: the presenter's mutex serializes every use, including the
// callbacks the proxy makes during Present.
unsafe impl Send for Context {}

impl Context {
    fn create(api: &'static Api, desc: &mut Header) -> Result<Self, u32> {
        let mut ctx = Self { api, handle: null_mut() };
        // SAFETY: the typed chain and the handle slot live through the call.
        let code = unsafe { (api.create)(&mut ctx.handle, desc, null()) };
        if code == OK && !ctx.handle.is_null() { Ok(ctx) } else { Err(code) }
    }

    fn configure(&self, desc: &mut Configure) -> u32 {
        // SAFETY: our context and the matching description.
        unsafe { (self.api.configure)(&mut { self.handle }, &mut desc.header) }
    }

    /// The proxy's UI resource: `ui` (our copy of the title's UI layer, of
    /// `size`), or none.
    fn register_ui(&self, ui: Option<&ID3D12Resource>, size: [u32; 2]) -> u32 {
        let mut desc = RegisterUi {
            header: header(SWAPCHAIN_REGISTER_UI),
            ui: match ui {
                Some(t) => Resource {
                    resource: t.as_raw(),
                    description: [TEXTURE_2D, RGBA8_UNORM, size[0], size[1], 1, 1, 0, 0],
                    state: FFX_PIXEL_COMPUTE_READ,
                },
                None => Resource { resource: null_mut(), description: [0; 8], state: 0 },
            },
            flags: UI_PREMULTIPLIED | UI_DOUBLE_BUFFERED,
        };
        // SAFETY: the proxy's context and the matching description.
        unsafe { (self.api.configure)(&mut { self.handle }, &mut desc.header) }
    }

    /// The proxy's frame pacing: with fence waits ([`PacingTuning`]) or the
    /// SDK's defaults.
    fn pace(&self, fence_waits: bool) -> u32 {
        // (Static: whether the proxy copies the tuning or keeps the pointer,
        // it stays valid.)
        static TUNINGS: std::sync::LazyLock<[PacingTuning; 2]> =
            std::sync::LazyLock::new(|| [PacingTuning::with_fence_waits(false), PacingTuning::with_fence_waits(true)]);
        let tuning: *const PacingTuning = &TUNINGS[usize::from(fence_waits)];
        let mut desc = KeyValue { header: header(SWAPCHAIN_KEY_VALUE), key: KEY_FRAME_PACING, value: 0, ptr: tuning.cast_mut().cast() };
        // SAFETY: the proxy's context and the matching description; the
        // tuning is never written and lives as long as the process.
        unsafe { (self.api.configure)(&mut { self.handle }, &mut desc.header) }
    }

    fn dispatch(&self, desc: &mut Header) -> u32 {
        // SAFETY: our context and the matching description.
        unsafe { (self.api.dispatch)(&mut { self.handle }, desc) }
    }

    fn provider(&self) -> String {
        let mut q = ProviderVersion { header: header(PROVIDER_VERSION), id: 0, name: null() };
        // SAFETY: a provider query on our context.
        let code = unsafe { (self.api.query)(&mut { self.handle }, &mut q.header) };
        if code != OK || q.name.is_null() {
            return String::new();
        }
        // SAFETY: a NUL-terminated name the DLL keeps.
        unsafe { CStr::from_ptr(q.name) }.to_string_lossy().into_owned()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: owned, and no present is in flight when it is released.
        unsafe { (self.api.destroy)(&mut self.handle, null()) };
    }
}

struct Generator {
    context: Context,
    generated: AtomicU64,
    error: AtomicU32,
}

/// The proxy's call during Present: generate the frames.
unsafe extern "C" fn generate(desc: *mut Generate, user: *mut c_void) -> u32 {
    // SAFETY: the configured callback gets our boxed generator and a live
    // dispatch description the SDK owns.
    let g = unsafe { &*user.cast::<Generator>() };
    // SAFETY: as above.
    let code = unsafe { (g.context.api.dispatch)(&mut { g.context.handle }, desc.cast()) };
    if code == OK {
        // SAFETY: the description is live for this call; the SDK may update
        // the generated-frame count in it.
        let count = unsafe { std::ptr::addr_of!((*desc).count).read() };
        g.generated.fetch_add(u64::from(count), Ordering::Relaxed);
    } else {
        g.error.store(code, Ordering::Relaxed);
    }
    code
}

/// Frame generation for one presenter: the proxy swap chain's context and
/// the generator for the current sizes.
pub(crate) struct Presentation {
    api: &'static Api,
    generator_provider: u64,
    proxy: Context,
    generator: Option<Box<Generator>>,
    key: Option<([u32; 2], [u32; 2], u32)>,
    armed: bool,
    next_id: u64,
    continuous: u64,
    last_frame: u64,
    base_presents: u32,
    logged: bool,
    missing_camera_warning: camera::MissingCameraWarning,
    /// The HUD-less colour for this present and its size, if kept.
    hudless: Option<(ID3D12Resource, [u32; 2])>,
    /// The title's frame before its UI layer for this present
    /// ([`keep_before_ui`]): the HUD-less colour in place of `hudless`.
    before_ui: Option<ID3D12Resource>,
    /// `hudless` holds this present's frame (in a compute-readable state).
    hudless_kept: bool,
    /// The proxy's present count when last looked at.
    shown_count: u32,
    /// The proxy has a UI resource ([`keep_ui_layer`]).
    ui_registered: bool,
}

impl Presentation {
    /// The proxy swap chain for `hwnd` and its frame generation, in place
    /// of the plain one; `None` when off, for another format, or if the
    /// proxy is refused (the presenter makes a plain one).
    pub(crate) fn create(
        factory: &IDXGIFactory2,
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
        hwnd: HWND,
        desc: &DXGI_SWAP_CHAIN_DESC1,
    ) -> Option<(IDXGISwapChain3, Self)> {
        let api = api()?;
        if desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM {
            tracing::warn!(format = desc.Format.0, "FidelityFX frame generation: only an RGBA8 swap chain; presenting without it");
            return None;
        }
        let providers = if let Some(legacy) = api.legacy_providers {
            legacy
        } else {
            let (Some(generator), Some(swapchain)) = (
                find_provider(api.query, CREATE, device.as_raw(), b"4.0.2"),
                find_provider(api.query, SWAPCHAIN_CREATE, device.as_raw(), b"4.0.0"),
            ) else {
                tracing::warn!("FidelityFX frame generation: the GPU did not offer the 4.0.2 generator and 4.0.0 proxy; off");
                return None;
            };
            Providers { generator, swapchain }
        };
        let mut version = Version { header: header(SWAPCHAIN_VERSION), version: 4 << 22 };
        let mut provider = ProviderOverride {
            header: Header { kind: OVERRIDE_VERSION, next: &mut version.header },
            id: providers.swapchain,
        };
        let mut output = null_mut();
        let mut args = SwapchainDesc {
            header: Header { kind: SWAPCHAIN_CREATE, next: &mut provider.header },
            output: &mut output,
            hwnd,
            desc,
            fullscreen_desc: null(),
            factory: factory.as_raw(),
            queue: queue.as_raw(),
        };
        let proxy = Context::create(api, &mut args.header)
            .inspect_err(|code| tracing::warn!(code, "FidelityFX frame generation: no proxy swap chain; presenting without it"))
            .ok()?;
        if output.is_null() {
            return None;
        }
        // SAFETY: the SDK returned an owned IDXGISwapChain4 reference.
        let swap_chain = unsafe { IDXGISwapChain4::from_raw(output) }.cast().ok()?;
        let proxy_provider = proxy.provider();
        if api.legacy_providers.is_none() && proxy_provider != "4.0.0" {
            tracing::warn!(provider = proxy_provider, "FidelityFX frame generation: unexpected ML proxy provider; off");
            return None;
        }
        tracing::info!(provider = proxy_provider, "FidelityFX frame generation: proxy swap chain");
        let waits = fence_waits();
        let code = proxy.pace(waits);
        tracing::info!(code, fence_waits = waits, "FidelityFX frame generation: frame pacing (VICTORY_FG_FENCE_WAIT)");
        PACED.store(waits, Ordering::Relaxed);
        Some((
            swap_chain,
            Self { api, generator_provider: providers.generator, proxy, generator: None, key: None, armed: false, next_id: 0, continuous: 0, last_frame: 0, base_presents: 0, logged: false, missing_camera_warning: camera::MissingCameraWarning::default(), hudless: None, hudless_kept: false, before_ui: None, shown_count: 0, ui_registered: false },
        ))
    }

    /// Frame generation off for the coming presents (another size, or no
    /// upscaled frame): waits for the proxy's queued frames.
    pub(crate) fn disable(&mut self, chain: &IDXGISwapChain3) {
        if !self.armed {
            return;
        }
        if let Some(g) = &self.generator {
            let code = g.context.configure(&mut self.config(chain, false, [0; 2]));
            if code != OK {
                tracing::warn!(code, "FidelityFX frame generation: not switched off");
            }
        }
        let _ = wait_queued(&self.proxy);
        if self.ui_registered {
            let _ = self.proxy.register_ui(None, [0; 2]);
            self.ui_registered = false;
        }
        self.armed = false;
        ARMED.store(false, Ordering::Release);
    }

    fn config(&self, chain: &IDXGISwapChain3, armed: bool, display: [u32; 2]) -> Configure {
        let generator = self.generator.as_deref().filter(|_| armed);
        Configure {
            header: header(CONFIGURE),
            swapchain: chain.as_raw(),
            present_callback: null(),
            present_user: null_mut(),
            generate_callback: generator.map(|_| generate as Callback),
            generate_user: generator.map_or(null_mut(), |g| std::ptr::from_ref(g).cast_mut().cast()),
            enabled: armed,
            allow_async: false,
            hudless: match (&self.before_ui, &self.hudless) {
                (Some(texture), _) if armed => Resource {
                    resource: texture.as_raw(),
                    description: [TEXTURE_2D, RGBA8_UNORM, display[0], display[1], 1, 1, 0, 0],
                    state: FFX_COMPUTE_READ,
                },
                (_, Some((texture, size))) if armed && self.hudless_kept && *size == display => Resource {
                    resource: texture.as_raw(),
                    description: [TEXTURE_2D, RGBA8_UNORM, size[0], size[1], 1, 1, 0, 0],
                    state: FFX_COMPUTE_READ,
                },
                _ => Resource { resource: null_mut(), description: [0; 8], state: 0 },
            },
            flags: 0,
            only_generated: false,
            rect: [0, 0, display[0] as i32, display[1] as i32],
            id: self.next_id,
        }
    }

    /// Keeps the swap chain buffer `back` (in the copy-destination state, at
    /// `size`) as it is now, before the presenter's panel is drawn over it,
    /// as the HUD-less colour for this present: a copy in `list`, left
    /// compute-readable. One texture does: generation runs in the Present
    /// on the presenter's queue (`allow_async` off), before the next
    /// present's copy into it.
    pub(crate) fn keep_hudless(&mut self, device: &ID3D12Device, list: &HostList, back: &ID3D12Resource, size: [u32; 2]) {
        use crate::host::transition;
        self.hudless_kept = false;
        let reuse = self.hudless.as_ref().is_some_and(|(_, s)| *s == size);
        if !reuse {
            self.hudless = hudless_texture(device, size).map(|t| (t, size));
        }
        let Some((texture, _)) = &self.hudless else { return };
        let mut before = vec![transition(back, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE)];
        if reuse {
            before.push(transition(texture, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST));
        }
        // SAFETY: an open list; both resources live (the texture is kept by
        // the list and by `self`), of one size and format.
        unsafe {
            list.list.ResourceBarrier(&before);
            list.list.CopyResource(texture, back);
            list.list.ResourceBarrier(&[
                transition(back, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
                transition(texture, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE),
            ]);
        }
        list.keep(texture);
        self.hudless_kept = true;
    }

    /// Before presenting frame `frame` at `display` size: the generator
    /// configured and given the last submitted upscale's depth and motion
    /// (recorded into `list`, which runs before the present). True if
    /// frames will be generated for this present.
    pub(crate) fn prepare(
        &mut self,
        chain: &IDXGISwapChain3,
        device: &ID3D12Device,
        list: &HostList,
        display: [u32; 2],
        frame: u64,
    ) -> bool {
        if frame.is_multiple_of(300) {
            let take = |c: &AtomicU64| c.swap(0, Ordering::Relaxed);
            tracing::info!(
                frame,
                generated_for = take(&GENERATED_FOR),
                without_upscale = take(&WITHOUT),
                upscales_overwritten = take(&OVERWRITTEN),
                resets = take(&RESETS),
                waited_ms = take(&WAITED_US) / 1000,
                before_ui = take(&BEFORE_UI_USED),
                ui_layers = take(&UI_LAYERS),
                "FidelityFX frame generation: presents since the last report"
            );
        }
        // `VICTORY_AB=<s>,VICTORY_FIDELITYFX_FRAMEGEN`: generation off in
        // the A phases (the proxy still presents), for its cost in one run.
        if crate::ab::pick("VICTORY_FIDELITYFX_FRAMEGEN") == Some(false) {
            let _ = take_inputs();
            self.disable(chain);
            return false;
        }
        if ml_enabled() {
            camera::clear();
        }
        let Some(input) = take_inputs().filter(|i| i.frame.render.into_iter().zip(display).all(|(r, d)| r > 0 && r <= d)) else {
            WITHOUT.fetch_add(1, Ordering::Relaxed);
            self.disable(chain);
            return false;
        };
        let pose = if ml_enabled() {
            if self.missing_camera_warning.observe(input.pose.is_some()) {
                tracing::warn!(
                    upscales = camera::WARNING_AFTER_UPSCALES,
                    "FidelityFX Frame Generation 4.0.2: no matching velocity-pass camera; presenting ordinary frames. The shader pair was verified on title 1.0.2.1. Set VICTORY_LOG=info,victory_gpu::framegen=debug for matching details"
                );
            }
            let Some(pose) = input.pose else {
                tracing::debug!(frame, "FidelityFX frame generation: no camera matching this upscale");
                self.disable(chain);
                return false;
            };
            pose
        } else {
            camera::Pose::LEGACY
        };
        let flags = input.frame.flags & MOTION_DEPTH_FLAGS;
        let key = (display, input.frame.render, flags);
        if self.key != Some(key) {
            self.renew(chain, device, key);
        }
        self.before_ui = take_before_ui(display);
        if let Some(texture) = &self.before_ui {
            list.keep(texture);
            BEFORE_UI_USED.fetch_add(1, Ordering::Relaxed);
        }
        // The title's UI layer, laid over every frame the proxy shows; none
        // when this frame drew its own (frame generation was not armed).
        let ui = take_ui_layer(display);
        if ui.is_some() || self.ui_registered {
            if let Some(texture) = &ui {
                list.keep(texture);
                UI_LAYERS.fetch_add(1, Ordering::Relaxed);
            }
            let code = self.proxy.register_ui(ui.as_ref(), display);
            if code != OK {
                tracing::warn!(code, "FidelityFX frame generation: the UI layer was not taken");
            }
            self.ui_registered = ui.is_some();
        }
        let Some(g) = &self.generator else { return false };
        let code = g.context.configure(&mut self.config(chain, true, display));
        if code != OK {
            tracing::warn!(code, "FidelityFX frame generation: configure failed");
            self.disable(chain);
            return false;
        }
        let reset = input.frame.reset || !self.armed || self.last_frame + 1 != frame;
        let mut barriers = Vec::new();
        let depth = read(list, &input.depth, input.frame.depth_desc, &mut barriers);
        let motion = read(list, &input.motion, input.frame.motion_desc, &mut barriers);
        if !barriers.is_empty() {
            // SAFETY: an open list and textures the inputs hold.
            unsafe { list.list.ResourceBarrier(&barriers) };
        }
        list.keep(&input.depth.texture);
        list.keep(&input.motion.texture);
        let mut prepare = Prepare {
            header: header(PREPARE),
            id: self.next_id,
            flags: 0,
            list: list.list.as_raw(),
            render: input.frame.render,
            jitter: input.frame.jitter,
            scale: input.frame.motion_scale,
            time_ms: input.frame.frame_time_ms,
            reset,
            camera: input.frame.camera,
            depth,
            motion,
            camera_position: pose.position,
            camera_up: pose.up,
            camera_right: pose.right,
            camera_forward: pose.forward,
        };
        self.armed = true;
        ARMED.store(true, Ordering::Release);
        let code = g.context.dispatch(&mut prepare.header);
        if code != OK {
            tracing::warn!(code, "FidelityFX frame generation: prepare failed");
            self.disable(chain);
            return false;
        }
        self.last_frame = frame;
        self.next_id += 1;
        self.continuous += u64::from(!reset);
        GENERATED_FOR.fetch_add(1, Ordering::Relaxed);
        RESETS.fetch_add(u64::from(reset), Ordering::Relaxed);
        if self.next_id == 1 {
            let screen = display;
            tracing::info!(frame, render = ?input.frame.render, ?screen, camera_pose = ml_enabled(), hudless = self.hudless_kept, depth = ?input.frame.depth_desc, "FidelityFX frame generation: first upscaled frame's depth and motion given to it");
        }
        true
    }

    /// A new generator for (display, render, flags).
    fn renew(&mut self, chain: &IDXGISwapChain3, device: &ID3D12Device, key: ([u32; 2], [u32; 2], u32)) {
        self.disable(chain);
        self.generator = None;
        self.key = Some(key);
        self.next_id = 0;
        self.continuous = 0;
        let _ = wait_queued(&self.proxy);
        // SAFETY: the proxy swap chain is live; the presenter serializes us.
        self.base_presents = unsafe { chain.GetLastPresentCount() }.unwrap_or(0);
        let (screen, render, flags) = key;
        let mut backend = BackendDx12 { header: header(BACKEND_DX12), device: device.as_raw() };
        let mut version = Version { header: Header { kind: VERSION, next: &mut backend.header }, version: (4 << 22) | 2 };
        let mut provider = ProviderOverride {
            header: Header { kind: OVERRIDE_VERSION, next: &mut version.header },
            id: self.generator_provider,
        };
        let mut args = Create { header: Header { kind: CREATE, next: &mut provider.header }, flags, display: screen, max_render: render, format: RGBA8_UNORM };
        match Context::create(self.api, &mut args.header) {
            Ok(context) => {
                let provider = context.provider();
                let expected = if ml_enabled() { provider == "4.0.2" } else { provider.starts_with("3.") };
                if expected {
                    tracing::info!(provider, ?screen, ?render, "FidelityFX frame generation: generator for the upscaled frames");
                    self.generator = Some(Box::new(Generator { context, generated: AtomicU64::new(0), error: AtomicU32::new(0) }));
                } else {
                    tracing::warn!(provider, "FidelityFX frame generation: unexpected generator provider; off");
                }
            }
            Err(code) => tracing::warn!(code, ?screen, ?render, "FidelityFX frame generation: no generator"),
        }
    }

    /// After any present through the proxy: what it showed since the last
    /// one (its present count, which has the generated frames) goes to
    /// [`SHOWN`].
    pub(crate) fn count_shown(&mut self, chain: &IDXGISwapChain3) {
        // SAFETY: a live proxy swap chain; the presenter serializes us.
        let count = unsafe { chain.GetLastPresentCount() }.unwrap_or(self.shown_count);
        let new = if count >= self.shown_count { count - self.shown_count } else { count };
        SHOWN.fetch_add(u64::from(new), Ordering::Relaxed);
        self.shown_count = count;
    }

    /// After presenting frame `frame`: a failed generation switches it off;
    /// the first steady stretch is logged with the presents it made.
    pub(crate) fn presented(&mut self, chain: &IDXGISwapChain3, frame: u64) {
        // An A/B run's phase may have changed the pacing.
        let waits = fence_waits();
        if PACED.swap(waits, Ordering::Relaxed) != waits {
            let code = self.proxy.pace(waits);
            tracing::info!(code, fence_waits = waits, "FidelityFX frame generation: frame pacing changed");
        }
        let Some(g) = &self.generator else { return };
        let error = g.error.load(Ordering::Relaxed);
        if error != OK {
            tracing::warn!(error, "FidelityFX frame generation: generating failed; off");
            self.disable(chain);
        } else if !self.logged && self.continuous >= 2 && g.generated.load(Ordering::Relaxed) >= 3 {
            let waited = self.proxy.dispatch(&mut header(SWAPCHAIN_WAIT));
            // SAFETY: a live proxy swap chain with completed presents.
            let presents = unsafe { chain.GetLastPresentCount() }.unwrap_or(0).saturating_sub(self.base_presents);
            tracing::info!(
                frame,
                rendered = self.next_id,
                continuous = self.continuous,
                generated = g.generated.load(Ordering::Relaxed),
                presents,
                waited,
                "FidelityFX frame generation: generated frames"
            );
            self.logged = true;
        }
    }
}

/// An RGBA8 texture of `size` on the card, in the copy-destination state,
/// for the HUD-less colour; `None` (with a warning once) if not made.
fn hudless_texture(device: &ID3D12Device, size: [u32; 2]) -> Option<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(size[0]),
        Height: size[1],
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        ..Default::default()
    };
    let mut texture: Option<ID3D12Resource> = None;
    // SAFETY: plain creation of a card texture.
    let made = unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() },
            crate::host::not_resident(),
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut texture,
        )
    }
    .and_then(|()| texture.as_ref().map_or(Ok(()), |t| crate::host::resident(device, t)));
    match made {
        Ok(()) => texture,
        Err(e) => {
            tracing::warn!(error = %e, ?size, "FidelityFX frame generation: no HUD-less colour texture; the panel is interpolated");
            None
        }
    }
}

/// `copy` as frame generation reads it (compute read), moved there in
/// `list` (`barriers` gets the transition, if any).
fn read(list: &HostList, copy: &Arc<HostTexture>, description: [u32; 8], barriers: &mut Vec<D3D12_RESOURCE_BARRIER>) -> Resource {
    barriers.extend(list.move_texture(copy, HOST_NON_PIXEL_SHADER_RESOURCE));
    Resource { resource: copy.texture.as_raw(), description, state: FFX_COMPUTE_READ }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_dll_frame_generation_stays_off() {
        let dir = std::env::temp_dir().join("victory-no-framegen");
        let _ = std::fs::create_dir_all(&dir);
        assert!(load(&dir, false).unwrap_err().contains(DLL));
    }

    #[test]
    fn selects_the_builtin_three_provider_for_each_effect() {
        unsafe extern "C" fn query(ctx: *mut *mut c_void, desc: *mut Header) -> u32 {
            // SAFETY: find_provider passes its live Versions descriptor.
            let versions = unsafe { &mut *desc.cast::<Versions>() };
            if !ctx.is_null() || versions.header.kind != GET_VERSIONS || !versions.device.is_null() {
                return 1;
            }
            let provider_id = match versions.effect {
                CREATE => 31,
                SWAPCHAIN_CREATE => 32,
                _ => return 1,
            };
            // SAFETY: count is live. On the second call both arrays have two entries.
            unsafe {
                if versions.ids.is_null() && versions.names.is_null() {
                    *versions.count = 2;
                } else if *versions.count == 2 && !versions.ids.is_null() && !versions.names.is_null() {
                    *versions.ids = 40;
                    *versions.ids.add(1) = provider_id;
                    *versions.names = c"4.0.2".as_ptr();
                    *versions.names.add(1) = c"3.1.6".as_ptr();
                } else {
                    return 1;
                }
            }
            OK
        }

        assert_eq!(find_provider(query, CREATE, null_mut(), b"3."), Some(31));
        assert_eq!(find_provider(query, SWAPCHAIN_CREATE, null_mut(), b"3."), Some(32));
    }
}

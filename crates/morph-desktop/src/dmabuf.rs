//! Can WebKit's DMA-BUF renderer allocate its buffers here?
//!
//! WebKitGTK renders pages into GBM buffer objects created with the format
//! modifiers the EGL display offers. Where the driver refuses that
//! allocation ("Failed to create GBM buffer of size 2x2: Invalid argument",
//! seen on an NVIDIA GPU with nvidia-open 615.78 and webkit2gtk 2.54.1) pages
//! stay black. No vendor is named here: `morph --probe-dmabuf` (a child
//! process, so driver libraries and their threads never enter Morph) makes
//! the same allocation on the first render node, and only when it fails
//! does Morph pick one of WebKit's own fallbacks before GTK starts:
//! buffers exported from GL textures (`WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1`,
//! still on the GPU) when EGL can export them, else shared memory
//! (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`). GL compositing stays on either
//! way. Any of those variables set by the user, or `MORPH_DMABUF_PROBE=0`,
//! skips the probe.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What the probe found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// A buffer with the EGL display's modifiers was allocated.
    Works,
    /// The allocation WebKit makes failed; `export` says whether EGL can
    /// export GL textures as DMA-BUFs instead.
    Fails { export: bool },
    /// No render node, GBM or EGL: nothing to decide (WebKit falls back
    /// by itself).
    Unavailable,
}

impl Probe {
    pub fn code(self) -> i32 {
        match self {
            Probe::Works => 0,
            Probe::Fails { export: true } => 10,
            Probe::Fails { export: false } => 11,
            Probe::Unavailable => 12,
        }
    }

    pub fn from_code(code: Option<i32>) -> Probe {
        match code {
            Some(0) => Probe::Works,
            Some(10) => Probe::Fails { export: true },
            Some(11) => Probe::Fails { export: false },
            _ => Probe::Unavailable,
        }
    }
}

/// The variables WebKit reads that mean the user chose a renderer already.
pub const USER_CHOICES: [&str; 5] = [
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_DMABUF_RENDERER_DISABLE_GBM",
    "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    "WEBKIT_FORCE_DMABUF_RENDERER",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
];

/// The variable to set for a probe result, if any.
pub fn fallback(probe: Probe) -> Option<&'static str> {
    match probe {
        Probe::Fails { export: true } => Some("WEBKIT_DMABUF_RENDERER_DISABLE_GBM"),
        Probe::Fails { export: false } => Some("WEBKIT_DMABUF_RENDERER_FORCE_SHM"),
        Probe::Works | Probe::Unavailable => None,
    }
}

/// Run the probe in a child process (`morph --probe-dmabuf`), giving up
/// after two seconds.
pub fn probe_in_child() -> Probe {
    let Ok(exe) = std::env::current_exe() else {
        return Probe::Unavailable;
    };
    let Ok(mut child) = Command::new(exe).arg("--probe-dmabuf").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return Probe::Unavailable;
    };
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Probe::from_code(status.code()),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::Unavailable;
            }
        }
    }
}

const GBM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const GBM_BO_USE_RENDERING: u32 = 1 << 2;
const EGL_PLATFORM_GBM_KHR: c_uint = 0x31D7;
const EGL_EXTENSIONS: c_int = 0x3055;

type GbmCreateDevice = unsafe extern "C" fn(c_int) -> *mut c_void;
type GbmDeviceDestroy = unsafe extern "C" fn(*mut c_void);
type GbmBoCreate = unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void;
type GbmBoCreateWithModifiers2 = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u64, c_uint, u32) -> *mut c_void;
type GbmBoDestroy = unsafe extern "C" fn(*mut c_void);
type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type EglGetPlatformDisplay = unsafe extern "C" fn(c_uint, *mut c_void, *const isize) -> *mut c_void;
type EglInitialize = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_uint;
type EglTerminate = unsafe extern "C" fn(*mut c_void) -> c_uint;
type EglQueryString = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;
type EglQueryDmaBufModifiers = unsafe extern "C" fn(*mut c_void, c_int, c_int, *mut u64, *mut c_uint, *mut c_int) -> c_uint;

/// The probe itself (in the child): allocate a 2x2 ARGB8888 buffer for
/// rendering with the modifiers EGL offers for that format, as WebKit does.
pub fn probe() -> Probe {
    let Some(node) = std::fs::read_dir("/dev/dri").ok().and_then(|dir| {
        let mut nodes: Vec<_> =
            dir.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("renderD"))).collect();
        nodes.sort();
        nodes.into_iter().next()
    }) else {
        return Probe::Unavailable;
    };
    let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&node) else {
        return Probe::Unavailable;
    };
    // SAFETY: the libraries are the system's GBM and EGL; every symbol is
    // used with its C signature, and objects are freed before returning.
    unsafe {
        let (Ok(gbm), Ok(egl)) = (libloading::Library::new("libgbm.so.1"), libloading::Library::new("libEGL.so.1")) else {
            return Probe::Unavailable;
        };
        let (Ok(create_device), Ok(destroy_device), Ok(bo_create), Ok(bo_destroy)) = (
            gbm.get::<GbmCreateDevice>(b"gbm_create_device\0"),
            gbm.get::<GbmDeviceDestroy>(b"gbm_device_destroy\0"),
            gbm.get::<GbmBoCreate>(b"gbm_bo_create\0"),
            gbm.get::<GbmBoDestroy>(b"gbm_bo_destroy\0"),
        ) else {
            return Probe::Unavailable;
        };
        let device = create_device(file.as_raw_fd());
        if device.is_null() {
            return Probe::Fails { export: false };
        }
        let mut modifiers = Vec::new();
        let mut export = false;
        if let (Ok(get_proc), Ok(initialize), Ok(terminate), Ok(query_string)) = (
            egl.get::<EglGetProcAddress>(b"eglGetProcAddress\0"),
            egl.get::<EglInitialize>(b"eglInitialize\0"),
            egl.get::<EglTerminate>(b"eglTerminate\0"),
            egl.get::<EglQueryString>(b"eglQueryString\0"),
        ) {
            let platform = get_proc(c"eglGetPlatformDisplayEXT".as_ptr());
            if !platform.is_null() {
                let platform: EglGetPlatformDisplay = std::mem::transmute(platform);
                let display = platform(EGL_PLATFORM_GBM_KHR, device, std::ptr::null());
                if !display.is_null() && initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
                    let ext = query_string(display, EGL_EXTENSIONS);
                    let ext = if ext.is_null() { String::new() } else { CStr::from_ptr(ext).to_string_lossy().into_owned() };
                    export = ext.split_whitespace().any(|e| e == "EGL_MESA_image_dma_buf_export");
                    let query = get_proc(c"eglQueryDmaBufModifiersEXT".as_ptr());
                    if !query.is_null() && ext.split_whitespace().any(|e| e == "EGL_EXT_image_dma_buf_import_modifiers") {
                        let query: EglQueryDmaBufModifiers = std::mem::transmute(query);
                        let mut count: c_int = 0;
                        if query(display, GBM_FORMAT_ARGB8888 as c_int, 0, std::ptr::null_mut(), std::ptr::null_mut(), &mut count) != 0 && count > 0 {
                            let mut mods = vec![0u64; count as usize];
                            let mut external = vec![0 as c_uint; count as usize];
                            if query(display, GBM_FORMAT_ARGB8888 as c_int, count, mods.as_mut_ptr(), external.as_mut_ptr(), &mut count) != 0 {
                                modifiers =
                                    mods.into_iter().zip(external).take(count.max(0) as usize).filter(|(_, ext)| *ext == 0).map(|(m, _)| m).collect();
                            }
                        }
                    }
                    terminate(display);
                }
            }
        }
        let bo = match (modifiers.is_empty(), gbm.get::<GbmBoCreateWithModifiers2>(b"gbm_bo_create_with_modifiers2\0")) {
            (false, Ok(with_modifiers)) => {
                with_modifiers(device, 2, 2, GBM_FORMAT_ARGB8888, modifiers.as_ptr(), modifiers.len() as c_uint, GBM_BO_USE_RENDERING)
            }
            _ => bo_create(device, 2, 2, GBM_FORMAT_ARGB8888, GBM_BO_USE_RENDERING),
        };
        let works = !bo.is_null();
        if works {
            bo_destroy(bo);
        }
        destroy_device(device);
        if works { Probe::Works } else { Probe::Fails { export } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_allocation_picks_a_fallback_and_a_working_one_none() {
        assert_eq!(fallback(Probe::Works), None);
        assert_eq!(fallback(Probe::Unavailable), None);
        assert_eq!(fallback(Probe::Fails { export: true }), Some("WEBKIT_DMABUF_RENDERER_DISABLE_GBM"));
        assert_eq!(fallback(Probe::Fails { export: false }), Some("WEBKIT_DMABUF_RENDERER_FORCE_SHM"));
    }

    #[test]
    fn results_survive_the_exit_code() {
        for probe in [Probe::Works, Probe::Fails { export: true }, Probe::Fails { export: false }, Probe::Unavailable] {
            assert_eq!(Probe::from_code(Some(probe.code())), probe);
        }
        assert_eq!(Probe::from_code(None), Probe::Unavailable, "killed by a signal");
        assert_eq!(Probe::from_code(Some(101)), Probe::Unavailable, "a panic");
    }
}

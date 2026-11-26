//! Capture of what a Linux system plays: the monitor source of a PulseAudio sink.
//!
//! PipeWire serves the same protocol through `pipewire-pulse`, so this covers both sound
//! servers. The client libraries (`libpulse.so.0`, `libpulse-simple.so.0`) are loaded at run
//! time: the application still starts on systems without them and only lacks these sources.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use libloading::Library;

use crate::audio::{format_rate_channels, Capture, Feed};

const PA_SAMPLE_FLOAT32LE: c_int = 5;
const PA_STREAM_RECORD: c_int = 2;
const PA_CONTEXT_NOAUTOSPAWN: c_int = 1;
const PA_CONTEXT_READY: c_int = 4;
const PA_CONTEXT_FAILED: c_int = 5;
const PA_CONTEXT_TERMINATED: c_int = 6;
const PA_OPERATION_RUNNING: c_int = 0;
/// Time allowed for connecting to the sound server and listing the sinks.
const TIMEOUT: Duration = Duration::from_secs(2);

#[repr(C)]
#[derive(Clone, Copy)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

/// Leading fields of `pa_sink_info` (the rest is not read).
#[repr(C)]
struct SinkInfo {
    name: *const c_char,
    index: u32,
    description: *const c_char,
    sample_spec: SampleSpec,
}

/// Leading fields of `pa_server_info`.
#[repr(C)]
struct ServerInfo {
    user_name: *const c_char,
    host_name: *const c_char,
    server_version: *const c_char,
    server_name: *const c_char,
    sample_spec: SampleSpec,
    default_sink_name: *const c_char,
}

type SinkCb = unsafe extern "C" fn(*mut c_void, *const SinkInfo, c_int, *mut c_void);
type ServerCb = unsafe extern "C" fn(*mut c_void, *const ServerInfo, *mut c_void);

struct Api {
    _pulse: Library,
    _simple: Library,
    mainloop_new: unsafe extern "C" fn() -> *mut c_void,
    mainloop_get_api: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    mainloop_iterate: unsafe extern "C" fn(*mut c_void, c_int, *mut c_int) -> c_int,
    mainloop_free: unsafe extern "C" fn(*mut c_void),
    context_new: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    context_connect:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *const c_void) -> c_int,
    context_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    context_disconnect: unsafe extern "C" fn(*mut c_void),
    context_unref: unsafe extern "C" fn(*mut c_void),
    get_sink_info_list: unsafe extern "C" fn(*mut c_void, SinkCb, *mut c_void) -> *mut c_void,
    get_server_info: unsafe extern "C" fn(*mut c_void, ServerCb, *mut c_void) -> *mut c_void,
    operation_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    operation_unref: unsafe extern "C" fn(*mut c_void),
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    simple_new: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        c_int,
        *const c_char,
        *const c_char,
        *const SampleSpec,
        *const c_void,
        *const BufferAttr,
        *mut c_int,
    ) -> *mut c_void,
    simple_read: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_int) -> c_int,
    simple_free: unsafe extern "C" fn(*mut c_void),
}

impl Api {
    fn load() -> Result<Self> {
        // SAFETY: loading the PulseAudio client libraries runs no initialisation code with
        // requirements on the caller, and every symbol is used with its C signature.
        unsafe {
            let pulse = Library::new("libpulse.so.0")?;
            let simple = Library::new("libpulse-simple.so.0")?;
            macro_rules! sym {
                ($lib:expr, $name:literal) => {
                    *$lib.get(concat!($name, "\0").as_bytes())?
                };
            }
            Ok(Self {
                mainloop_new: sym!(pulse, "pa_mainloop_new"),
                mainloop_get_api: sym!(pulse, "pa_mainloop_get_api"),
                mainloop_iterate: sym!(pulse, "pa_mainloop_iterate"),
                mainloop_free: sym!(pulse, "pa_mainloop_free"),
                context_new: sym!(pulse, "pa_context_new"),
                context_connect: sym!(pulse, "pa_context_connect"),
                context_get_state: sym!(pulse, "pa_context_get_state"),
                context_disconnect: sym!(pulse, "pa_context_disconnect"),
                context_unref: sym!(pulse, "pa_context_unref"),
                get_sink_info_list: sym!(pulse, "pa_context_get_sink_info_list"),
                get_server_info: sym!(pulse, "pa_context_get_server_info"),
                operation_get_state: sym!(pulse, "pa_operation_get_state"),
                operation_unref: sym!(pulse, "pa_operation_unref"),
                strerror: sym!(pulse, "pa_strerror"),
                simple_new: sym!(simple, "pa_simple_new"),
                simple_read: sym!(simple, "pa_simple_read"),
                simple_free: sym!(simple, "pa_simple_free"),
                _pulse: pulse,
                _simple: simple,
            })
        }
    }

    fn error_text(&self, code: c_int) -> String {
        // SAFETY: pa_strerror returns a static string for any code.
        unsafe { text((self.strerror)(code)) }
    }
}

fn api() -> Result<&'static Api> {
    static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| Api::load().map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow!("PulseAudio client library not available: {e}"))
}

/// # Safety
/// `p` must be null or a valid C string.
unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// An output of the sound server.
#[derive(Clone, Debug)]
pub struct Sink {
    /// Sink name.
    pub name: String,
    /// Human-readable name.
    pub description: String,
    /// Sample rate of the sink.
    pub rate: u32,
    /// Channel count of the sink.
    pub channels: u8,
}

#[derive(Default)]
struct Collected {
    sinks: Vec<Sink>,
    default_sink: Option<String>,
    server: String,
}

unsafe extern "C" fn on_sink(_: *mut c_void, info: *const SinkInfo, eol: c_int, data: *mut c_void) {
    if eol != 0 || info.is_null() {
        return;
    }
    let out = &mut *(data as *mut Collected);
    let i = &*info;
    out.sinks.push(Sink {
        name: text(i.name),
        description: text(i.description),
        rate: i.sample_spec.rate,
        channels: i.sample_spec.channels,
    });
}

unsafe extern "C" fn on_server(_: *mut c_void, info: *const ServerInfo, data: *mut c_void) {
    if info.is_null() {
        return;
    }
    let out = &mut *(data as *mut Collected);
    let i = &*info;
    out.default_sink = Some(text(i.default_sink_name));
    out.server = format!("{} {}", text(i.server_name), text(i.server_version));
}

/// Connects to the sound server and lists its sinks, the default sink first.
fn query() -> Result<Collected> {
    let a = api()?;
    let mut out = Collected::default();
    // SAFETY: standard libpulse client sequence on a private main loop; every object is
    // freed before returning and `out` outlives the operations that write to it.
    unsafe {
        let ml = (a.mainloop_new)();
        if ml.is_null() {
            bail!("cannot create a PulseAudio main loop");
        }
        let ctx = (a.context_new)((a.mainloop_get_api)(ml), c"Shirabe".as_ptr());
        let result = (|| -> Result<()> {
            if ctx.is_null() {
                bail!("cannot create a PulseAudio context");
            }
            if (a.context_connect)(ctx, null(), PA_CONTEXT_NOAUTOSPAWN, null()) < 0 {
                bail!("no PulseAudio / PipeWire sound server is running");
            }
            let deadline = Instant::now() + TIMEOUT;
            loop {
                match (a.context_get_state)(ctx) {
                    PA_CONTEXT_READY => break,
                    PA_CONTEXT_FAILED | PA_CONTEXT_TERMINATED => {
                        bail!("cannot connect to the sound server")
                    }
                    _ => {}
                }
                if Instant::now() > deadline {
                    bail!("the sound server does not answer");
                }
                (a.mainloop_iterate)(ml, 0, null_mut());
                std::thread::sleep(Duration::from_millis(1));
            }
            let data = &mut out as *mut Collected as *mut c_void;
            let ops = [
                (a.get_server_info)(ctx, on_server, data),
                (a.get_sink_info_list)(ctx, on_sink, data),
            ];
            let mut timed_out = false;
            while ops
                .iter()
                .any(|&op| !op.is_null() && (a.operation_get_state)(op) == PA_OPERATION_RUNNING)
            {
                if Instant::now() > deadline {
                    timed_out = true;
                    break;
                }
                (a.mainloop_iterate)(ml, 0, null_mut());
                std::thread::sleep(Duration::from_millis(1));
            }
            for op in ops.into_iter().filter(|op| !op.is_null()) {
                (a.operation_unref)(op);
            }
            if timed_out {
                bail!("the sound server does not answer");
            }
            Ok(())
        })();
        if !ctx.is_null() {
            (a.context_disconnect)(ctx);
            (a.context_unref)(ctx);
        }
        (a.mainloop_free)(ml);
        result?;
    }
    if let Some(d) = out.default_sink.clone() {
        if let Some(i) = out.sinks.iter().position(|s| s.name == d) {
            let s = out.sinks.remove(i);
            out.sinks.insert(0, s);
        }
    }
    Ok(out)
}

/// Sinks of the sound server, the default sink first.
pub fn sinks() -> Result<Vec<Sink>> {
    Ok(query()?.sinks)
}

/// The sink called `name`.
pub fn sink(name: &str) -> Result<Sink> {
    query()?
        .sinks
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow!("sound server output '{name}' not found"))
}

struct Simple(*mut c_void);
// SAFETY: a pa_simple object may be used from any one thread at a time; it is moved into the
// capture thread and used only there.
unsafe impl Send for Simple {}

/// Records the monitor of `sink` and feeds it to `feed` until `stop` is set.
pub fn start(
    sink: &Sink,
    feed: Arc<Feed>,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<Capture> {
    let a = api()?;
    let channels = usize::from(sink.channels.max(1));
    let spec = SampleSpec {
        format: PA_SAMPLE_FLOAT32LE,
        rate: sink.rate,
        channels: channels as u8,
    };
    // About 10 ms per read keeps the meters responsive.
    let frames = (sink.rate as usize / 100).max(1);
    let bytes = frames * channels * std::mem::size_of::<f32>();
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: bytes as u32,
    };
    let device = CString::new(format!("{}.monitor", sink.name))?;
    let mut code: c_int = 0;
    // SAFETY: all pointers are valid for the duration of the call.
    let simple = unsafe {
        (a.simple_new)(
            null(),
            c"Shirabe".as_ptr(),
            PA_STREAM_RECORD,
            device.as_ptr(),
            c"Meter".as_ptr(),
            &spec,
            null(),
            &attr,
            &mut code,
        )
    };
    if simple.is_null() {
        bail!(
            "cannot record '{}': {}",
            sink.description,
            a.error_text(code)
        );
    }
    let simple = Simple(simple);
    let thread = std::thread::Builder::new()
        .name("shirabe-pulse".into())
        .spawn(move || {
            let simple = simple;
            let mut buffer = vec![0.0f32; frames * channels];
            while !stop.load(Ordering::Relaxed) {
                let mut code: c_int = 0;
                // SAFETY: `buffer` holds `bytes` bytes.
                let r = unsafe {
                    (a.simple_read)(simple.0, buffer.as_mut_ptr().cast(), bytes, &mut code)
                };
                if r < 0 {
                    *error.lock().unwrap_or_else(|e| e.into_inner()) = Some(a.error_text(code));
                    break;
                }
                feed.with(|pump| pump.interleaved(&buffer, channels));
            }
            // SAFETY: created by pa_simple_new and not used afterwards.
            unsafe { (a.simple_free)(simple.0) };
        })?;
    let server = query().map(|c| c.server).unwrap_or_default();
    Ok(Capture {
        keep: Box::new(()),
        thread: Some(thread),
        description: format!(
            "Playing on {}  ·  {}  ·  f32  ·  {server}",
            sink.description,
            format_rate_channels(sink.rate as f32, channels)
        ),
    })
}

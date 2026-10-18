//! `CoreAudio` properties are data/policy and therefore belong in Rust, not the
//! C vtable shim. `FourCC` values here are the stable Apple SDK ABI, not a second
//! product format catalog. Native ABI tests compare payloads with SDK structs.
use mixrelay_pcm_ring::SampleFormat;
use std::ffi::{c_char, c_void};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::abi::Snapshot;
use crate::registry::Registry;

#[repr(C)]
pub(crate) struct Address {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

#[repr(C)]
pub(crate) struct Notification {
    object: u32,
    address: Address,
}

/// Writes the property addresses changed by one control-plane event, in host order.
/// Returns the count fitting `capacity`; a null output only queries that count.
///
/// # Safety
/// A non-null output must point to `capacity` aligned writable notifications and
/// remain exclusively borrowed for this call. No output storage is retained.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_notifications(
    object: u32,
    change: u32,
    output: *mut Notification,
    capacity: usize,
) -> usize {
    use crate::abi::{
        ALIVE, FORMAT, INPUT_LATENCY, LIST, NAME, OUTPUT_MUTE, OUTPUT_VOLUME, RUNNING,
        STREAM_ACTIVE, TRANSPORT,
    };
    let mut count = 0;
    let mut push = |object, selector, scope| {
        if count == capacity {
            return;
        }
        if !output.is_null() {
            // SAFETY: count is below the caller's writable capacity; each index
            // is initialized once and no reference to the ABI output is retained.
            unsafe {
                output.add(count).write(Notification {
                    object,
                    address: Address {
                        selector: u32::from_be_bytes(selector),
                        scope,
                        element: 0,
                    },
                });
            }
        }
        count += 1;
    };
    match change {
        LIST => {
            push(1, *b"dev#", GLOBAL);
            push(1, *b"ownd", GLOBAL);
        }
        NAME => {
            for id in [object, object + 1, object + 2] {
                push(id, *b"lnam", GLOBAL);
            }
        }
        FORMAT => {
            push(object, *b"nsrt", GLOBAL);
            push(object, *b"ownd", GLOBAL);
            push(object, *b"ctrl", GLOBAL);
            push(object, *b"ctrl", OUTPUT);
            for scope in [INPUT, OUTPUT] {
                push(object, *b"stm#", scope);
                push(object, *b"srnd", scope);
            }
            for stream in [object + 1, object + 2] {
                for selector in [*b"sfmt", *b"pft ", *b"sfma", *b"pfta"] {
                    push(stream, selector, GLOBAL);
                }
            }
        }
        INPUT_LATENCY => push(object, *b"ltnc", INPUT),
        TRANSPORT => push(object, *b"tran", GLOBAL),
        ALIVE => push(object, *b"livn", GLOBAL),
        RUNNING => push(object, *b"goin", GLOBAL),
        STREAM_ACTIVE => push(object, *b"sact", GLOBAL),
        OUTPUT_VOLUME => {
            push(object + 3, *b"lcsv", GLOBAL);
            push(object + 3, *b"lcdv", GLOBAL);
        }
        OUTPUT_MUTE => push(object + 4, *b"bcvl", GLOBAL),
        _ => {}
    }
    count
}

const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const INPUT: u32 = u32::from_be_bytes(*b"inpt");
const OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const BAD_OBJECT: i32 = i32::from_be_bytes(*b"!obj");
const UNKNOWN: i32 = i32::from_be_bytes(*b"who?");
const BAD_SIZE: i32 = i32::from_be_bytes(*b"!siz");
const ILLEGAL: i32 = i32::from_be_bytes(*b"nope");
const UNSUPPORTED_FORMAT: i32 = i32::from_be_bytes(*b"!dat");
const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        count: isize,
        encoding: u32,
        external: u8,
    ) -> *const c_void;
    fn CFStringGetCString(
        string: *const c_void,
        output: *mut c_char,
        capacity: isize,
        encoding: u32,
    ) -> u8;
    fn CFGetTypeID(object: *const c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
}

/// Largest fixed payload stored inline: an `AudioValueRange` (two `Float64`).
const INLINE_BYTES: usize = 16;

/// A fixed payload of at most `INLINE_BYTES`, stored without allocation. Scalars use it,
/// so the answers given on IO threads (see `property`) never allocate.
#[derive(Clone, Copy)]
struct Inline {
    bytes: [u8; INLINE_BYTES],
    length: usize,
}
impl Inline {
    fn new<const N: usize>(value: [u8; N]) -> Self {
        const { assert!(N <= INLINE_BYTES, "inline payload exceeds INLINE_BYTES") };
        let mut bytes = [0; INLINE_BYTES];
        let (head, _) = bytes.split_at_mut(N);
        head.copy_from_slice(&value);
        Self { bytes, length: N }
    }
    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.length).unwrap_or_default()
    }
}

enum Value {
    /// A fixed scalar or small struct; never allocates.
    Inline(Inline),
    /// A fixed struct too large to store inline (ASBD, channel layout).
    Struct(Vec<u8>),
    Array(Vec<u8>, usize),
    Text(Vec<u8>),
}
impl Value {
    fn size(&self) -> usize {
        match self {
            Self::Inline(inline) => inline.length,
            Self::Struct(bytes) | Self::Array(bytes, _) => bytes.len(),
            Self::Text(_) => size_of::<*const c_void>(),
        }
    }
    /// # Safety
    /// A non-null `output` must be writable for `capacity` bytes for this call.
    unsafe fn write(&self, capacity: usize, output: *mut c_void) -> Result<usize, i32> {
        match self {
            // SAFETY: forwarded from this function's contract.
            Self::Inline(inline) => unsafe { write_fixed(inline.as_bytes(), capacity, output) },
            // SAFETY: as above.
            Self::Struct(bytes) => unsafe { write_fixed(bytes, capacity, output) },
            Self::Array(bytes, stride) => {
                let size = bytes.len().min(capacity / stride * stride);
                if size > 0 {
                    // SAFETY: size is at most capacity, which the caller made writable.
                    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), size) };
                }
                Ok(size)
            }
            Self::Text(bytes) => {
                if capacity < size_of::<*const c_void>() {
                    return Err(BAD_SIZE);
                }
                let count = isize::try_from(bytes.len()).map_err(|_| BAD_SIZE)?;
                // SAFETY: bytes is a live slice of exactly count bytes.
                let string = unsafe {
                    CFStringCreateWithBytes(std::ptr::null(), bytes.as_ptr(), count, UTF8, 0)
                };
                if string.is_null() {
                    return Err(ILLEGAL);
                }
                // SAFETY: capacity holds one pointer; the SDK output need not be aligned.
                unsafe { output.cast::<*const c_void>().write_unaligned(string) };
                Ok(size_of::<*const c_void>())
            }
        }
    }
}

/// Writes a fixed payload whole, or refuses a smaller output.
///
/// # Safety
/// A non-null `output` must be writable for `capacity` bytes for this call.
unsafe fn write_fixed(bytes: &[u8], capacity: usize, output: *mut c_void) -> Result<usize, i32> {
    if capacity < bytes.len() || output.is_null() {
        return Err(BAD_SIZE);
    }
    // SAFETY: output is non-null and writable for capacity >= bytes.len() bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), bytes.len()) };
    Ok(bytes.len())
}

/// Two adjacent fields of one fixed SDK struct, in native byte order.
fn pair<const N: usize, const M: usize>(first: [u8; N], second: [u8; N]) -> [u8; M] {
    const { assert!(M == 2 * N, "a pair holds exactly two fields") };
    let mut bytes = [0; M];
    let (head, tail) = bytes.split_at_mut(N);
    head.copy_from_slice(&first);
    tail.copy_from_slice(&second);
    bytes
}

fn number(value: u32) -> Value {
    Value::Inline(Inline::new(value.to_ne_bytes()))
}
fn code(value: [u8; 4]) -> Value {
    number(u32::from_be_bytes(value))
}
fn text(value: &[u8]) -> Value {
    Value::Text(
        value
            .split(|byte| *byte == 0)
            .next()
            .unwrap_or_default()
            .to_vec(),
    )
}
fn objects(values: impl IntoIterator<Item = u32>) -> Value {
    Value::Array(values.into_iter().flat_map(u32::to_ne_bytes).collect(), 4)
}
fn rate(value: u32) -> Value {
    Value::Inline(Inline::new(f64::from(value).to_ne_bytes()))
}

fn scalar(value: f32) -> Value {
    Value::Inline(Inline::new(value.to_ne_bytes()))
}

fn stream_ids(snapshot: &Snapshot, scope: u32) -> Vec<u32> {
    let mut result = Vec::with_capacity(2);
    if scope != OUTPUT && snapshot.capture_channels > 0 {
        result.push(snapshot.input_stream_id);
    }
    if scope != INPUT && snapshot.render_channels > 0 {
        result.push(snapshot.output_stream_id);
    }
    result
}

fn control_ids(snapshot: &Snapshot, scope: u32) -> Vec<u32> {
    if scope != INPUT && snapshot.render_channels != 0 {
        vec![snapshot.device_id + 3, snapshot.device_id + 4]
    } else {
        Vec::new()
    }
}

fn plugin(registry: &Registry, address: &Address, uid: Option<&str>) -> Result<Value, i32> {
    if address.scope != GLOBAL || address.element != 0 {
        return Err(UNKNOWN);
    }
    match &address.selector.to_be_bytes() {
        // Text, object lists and the UID lookup allocate: never built on an IO thread.
        b"lnam" | b"lmak" | b"rsrc" | b"ownd" | b"dev#" | b"box#" | b"clk#" | b"uidd"
            if crate::transport::on_audio_thread() =>
        {
            Err(ILLEGAL)
        }
        b"bcls" => Ok(code(*b"aobj")),
        b"clas" => Ok(code(*b"aplg")),
        b"stdv" => Ok(number(0)),
        b"lnam" | b"lmak" => Ok(text(b"MixRelay")),
        b"rsrc" => Ok(text(b"")),
        b"ownd" | b"dev#" => Ok(objects(registry.devices())),
        b"box#" | b"clk#" => Ok(objects([])),
        b"uidd" => Ok(number(
            uid.and_then(|uid| registry.lookup_uid(uid)).unwrap_or(0),
        )),
        _ => Err(UNKNOWN),
    }
}

fn common(snapshot: &Snapshot, object: u32, selector: u32) -> Option<Value> {
    let device = object == snapshot.device_id;
    match &selector.to_be_bytes() {
        b"bcls" => Some(code(*b"aobj")),
        b"clas" => Some(code(if device { *b"adev" } else { *b"astr" })),
        b"stdv" => Some(number(if device { 1 } else { snapshot.device_id })),
        b"lnam" => Some(text(&snapshot.name)),
        b"lmak" => Some(text(b"MixRelay")),
        _ => None,
    }
}

fn ranges() -> Value {
    Value::Array(
        crate::formats::SAMPLE_RATES
            .iter()
            .flat_map(|rate| {
                let bytes = f64::from(*rate).to_ne_bytes();
                bytes.into_iter().chain(bytes)
            })
            .collect(),
        16,
    )
}

fn device(snapshot: &Snapshot, address: &Address) -> Result<Value, i32> {
    let directional = address.scope == INPUT || address.scope == OUTPUT;
    let channels = if address.scope == INPUT {
        snapshot.capture_channels
    } else {
        snapshot.render_channels
    };
    match &address.selector.to_be_bytes() {
        b"ownd" => Ok(objects(
            stream_ids(snapshot, address.scope)
                .into_iter()
                .chain(control_ids(snapshot, address.scope)),
        )),
        b"stm#" => Ok(objects(stream_ids(snapshot, address.scope))),
        b"ctrl" => Ok(objects(control_ids(snapshot, address.scope))),
        b"uid " => Ok(text(&snapshot.uid)),
        b"muid" => Ok(text(b"io.mixrelay.audio.endpoint")),
        b"akin" => Ok(objects([snapshot.device_id])),
        b"clkd" | b"hidn" => Ok(number(0)),
        b"livn" => Ok(number(u32::from(snapshot.alive))),
        b"goin" => Ok(number(u32::from(snapshot.running))),
        b"dflt" | b"sflt" if directional => Ok(number(u32::from(channels > 0))),
        b"ltnc" | b"saft" if directional => Ok(number(0)),
        b"nsrt" if address.scope == GLOBAL => Ok(rate(snapshot.sample_rate)),
        b"nsr#" if address.scope == GLOBAL => Ok(ranges()),
        b"ring" => Ok(number(crate::clock::TIMESTAMP_PERIOD)),
        b"cstb" => Ok(number(1)),
        b"dch2" if directional && channels >= 2 => Ok(Value::Inline(Inline::new(pair::<4, 8>(
            1_u32.to_ne_bytes(),
            2_u32.to_ne_bytes(),
        )))),
        b"srnd" if directional => Ok(channel_layout(channels)),
        _ => Err(UNKNOWN),
    }
}

fn channel_layout(channels: u32) -> Value {
    // Use explicit discrete labels. There is no fabricated speaker assignment
    // for a 7/13/33/64-channel virtual patchbay endpoint.
    let mut bytes: Vec<u8> = [0_u32, 0, channels]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    for index in 0..channels {
        for field in [65_536 + index, 0, 0, 0, 0] {
            bytes.extend_from_slice(&field.to_ne_bytes());
        }
    }
    Value::Struct(bytes)
}

pub(crate) const NATIVE_FORMATS: [SampleFormat; 4] = [
    SampleFormat::Float32,
    SampleFormat::Signed16,
    SampleFormat::Signed24,
    SampleFormat::Signed32,
];

/// Builds the exact SDK ASBD for one interleaved little-endian native stream.
#[expect(
    clippy::cast_possible_truncation,
    reason = "SampleFormat samples occupy at most four bytes"
)]
pub(crate) fn format(rate: u32, channels: u32, encoding: SampleFormat) -> Vec<u8> {
    let sample_bytes = encoding.bytes_per_sample() as u32;
    let mut bytes = f64::from(rate).to_ne_bytes().to_vec();
    for field in [
        u32::from_be_bytes(*b"lpcm"),
        if encoding == SampleFormat::Float32 {
            9
        } else {
            12
        },
        channels * sample_bytes,
        1,
        channels * sample_bytes,
        channels,
        sample_bytes * 8,
        0,
    ] {
        bytes.extend_from_slice(&field.to_ne_bytes());
    }
    bytes
}

fn stream(snapshot: &Snapshot, object: u32, address: &Address) -> Result<Value, i32> {
    if address.scope != GLOBAL {
        return Err(UNKNOWN);
    }
    let input = object == snapshot.input_stream_id;
    let channels = if input {
        snapshot.capture_channels
    } else {
        snapshot.render_channels
    };
    if channels == 0 {
        return Err(BAD_OBJECT);
    }
    match &address.selector.to_be_bytes() {
        b"ownd" => Ok(objects([])),
        b"sact" => Ok(number(u32::from(if input {
            snapshot.input_active
        } else {
            snapshot.output_active
        }))),
        b"sdir" => Ok(number(u32::from(input))),
        b"term" => Ok(code(*b"line")),
        b"schn" => Ok(number(1)),
        b"ltnc" => Ok(number(0)),
        b"sfmt" | b"pft " => {
            let code = if input {
                snapshot.capture_format
            } else {
                snapshot.render_format
            };
            let encoding = NATIVE_FORMATS
                .into_iter()
                .find(|encoding| *encoding as u32 == code)
                .ok_or(BAD_OBJECT)?;
            Ok(Value::Struct(format(
                snapshot.sample_rate,
                channels,
                encoding,
            )))
        }
        b"sfma" | b"pfta" => {
            let mut bytes = Vec::new();
            for &rate in crate::formats::SAMPLE_RATES {
                for encoding in NATIVE_FORMATS {
                    bytes.extend(format(rate, channels, encoding));
                    bytes.extend(f64::from(rate).to_ne_bytes());
                    bytes.extend(f64::from(rate).to_ne_bytes());
                }
            }
            Ok(Value::Array(bytes, 56))
        }
        _ => Err(UNKNOWN),
    }
}

fn output_control(
    registry: &Registry,
    snapshot: &Snapshot,
    object: u32,
    address: &Address,
) -> Result<Value, i32> {
    if snapshot.render_channels == 0 {
        return Err(BAD_OBJECT);
    }
    if address.scope != GLOBAL {
        return Err(UNKNOWN);
    }
    let volume = object == snapshot.device_id + 3;
    let (gain, muted) = registry.output_control(object).ok_or(BAD_OBJECT)?;
    match &address.selector.to_be_bytes() {
        b"bcls" => Ok(code(if volume { *b"levl" } else { *b"togl" })),
        b"clas" => Ok(code(if volume { *b"vlme" } else { *b"mute" })),
        b"stdv" => Ok(number(snapshot.device_id)),
        b"lnam" => Ok(text(if volume {
            b"Output volume"
        } else {
            b"Output mute"
        })),
        b"ownd" => Ok(objects([])),
        b"cscp" => Ok(number(OUTPUT)),
        b"celm" => Ok(number(0)),
        b"lcsv" if volume => Ok(scalar(gain)),
        b"lcdv" if volume => Ok(scalar(
            crate::output_control::scalar_to_decibels(gain).ok_or(ILLEGAL)?,
        )),
        b"lcdr" if volume => Ok(Value::Inline(Inline::new(pair::<8, 16>(
            f64::from(crate::output_control::MIN_DECIBELS).to_ne_bytes(),
            0.0_f64.to_ne_bytes(),
        )))),
        b"lcsd" | b"lcds" if volume => Ok(scalar(0.0)),
        b"bcvl" if !volume => Ok(number(u32::from(muted))),
        _ => Err(UNKNOWN),
    }
}

fn property(
    registry: &Registry,
    object: u32,
    address: &Address,
    uid: Option<&str>,
) -> Result<Value, i32> {
    // IO/timestamp threads may reenter property reads. They are answered only inline values
    // that need no control lock: the plugin's class/owner scalars and the device properties
    // below that read fixed-slot atomics (`ltnc`, `saft`, `tran`).
    if object == 1 {
        return plugin(registry, address, uid);
    }
    if address.element != 0 || ![GLOBAL, INPUT, OUTPUT].contains(&address.scope) {
        return Err(UNKNOWN);
    }
    if [*b"ltnc", *b"saft"].contains(&address.selector.to_be_bytes()) {
        let device = registry.pin(object).ok_or(BAD_OBJECT)?;
        if object == device.id {
            if ![INPUT, OUTPUT].contains(&address.scope) {
                return Err(UNKNOWN);
            }
            return Ok(number(
                if address.scope == INPUT && address.selector == u32::from_be_bytes(*b"ltnc") {
                    device
                        .input_latency
                        .effective(device.rate.load(Ordering::Acquire))
                } else {
                    0
                },
            ));
        }
        if address.selector != u32::from_be_bytes(*b"ltnc")
            || object > device.id + 2
            || address.scope != GLOBAL
        {
            return Err(UNKNOWN);
        }
        let channels = if object == device.id + 1 {
            &device.capture_channels
        } else {
            &device.render_channels
        };
        if channels.load(Ordering::Acquire) == 0 {
            return Err(BAD_OBJECT);
        }
        return Ok(number(0));
    }
    if address.selector == u32::from_be_bytes(*b"tran") {
        // Reentrant IO/timestamp reads must not take control: StopIO holds
        // that mutex while freeze waits for this callback's IO pin to drain.
        // Fixed-slot pins and channel atomics preserve the property contract
        // without snapshot or any registry/control-plane mutex, for all callers.
        let device = registry.pin(object).ok_or(BAD_OBJECT)?;
        if (object == device.id + 1 && device.capture_channels.load(Ordering::Acquire) == 0)
            || (object >= device.id + 2 && device.render_channels.load(Ordering::Acquire) == 0)
        {
            return Err(BAD_OBJECT);
        }
        return if object == device.id {
            Ok(number(crate::transport::VIRTUAL))
        } else {
            Err(UNKNOWN)
        };
    }
    // Every other answer reads the control snapshot. An IO/timestamp thread must not wait
    // for control (StopIO holds it while draining that thread's pin), so it is refused.
    if crate::transport::on_audio_thread() {
        return Err(ILLEGAL);
    }
    let snapshot = registry.snapshot(object).ok_or(BAD_OBJECT)?;
    if (object == snapshot.input_stream_id && snapshot.capture_channels == 0)
        || (object == snapshot.output_stream_id && snapshot.render_channels == 0)
    {
        return Err(BAD_OBJECT);
    }
    if object == snapshot.device_id + 3 || object == snapshot.device_id + 4 {
        return output_control(registry, &snapshot, object, address);
    }
    if let Some(value) = common(&snapshot, object, address.selector) {
        return Ok(value);
    }
    if object == snapshot.device_id {
        device(&snapshot, address)
    } else {
        stream(&snapshot, object, address)
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_has_property(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
) -> bool {
    match (unsafe { registry.as_ref() }, unsafe { address.as_ref() }) {
        (Some(registry), Some(address)) => property(registry, object, address, None).is_ok(),
        _ => false,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_property_size(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
    output: *mut u32,
) -> i32 {
    let (Some(registry), Some(address), Some(output)) = (
        unsafe { registry.as_ref() },
        unsafe { address.as_ref() },
        unsafe { output.as_mut() },
    ) else {
        return ILLEGAL;
    };
    match property(registry, object, address, None) {
        Ok(value) => match u32::try_from(value.size()) {
            Ok(size) => {
                *output = size;
                0
            }
            Err(_) => BAD_SIZE,
        },
        Err(error) => error,
    }
}

unsafe fn qualifier_string(size: u32, qualifier: *const c_void) -> Result<Option<String>, i32> {
    if size as usize != size_of::<*const c_void>() || qualifier.is_null() {
        return Err(BAD_SIZE);
    }
    let string = unsafe { qualifier.cast::<*const c_void>().read_unaligned() };
    if string.is_null() || unsafe { CFGetTypeID(string) != CFStringGetTypeID() } {
        return Err(ILLEGAL);
    }
    let mut bytes = [0_u8; 128];
    if unsafe { CFStringGetCString(string, bytes.as_mut_ptr().cast(), 128, UTF8) } == 0 {
        return Err(ILLEGAL);
    }
    let length = bytes.iter().position(|byte| *byte == 0).ok_or(ILLEGAL)?;
    std::str::from_utf8(&bytes[..length])
        .map(|value| Some(value.into()))
        .map_err(|_| ILLEGAL)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_property_get(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
    qualifier_size: u32,
    qualifier: *const c_void,
    capacity: u32,
    written: *mut u32,
    output: *mut c_void,
    requester_pid: i32,
) -> i32 {
    let deadline = Instant::now() + crate::transport::QUERY_BUDGET;
    let (Some(registry), Some(address), Some(written)) = (
        unsafe { registry.as_ref() },
        unsafe { address.as_ref() },
        unsafe { written.as_mut() },
    ) else {
        return ILLEGAL;
    };
    if output.is_null() && capacity != 0 {
        return ILLEGAL;
    }
    // `uidd` is never answered on an IO thread (see `plugin`); skip its allocating parse.
    let uid = if address.selector == u32::from_be_bytes(*b"uidd")
        && !crate::transport::on_audio_thread()
    {
        match unsafe { qualifier_string(qualifier_size, qualifier) } {
            Ok(uid) => uid,
            Err(error) => return error,
        }
    } else {
        None
    };
    match property(registry, object, address, uid.as_deref())
        .map(|value| {
            if address.selector == u32::from_be_bytes(*b"tran") {
                number(registry.transport.get_until(requester_pid, deadline))
            } else if address.selector == u32::from_be_bytes(*b"ltnc")
                && address.scope == INPUT
                && registry.transport.is_coreaudio(requester_pid)
            {
                // Apps read the measured frames; coreaudiod reads only a change marker.
                registry
                    .pin(object)
                    .filter(|device| device.id == object)
                    .map_or(value, |device| {
                        number(device.input_latency.coreaudio_marker())
                    })
            } else {
                value
            }
        })
        .and_then(|value| unsafe { convert_level(value, address, capacity, output) })
        .and_then(|value| unsafe { value.write(capacity as usize, output) })
    {
        Ok(size) => match u32::try_from(size) {
            Ok(size) => {
                *written = size;
                0
            }
            Err(_) => BAD_SIZE,
        },
        Err(error) => error,
    }
}

unsafe fn convert_level(
    value: Value,
    address: &Address,
    capacity: u32,
    output: *mut c_void,
) -> Result<Value, i32> {
    let convert = match &address.selector.to_be_bytes() {
        b"lcsd" => crate::output_control::scalar_to_decibels,
        b"lcds" => crate::output_control::decibels_to_scalar,
        _ => return Ok(value),
    };
    if capacity < 4 || output.is_null() {
        return Err(BAD_SIZE);
    }
    // SAFETY: CoreAudio defines these Get properties as an in/out Float32.
    // The caller supplies at least four readable/writable bytes for this call.
    let input = unsafe { output.cast::<f32>().read_unaligned() };
    convert(input).map(scalar).ok_or(ILLEGAL)
}

fn settable(registry: &Registry, object: u32, address: &Address) -> Result<bool, i32> {
    property(registry, object, address, None)?;
    Ok(matches!(
        &address.selector.to_be_bytes(),
        b"nsrt" | b"sact" | b"sfmt" | b"pft " | b"lcsv" | b"lcdv" | b"bcvl"
    ))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_property_settable(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
    output: *mut bool,
) -> i32 {
    let (Some(registry), Some(address), Some(output)) = (
        unsafe { registry.as_ref() },
        unsafe { address.as_ref() },
        unsafe { output.as_mut() },
    ) else {
        return ILLEGAL;
    };
    match settable(registry, object, address) {
        Ok(value) => {
            *output = value;
            0
        }
        Err(error) => error,
    }
}

fn parse_rate(bytes: &[u8]) -> Option<u32> {
    let value = f64::from_ne_bytes(bytes.get(..8)?.try_into().ok()?);
    crate::formats::SAMPLE_RATES
        .iter()
        .copied()
        .find(|rate| f64::from(*rate).to_bits() == value.to_bits())
}

fn set_output_control(registry: &Registry, object: u32, selector: u32, bytes: &[u8]) -> i32 {
    let Ok(bytes) = <[u8; 4]>::try_from(bytes) else {
        return BAD_SIZE;
    };
    if selector == u32::from_be_bytes(*b"bcvl") {
        return registry.set_output_mute(object, u32::from_ne_bytes(bytes) != 0);
    }
    let value = f32::from_ne_bytes(bytes);
    let gain = if selector == u32::from_be_bytes(*b"lcdv") {
        let Some(gain) = crate::output_control::decibels_to_scalar(value) else {
            return ILLEGAL;
        };
        gain
    } else {
        value
    };
    registry.set_output_volume(object, gain)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_hal_core_property_set(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
    size: u32,
    input: *const c_void,
) -> i32 {
    let (Some(registry), Some(address)) =
        (unsafe { registry.as_ref() }, unsafe { address.as_ref() })
    else {
        return ILLEGAL;
    };
    if input.is_null() {
        return ILLEGAL;
    }
    match settable(registry, object, address) {
        Ok(true) => {}
        Ok(false) => return ILLEGAL,
        Err(error) => return error,
    }
    let bytes = unsafe { std::slice::from_raw_parts(input.cast::<u8>(), size as usize) };
    let Some(snapshot) = registry.snapshot(object) else {
        return BAD_OBJECT;
    };
    let status = match &address.selector.to_be_bytes() {
        b"lcsv" | b"lcdv" | b"bcvl" => {
            set_output_control(registry, object, address.selector, bytes)
        }
        b"sact" => {
            let Ok(bytes) = bytes.try_into() else {
                return BAD_SIZE;
            };
            registry.set_stream_active(object, u32::from_ne_bytes(bytes) != 0)
        }
        b"nsrt" => {
            if bytes.len() != 8 {
                return BAD_SIZE;
            }
            let Some(rate) = parse_rate(bytes) else {
                return UNSUPPORTED_FORMAT;
            };
            registry.set_rate(snapshot.device_id, rate)
        }
        b"sfmt" | b"pft " => {
            if bytes.len() != 40 {
                return BAD_SIZE;
            }
            let Some(rate) = parse_rate(bytes) else {
                return UNSUPPORTED_FORMAT;
            };
            let channels = if object == snapshot.input_stream_id {
                snapshot.capture_channels
            } else {
                snapshot.render_channels
            };
            let Some(encoding) = NATIVE_FORMATS
                .into_iter()
                .find(|encoding| bytes == format(rate, channels, *encoding))
            else {
                return UNSUPPORTED_FORMAT;
            };
            registry.set_format(object, rate, encoding)
        }
        _ => return UNKNOWN,
    };
    if status == 0 { 0 } else { ILLEGAL }
}

#[cfg(test)]
mod notification_tests {
    use super::*;
    use crate::abi::{FORMAT, LIST, NAME};

    const OBJECT: u32 = 20;
    const SENTINEL: (u32, u32, u32, u32) = (u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    type NotificationPayload = (usize, [(u32, u32, u32, u32); 16]);

    fn payload(change: u32, capacity: usize) -> NotificationPayload {
        let mut output: [Notification; 16] = std::array::from_fn(|_| Notification {
            object: SENTINEL.0,
            address: Address {
                selector: SENTINEL.1,
                scope: SENTINEL.2,
                element: SENTINEL.3,
            },
        });
        assert!(capacity <= output.len());
        // SAFETY: this initialized ABI array is aligned, exclusive and large enough.
        let count =
            unsafe { mr_hal_core_notifications(OBJECT, change, output.as_mut_ptr(), capacity) };
        (
            count,
            output.map(|value| {
                (
                    value.object,
                    value.address.selector,
                    value.address.scope,
                    value.address.element,
                )
            }),
        )
    }

    #[test]
    fn format_notifications_preserve_host_order_and_partial_output_bounds() {
        let expected = [
            (OBJECT, *b"nsrt", GLOBAL),
            (OBJECT, *b"ownd", GLOBAL),
            (OBJECT, *b"ctrl", GLOBAL),
            (OBJECT, *b"ctrl", OUTPUT),
            (OBJECT, *b"stm#", INPUT),
            (OBJECT, *b"srnd", INPUT),
            (OBJECT, *b"stm#", OUTPUT),
            (OBJECT, *b"srnd", OUTPUT),
            (OBJECT + 1, *b"sfmt", GLOBAL),
            (OBJECT + 1, *b"pft ", GLOBAL),
            (OBJECT + 1, *b"sfma", GLOBAL),
            (OBJECT + 1, *b"pfta", GLOBAL),
            (OBJECT + 2, *b"sfmt", GLOBAL),
            (OBJECT + 2, *b"pft ", GLOBAL),
            (OBJECT + 2, *b"sfma", GLOBAL),
            (OBJECT + 2, *b"pfta", GLOBAL),
        ]
        .map(|(object, selector, scope)| (object, u32::from_be_bytes(selector), scope, 0));
        for capacity in [0, 5, 16] {
            let (count, output) = payload(FORMAT, capacity);
            assert_eq!(count, capacity.min(expected.len()));
            assert_eq!(output[..count], expected[..count]);
            assert!(output[count..].iter().all(|value| *value == SENTINEL));
            // SAFETY: null output queries the fitting count and is never dereferenced.
            assert_eq!(
                unsafe {
                    mr_hal_core_notifications(OBJECT, FORMAT, std::ptr::null_mut(), capacity)
                },
                count
            );
        }
    }

    #[test]
    fn inventory_and_rename_notifications_target_owned_objects() {
        let (count, inventory) = payload(LIST, 16);
        assert_eq!(count, 2);
        assert_eq!(
            inventory[..count],
            [
                (1, u32::from_be_bytes(*b"dev#"), GLOBAL, 0),
                (1, u32::from_be_bytes(*b"ownd"), GLOBAL, 0),
            ]
        );
        let (count, rename) = payload(NAME, 16);
        assert_eq!(count, 3);
        assert_eq!(
            rename[..count],
            [OBJECT, OBJECT + 1, OBJECT + 2].map(|object| (
                object,
                u32::from_be_bytes(*b"lnam"),
                GLOBAL,
                0
            ))
        );
        let (count, unsupported) = payload(u32::MAX, 16);
        assert_eq!(count, 0);
        assert_eq!(unsupported, [SENTINEL; 16]);
    }

    #[test]
    fn transport_notifications_target_the_device_global_property() {
        let (count, output) = payload(crate::abi::TRANSPORT, 16);
        assert_eq!(count, 1);
        assert_eq!(output[0], (OBJECT, u32::from_be_bytes(*b"tran"), GLOBAL, 0));
        assert!(output[1..].iter().all(|value| *value == SENTINEL));
    }

    #[test]
    fn native_output_notifications_target_level_and_mute_control_objects() {
        let (count, volume) = payload(crate::abi::OUTPUT_VOLUME, 16);
        assert_eq!(count, 2);
        assert_eq!(
            volume[..count],
            [*b"lcsv", *b"lcdv"].map(|selector| (
                OBJECT + 3,
                u32::from_be_bytes(selector),
                GLOBAL,
                0
            ))
        );
        let (count, mute) = payload(crate::abi::OUTPUT_MUTE, 16);
        assert_eq!(count, 1);
        assert_eq!(
            mute[0],
            (OBJECT + 4, u32::from_be_bytes(*b"bcvl"), GLOBAL, 0)
        );
    }
}

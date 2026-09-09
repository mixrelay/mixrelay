//! `CoreAudio` properties are data/policy and therefore belong in Rust, not the
//! C vtable shim. `FourCC` values here are the stable Apple SDK ABI, not a second
//! product format catalog. Native ABI tests compare payloads with SDK structs.
use std::ffi::{c_char, c_void};

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

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn tn_hal_core_notifications(
    object: u32,
    change: u32,
    output: *mut Notification,
    capacity: usize,
) -> usize {
    use crate::abi::{ALIVE, FORMAT, LIST, NAME, RUNNING, STREAM_ACTIVE};
    let mut values = Vec::new();
    let mut push = |object, selector, scope| {
        values.push(Notification {
            object,
            address: Address {
                selector: u32::from_be_bytes(selector),
                scope,
                element: 0,
            },
        });
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
        ALIVE => push(object, *b"livn", GLOBAL),
        RUNNING => push(object, *b"goin", GLOBAL),
        STREAM_ACTIVE => push(object, *b"sact", GLOBAL),
        _ => {}
    }
    let count = values.len().min(capacity);
    if !output.is_null() {
        for (index, value) in values.into_iter().take(count).enumerate() {
            unsafe { output.add(index).write(value) };
        }
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

enum Value {
    Scalar(Vec<u8>),
    Array(Vec<u8>, usize),
    Text(Vec<u8>),
}
impl Value {
    fn size(&self) -> usize {
        match self {
            Self::Scalar(bytes) | Self::Array(bytes, _) => bytes.len(),
            Self::Text(_) => size_of::<*const c_void>(),
        }
    }
    unsafe fn write(&self, capacity: usize, output: *mut c_void) -> Result<usize, i32> {
        match self {
            Self::Scalar(bytes) => {
                if capacity < bytes.len() {
                    return Err(BAD_SIZE);
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), bytes.len());
                };
                Ok(bytes.len())
            }
            Self::Array(bytes, stride) => {
                let size = bytes.len().min(capacity / stride * stride);
                if size > 0 {
                    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), size) };
                }
                Ok(size)
            }
            Self::Text(bytes) => {
                if capacity < size_of::<*const c_void>() {
                    return Err(BAD_SIZE);
                }
                let count = isize::try_from(bytes.len()).map_err(|_| BAD_SIZE)?;
                let string = unsafe {
                    CFStringCreateWithBytes(std::ptr::null(), bytes.as_ptr(), count, UTF8, 0)
                };
                if string.is_null() {
                    return Err(ILLEGAL);
                }
                unsafe { output.cast::<*const c_void>().write_unaligned(string) };
                Ok(size_of::<*const c_void>())
            }
        }
    }
}

fn number(value: u32) -> Value {
    Value::Scalar(value.to_ne_bytes().to_vec())
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
    Value::Scalar(f64::from(value).to_ne_bytes().to_vec())
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

fn plugin(registry: &Registry, address: &Address, uid: Option<&str>) -> Result<Value, i32> {
    if address.scope != GLOBAL || address.element != 0 {
        return Err(UNKNOWN);
    }
    match &address.selector.to_be_bytes() {
        b"bcls" => Ok(code(*b"aobj")),
        b"clas" => Ok(code(*b"aplg")),
        b"stdv" => Ok(number(0)),
        b"lnam" | b"lmak" => Ok(text(b"ToneNari")),
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
        b"lmak" => Some(text(b"ToneNari")),
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
        b"ownd" | b"stm#" => Ok(objects(stream_ids(snapshot, address.scope))),
        b"ctrl" => Ok(objects([])),
        b"uid " => Ok(text(&snapshot.uid)),
        b"muid" => Ok(text(b"io.tonenari.audio.endpoint")),
        b"tran" => Ok(code(*b"virt")),
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
        b"dch2" if directional && channels >= 2 => Ok(Value::Scalar(
            [1_u32, 2].into_iter().flat_map(u32::to_ne_bytes).collect(),
        )),
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
    Value::Scalar(bytes)
}

fn format(rate: u32, channels: u32) -> Vec<u8> {
    let mut bytes = f64::from(rate).to_ne_bytes().to_vec();
    for field in [
        u32::from_be_bytes(*b"lpcm"),
        9,
        channels * 4,
        1,
        channels * 4,
        channels,
        32,
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
        b"sfmt" | b"pft " => Ok(Value::Scalar(format(snapshot.sample_rate, channels))),
        b"sfma" | b"pfta" => {
            let mut bytes = Vec::new();
            for &rate in crate::formats::SAMPLE_RATES {
                bytes.extend(format(rate, channels));
                bytes.extend(f64::from(rate).to_ne_bytes());
                bytes.extend(f64::from(rate).to_ne_bytes());
            }
            Ok(Value::Array(bytes, 56))
        }
        _ => Err(UNKNOWN),
    }
}

fn property(
    registry: &Registry,
    object: u32,
    address: &Address,
    uid: Option<&str>,
) -> Result<Value, i32> {
    if object == 1 {
        return plugin(registry, address, uid);
    }
    if address.element != 0 || ![GLOBAL, INPUT, OUTPUT].contains(&address.scope) {
        return Err(UNKNOWN);
    }
    let snapshot = registry.snapshot(object).ok_or(BAD_OBJECT)?;
    if (object == snapshot.input_stream_id && snapshot.capture_channels == 0)
        || (object == snapshot.output_stream_id && snapshot.render_channels == 0)
    {
        return Err(BAD_OBJECT);
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
pub(crate) unsafe extern "C" fn tn_hal_core_has_property(
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
pub(crate) unsafe extern "C" fn tn_hal_core_property_size(
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
pub(crate) unsafe extern "C" fn tn_hal_core_property_get(
    registry: *mut Registry,
    object: u32,
    address: *const Address,
    qualifier_size: u32,
    qualifier: *const c_void,
    capacity: u32,
    written: *mut u32,
    output: *mut c_void,
) -> i32 {
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
    let uid = if address.selector == u32::from_be_bytes(*b"uidd") {
        match unsafe { qualifier_string(qualifier_size, qualifier) } {
            Ok(uid) => uid,
            Err(error) => return error,
        }
    } else {
        None
    };
    match property(registry, object, address, uid.as_deref())
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

fn settable(registry: &Registry, object: u32, address: &Address) -> Result<bool, i32> {
    property(registry, object, address, None)?;
    Ok(matches!(
        &address.selector.to_be_bytes(),
        b"nsrt" | b"sact" | b"sfmt" | b"pft "
    ))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn tn_hal_core_property_settable(
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

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn tn_hal_core_property_set(
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
            if bytes != format(rate, channels) {
                return UNSUPPORTED_FORMAT;
            }
            registry.set_rate(snapshot.device_id, rate)
        }
        _ => return UNKNOWN,
    };
    if status == 0 { 0 } else { ILLEGAL }
}

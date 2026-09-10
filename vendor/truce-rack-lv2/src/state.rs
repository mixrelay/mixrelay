//! `MixRelay` patch: native LV2 State interface plus input control-port values.
//!
//! State calls run on the worker control thread with DSP/editor access serialized. Lilv's
//! convenience save API discards opaque properties after a native error, and its restore API
//! returns void; call the standard LV2 interface directly so failures cannot become successful
//! parameter-only snapshots. See <https://lv2plug.in/ns/ext/state>. File/path assets and nested
//! URID containers are explicitly rejected until portable asset/container packaging is available.

use super::{Error, LV2Feature, Lv2Plugin, PortKind, Result, UridMap};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, c_void},
};

const LIMIT: usize = 4 * 1024 * 1024;
const PROPERTY_LIMIT: usize = 1024 * 1024;
const COUNT_LIMIT: usize = 4096;
const PORTABLE_POD: u32 = 3;
const STATE_INTERFACE: &[u8] = b"http://lv2plug.in/ns/ext/state#interface\0";
const ATOM: &str = "http://lv2plug.in/ns/ext/atom#";

type Store = unsafe extern "C" fn(*mut c_void, u32, *const c_void, usize, u32, u32) -> u32;
type Extension = extern "C" fn(*const u8) -> *const c_void;
type Retrieve =
    unsafe extern "C" fn(*mut c_void, u32, *mut usize, *mut u32, *mut u32) -> *const c_void;
#[repr(C)]
struct StateInterface {
    save: Option<
        unsafe extern "C" fn(*mut c_void, Store, *mut c_void, u32, *const *const LV2Feature) -> u32,
    >,
    restore: Option<
        unsafe extern "C" fn(
            *mut c_void,
            Retrieve,
            *mut c_void,
            u32,
            *const *const LV2Feature,
        ) -> u32,
    >,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    plugin_uri: String,
    byte_order: String,
    native_state: bool,
    ports: BTreeMap<String, f32>,
    properties: Vec<Property>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Property {
    key: String,
    value_type: String,
    value: PropertyValue,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum PropertyValue {
    Bytes(Vec<u8>),
    Urid(String),
}

fn error(message: impl Into<String>) -> Error {
    Error::Other(message.into())
}
fn byte_order() -> &'static str {
    if cfg!(target_endian = "little") {
        "little"
    } else {
        "big"
    }
}

fn uri(map: &UridMap, id: u32) -> Option<String> {
    map.entries.lock().ok()?.iter().find_map(|(uri, value)| {
        (*value == id)
            .then(|| uri.to_str().ok().map(str::to_owned))
            .flatten()
    })
}

fn map_uri(map: &UridMap, value: &str) -> Result<u32> {
    if value.is_empty() || value.len() > 4096 || !value.contains(':') {
        return Err(error("LV2 state contains an invalid URI"));
    }
    let value = std::ffi::CString::new(value).map_err(|_| error("LV2 state URI contains NUL"))?;
    Ok(map.map_uri(&value))
}

fn validate_type(value: &str) -> Result<()> {
    if matches!(
        value.strip_prefix(ATOM),
        Some(
            "Path"
                | "Object"
                | "Blank"
                | "Resource"
                | "Literal"
                | "Property"
                | "Tuple"
                | "Vector"
                | "Sequence"
                | "Event"
        )
    ) {
        return Err(error(format!(
            "LV2 state type requires unsupported asset/container packaging: {value}"
        )));
    }
    Ok(())
}

fn interface(plugin: &Lv2Plugin) -> Result<Option<&StateInterface>> {
    if plugin.instance.is_null() {
        return Err(error("LV2 instance is not active"));
    }
    // SAFETY: the plugin owns this live instance/descriptor throughout the serialized call.
    // lv2_raw represents the nullable extension_data C callback as non-null fn; read its field
    // through an Option function pointer to respect the ABI rather than calling a null address.
    unsafe {
        let descriptor = (*plugin.instance).lv2_descriptor;
        if descriptor.is_null() {
            return Err(error("LV2 descriptor is unavailable"));
        }
        let extension = std::ptr::addr_of!((*descriptor).extension_data)
            .cast::<Option<Extension>>()
            .read();
        Ok(extension.and_then(|get| {
            get(STATE_INTERFACE.as_ptr())
                .cast::<StateInterface>()
                .as_ref()
        }))
    }
}

struct StoreContext<'a> {
    map: &'a UridMap,
    properties: Vec<Property>,
    bytes: usize,
    failure: Option<String>,
}

unsafe extern "C" fn store(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    value_type: u32,
    flags: u32,
) -> u32 {
    // SAFETY: our synchronous native save call owns this context for the whole callback window.
    let context = unsafe { &mut *handle.cast::<StoreContext<'_>>() };
    let result = (|| {
        if flags & PORTABLE_POD != PORTABLE_POD
            || value.is_null()
            || size == 0
            || size > PROPERTY_LIMIT
            || context.bytes.saturating_add(size) > PROPERTY_LIMIT
            || context.properties.len() >= COUNT_LIMIT
        {
            return Err(error(
                "LV2 state property is nonportable, invalid, or exceeds the capture limit",
            ));
        }
        let key = uri(context.map, key).ok_or_else(|| error("LV2 state key is not mapped"))?;
        let value_type =
            uri(context.map, value_type).ok_or_else(|| error("LV2 state type is not mapped"))?;
        validate_type(&value_type)?;
        if context
            .properties
            .iter()
            .any(|property| property.key == key)
        {
            return Err(error("LV2 state key was stored twice"));
        }
        // SAFETY: the LV2 save contract guarantees a readable size-byte property until return;
        // the bound above limits every allocation and we copy before the callback returns.
        let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
        let value = if value_type == format!("{ATOM}URID") {
            let raw: [u8; 4] = bytes
                .try_into()
                .map_err(|_| error("LV2 atom:URID has invalid size"))?;
            PropertyValue::Urid(
                uri(context.map, u32::from_ne_bytes(raw))
                    .ok_or_else(|| error("LV2 state value URID is not mapped"))?,
            )
        } else {
            PropertyValue::Bytes(bytes.to_vec())
        };
        context.bytes += size;
        context.properties.push(Property {
            key,
            value_type,
            value,
        });
        Ok(())
    })();
    if let Err(failure) = result {
        context.failure = Some(failure.to_string());
        1
    } else {
        0
    }
}

fn input_ports(plugin: &Lv2Plugin) -> Result<BTreeMap<String, usize>> {
    let mut result = BTreeMap::new();
    for (index, port) in plugin.ports.iter().enumerate() {
        if !port.is_input || !matches!(port.kind, PortKind::Control { .. }) {
            continue;
        }
        // SAFETY: all nodes are borrowed from this plugin's retained Lilv world.
        let symbol = unsafe {
            let native = lilv_sys::lilv_plugin_get_port_by_index(plugin.plugin, port.index);
            if native.is_null() {
                return Err(error("LV2 control port is unavailable"));
            }
            let node = lilv_sys::lilv_port_get_symbol(plugin.plugin, native);
            if node.is_null() {
                return Err(error("LV2 control symbol is unavailable"));
            }
            let raw = lilv_sys::lilv_node_as_string(node);
            if raw.is_null() {
                return Err(error("LV2 control symbol is unavailable"));
            }
            CStr::from_ptr(raw)
                .to_str()
                .map_err(|_| error("LV2 control symbol is not UTF-8"))?
                .to_owned()
        };
        let offset = plugin.control_value_offset[index];
        if offset >= plugin.control_values.len() || result.insert(symbol, offset).is_some() {
            return Err(error("LV2 control port layout is invalid"));
        }
    }
    Ok(result)
}

pub(super) fn save(plugin: &Lv2Plugin) -> Result<Vec<u8>> {
    let interface = interface(plugin)?;
    let mut context = StoreContext {
        map: &plugin.features.urid_map,
        properties: Vec::new(),
        bytes: 0,
        failure: None,
    };
    if let Some(interface) = interface {
        let save = interface
            .save
            .ok_or_else(|| error("LV2 native state save is unavailable"))?;
        if interface.restore.is_none() {
            return Err(error("LV2 native state restore is unavailable"));
        }
        // SAFETY: the caller serializes plugin DSP/editor mutation; all borrowed features and
        // the stack store context outlive this synchronous standard LV2 interface call.
        let status = unsafe {
            save(
                (*plugin.instance).lv2_handle,
                store,
                (&mut context as *mut StoreContext<'_>).cast(),
                PORTABLE_POD,
                plugin.features.feature_ptrs.as_ptr(),
            )
        };
        if let Some(failure) = context.failure {
            return Err(error(failure));
        }
        if status != 0 {
            return Err(error(format!(
                "LV2 native state save failed with status {status}"
            )));
        }
    }
    let ports: BTreeMap<_, _> = input_ports(plugin)?
        .into_iter()
        .map(|(symbol, offset)| (symbol, plugin.control_values[offset]))
        .collect();
    if ports.len() > COUNT_LIMIT || ports.values().any(|value| !value.is_finite()) {
        return Err(error(
            "LV2 input control state is invalid or exceeds its count limit",
        ));
    }
    let snapshot = Snapshot {
        version: 1,
        plugin_uri: plugin.info.unique_id.clone(),
        byte_order: byte_order().into(),
        native_state: interface.is_some(),
        ports,
        properties: context.properties,
    };
    let bytes = serde_json::to_vec(&snapshot).map_err(|failure| error(failure.to_string()))?;
    if bytes.len() > LIMIT {
        return Err(error("LV2 state exceeds 4 MiB"));
    }
    Ok(bytes)
}

struct RestoreProperty {
    key: u32,
    value_type: u32,
    bytes: Vec<u8>,
}
unsafe extern "C" fn retrieve(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    value_type: *mut u32,
    flags: *mut u32,
) -> *const c_void {
    // SAFETY: the immutable vector and every owned value live throughout native restore.
    let properties = unsafe { &*handle.cast::<Vec<RestoreProperty>>() };
    let Some(property) = properties.iter().find(|property| property.key == key) else {
        return std::ptr::null();
    };
    unsafe {
        if !size.is_null() {
            *size = property.bytes.len();
        }
        if !value_type.is_null() {
            *value_type = property.value_type;
        }
        if !flags.is_null() {
            *flags = PORTABLE_POD;
        }
    }
    property.bytes.as_ptr().cast()
}

fn decode(
    bytes: &[u8],
    plugin_uri: &str,
    map: &UridMap,
) -> Result<(Snapshot, Vec<RestoreProperty>)> {
    if bytes.len() > LIMIT {
        return Err(error("LV2 state exceeds 4 MiB"));
    }
    let snapshot: Snapshot = serde_json::from_slice(bytes)
        .map_err(|failure| error(format!("invalid LV2 state: {failure}")))?;
    if snapshot.version != 1
        || snapshot.plugin_uri != plugin_uri
        || snapshot.byte_order != byte_order()
        || snapshot.properties.len() > COUNT_LIMIT
        || snapshot.ports.len() > COUNT_LIMIT
        || (!snapshot.native_state && !snapshot.properties.is_empty())
    {
        return Err(error(
            "incompatible LV2 state identity, version, byte order, or property count",
        ));
    }
    let mut keys = BTreeSet::new();
    let mut total = 0_usize;
    let mut properties = Vec::new();
    for property in &snapshot.properties {
        validate_type(&property.value_type)?;
        if !keys.insert(&property.key) {
            return Err(error("duplicate LV2 state key"));
        }
        let bytes = match &property.value {
            PropertyValue::Urid(uri) if property.value_type == format!("{ATOM}URID") => {
                map_uri(map, uri)?.to_ne_bytes().to_vec()
            }
            PropertyValue::Bytes(bytes) if property.value_type != format!("{ATOM}URID") => {
                bytes.clone()
            }
            _ => return Err(error("LV2 state value does not match its type")),
        };
        total = total.saturating_add(bytes.len());
        if bytes.is_empty() || total > PROPERTY_LIMIT {
            return Err(error("LV2 state property size exceeds limit"));
        }
        properties.push(RestoreProperty {
            key: map_uri(map, &property.key)?,
            value_type: map_uri(map, &property.value_type)?,
            bytes,
        });
    }
    if snapshot.ports.values().any(|value| !value.is_finite()) {
        return Err(error("LV2 control state contains nonfinite values"));
    }
    Ok((snapshot, properties))
}

pub(super) fn restore(plugin: &mut Lv2Plugin, bytes: &[u8]) -> Result<()> {
    let (snapshot, properties) = decode(bytes, &plugin.info.unique_id, &plugin.features.urid_map)?;
    let ports = input_ports(plugin)?;
    if ports.keys().ne(snapshot.ports.keys()) {
        return Err(error("LV2 input control symbols changed"));
    }
    let interface = interface(plugin)?;
    if snapshot.native_state != interface.is_some() {
        return Err(error("LV2 native state capability changed"));
    }
    // All payload and control symbols are validated before invoking the native restore method.
    // Native restore can mutate before returning failure: the caller restores only a newly
    // prepared worker and discards that worker on failure, never commits it into a live graph.
    if let Some(interface) = interface {
        let restore = interface
            .restore
            .ok_or_else(|| error("LV2 native state restore is unavailable"))?;
        let status = unsafe {
            restore(
                (*plugin.instance).lv2_handle,
                retrieve,
                (&properties as *const Vec<RestoreProperty>)
                    .cast_mut()
                    .cast(),
                PORTABLE_POD,
                plugin.features.feature_ptrs.as_ptr(),
            )
        };
        if status != 0 {
            return Err(error(format!(
                "LV2 native state restore failed with status {status}"
            )));
        }
    }
    for (symbol, offset) in ports {
        plugin.control_values[offset] = snapshot.ports[&symbol];
    }
    Ok(())
}

#[cfg(test)]
mod tests;

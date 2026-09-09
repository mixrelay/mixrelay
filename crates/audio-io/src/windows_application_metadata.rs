//! Best-effort display metadata, deliberately separate from PID capture identity.
//! No target application is executed: version resources and shell icons are read as data.

use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    collections::BTreeMap,
    mem::size_of,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
use windows::{
    Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, HBITMAP, HDC, HGDIOBJ, SelectObject,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        },
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW},
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
        },
    },
    core::{PCWSTR, PWSTR},
};

const ICON_SIZE: usize = 32;
const CACHE_LIMIT: usize = 128;
const CACHE_LIFETIME: Duration = Duration::from_mins(1);

#[derive(Clone, Default)]
pub(super) struct Metadata {
    pub display_name: Option<String>,
    pub icon_data_url: Option<String>,
}

struct CachedMetadata {
    value: Metadata,
    modified: Option<SystemTime>,
    fetched: Instant,
}
type Cache = BTreeMap<String, CachedMetadata>;

pub(super) fn for_process(pid: u32) -> Metadata {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let Some(path) = executable_path(pid) else {
        return Metadata::default();
    };
    let cache = CACHE.get_or_init(Mutex::default);
    let modified = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok();
    let now = Instant::now();
    if let Ok(entries) = cache.lock()
        && let Some(entry) = entries.get(&path).filter(|entry| {
            entry.modified == modified && now.duration_since(entry.fetched) < CACHE_LIFETIME
        })
    {
        return entry.value.clone();
    }
    let wide = wide(&path);
    let value = Metadata {
        display_name: file_description(&wide),
        icon_data_url: icon_data_url(&wide),
    };
    // Shell/filesystem I/O must never hold the shared cache lock. Concurrent misses
    // may do duplicate bounded work, but cannot serialize on an unrelated process.
    if let Ok(mut cache) = cache.lock() {
        insert_cached(&mut cache, path, value.clone(), modified, now);
    }
    value
}

fn insert_cached(
    cache: &mut Cache,
    path: String,
    value: Metadata,
    modified: Option<SystemTime>,
    now: Instant,
) {
    cache.retain(|_, entry| now.duration_since(entry.fetched) < CACHE_LIFETIME);
    if cache.len() >= CACHE_LIMIT
        && !cache.contains_key(&path)
        && let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.fetched)
            .map(|(key, _)| key.clone())
    {
        cache.remove(&oldest);
    }
    cache.insert(
        path,
        CachedMetadata {
            value,
            modified,
            fetched: now,
        },
    );
}

fn executable_path(pid: u32) -> Option<String> {
    // Limited-query access works without debug privilege; protected/exited processes
    // simply keep their ToolHelp name. The process handle closes on every return.
    let process = super::OwnedHandle::new(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?,
    )
    .ok()?;
    let mut path = vec![0_u16; 32_768];
    let mut length = u32::try_from(path.len()).ok()?;
    unsafe {
        QueryFullProcessImageNameW(
            process.get(),
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &raw mut length,
        )
    }
    .ok()?;
    let path = String::from_utf16(path.get(..usize::try_from(length).ok()?)?).ok()?;
    (!path.is_empty() && Path::new(&path).is_absolute()).then_some(path)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn file_description(path: &[u16]) -> Option<String> {
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    // Reject pathological resource sizes rather than allocating from untrusted metadata.
    if length == 0 || length > 4 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0_u8; usize::try_from(length).ok()?];
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            length,
            data.as_mut_ptr().cast(),
        )
    }
    .ok()?;
    let translations = version_value(&data, "\\VarFileInfo\\Translation", false)?;
    for pair in translations.chunks_exact(4).take(64) {
        let language = u16::from_le_bytes([pair[0], pair[1]]);
        let codepage = u16::from_le_bytes([pair[2], pair[3]]);
        let key = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\FileDescription");
        if let Some(bytes) = version_value(&data, &key, true) {
            let utf16: Vec<_> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let text = super::wide_array_to_string(&utf16).trim().to_owned();
            if !text.is_empty() && text.chars().count() <= 512 {
                return Some(text);
            }
        }
    }
    None
}

fn version_value<'a>(data: &'a [u8], key: &str, unicode: bool) -> Option<&'a [u8]> {
    let mut pointer = std::ptr::null_mut();
    let mut length = 0;
    let key = wide(key);
    let found = unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(key.as_ptr()),
            &raw mut pointer,
            &raw mut length,
        )
    };
    if !found.as_bool() {
        return None;
    }
    let size = usize::try_from(length)
        .ok()?
        .checked_mul(if unicode { 2 } else { 1 })?;
    // VerQueryValue points inside its caller-owned block. Validate the complete
    // returned extent before exposing a slice, including malformed resources.
    let start = (pointer as usize).checked_sub(data.as_ptr() as usize)?;
    data.get(start..start.checked_add(size)?)
}

struct OwnedIcon(HICON);
impl Drop for OwnedIcon {
    fn drop(&mut self) {
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

/// A selected DIB is restored before deletion; no GDI handle crosses this scope.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut u8,
}
impl Surface {
    fn new() -> Option<Self> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return None;
        }
        let mut surface = Self {
            dc,
            bitmap: HBITMAP::default(),
            previous: HGDIOBJ::default(),
            pixels: std::ptr::null_mut(),
        };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        surface.bitmap = unsafe {
            CreateDIBSection(
                Some(dc),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                None,
                0,
            )
        }
        .ok()?;
        surface.pixels = bits.cast();
        surface.previous = unsafe { SelectObject(dc, HGDIOBJ(surface.bitmap.0)) };
        if surface.previous.is_invalid() || bits.is_null() {
            return None;
        }
        Some(surface)
    }
    fn draw(&mut self, icon: HICON, background: u8) -> Option<Vec<u8>> {
        unsafe { std::slice::from_raw_parts_mut(self.pixels, ICON_SIZE * ICON_SIZE * 4) }
            .fill(background);
        unsafe { DrawIconEx(self.dc, 0, 0, icon, 32, 32, 0, None, DI_NORMAL) }.ok()?;
        // GDI batches writes; synchronize before reading DIB memory on the CPU.
        if !unsafe { GdiFlush() }.as_bool() {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(self.pixels, ICON_SIZE * ICON_SIZE * 4) }.to_vec())
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_invalid() {
                SelectObject(self.dc, self.previous);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
            let _ = DeleteDC(self.dc);
        }
    }
}

fn icon_data_url(path: &[u16]) -> Option<String> {
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(path.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&raw mut info),
            u32::try_from(size_of::<SHFILEINFOW>()).ok()?,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if result == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let icon = OwnedIcon(info.hIcon);
    let mut surface = Surface::new()?;
    // Two backgrounds recover both legacy AND-mask and premultiplied-alpha icons.
    // Do not trust the alpha byte left by GDI for older executable resources.
    let black = surface.draw(icon.0, 0)?;
    let white = surface.draw(icon.0, 255)?;
    let rgba = transparent_rgba(&black, &white);
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 32, 32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&rgba).ok()?;
    writer.finish().ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn transparent_rgba(black: &[u8], white: &[u8]) -> Vec<u8> {
    black
        .chunks_exact(4)
        .zip(white.chunks_exact(4))
        .flat_map(|(black, white)| {
            let alpha = 255 - white[0].saturating_sub(black[0]);
            let unpremultiply = |value: u8| {
                if alpha == 0 {
                    0
                } else {
                    u8::try_from((u16::from(value) * 255 / u16::from(alpha)).min(255))
                        .unwrap_or(255)
                }
            };
            [
                unpremultiply(black[2]),
                unpremultiply(black[1]),
                unpremultiply(black[0]),
                alpha,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    static NATIVE_FIXTURE: Mutex<()> = Mutex::new(());

    #[test]
    fn cache_is_bounded_expires_and_replaces_same_path_without_eviction() {
        let mut cache = Cache::new();
        let start = Instant::now();
        for index in 0..CACHE_LIMIT + 10 {
            insert_cached(
                &mut cache,
                index.to_string(),
                Metadata::default(),
                None,
                start + Duration::from_millis(index as u64),
            );
        }
        assert_eq!(cache.len(), CACHE_LIMIT);
        assert!(!cache.contains_key("0"));
        insert_cached(
            &mut cache,
            "20".into(),
            Metadata {
                display_name: Some("replaced".into()),
                icon_data_url: None,
            },
            Some(SystemTime::UNIX_EPOCH),
            start + Duration::from_secs(1),
        );
        assert_eq!(cache.len(), CACHE_LIMIT);
        assert_eq!(cache["20"].value.display_name.as_deref(), Some("replaced"));
        insert_cached(
            &mut cache,
            "new".into(),
            Metadata::default(),
            None,
            start + CACHE_LIFETIME + Duration::from_secs(2),
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn alpha_conversion_preserves_transparency_color_and_legacy_mask() {
        assert_eq!(
            transparent_rgba(
                &[0, 0, 0, 0, 10, 20, 30, 0, 0, 0, 128, 128],
                &[255, 255, 255, 0, 10, 20, 30, 0, 127, 127, 255, 128]
            ),
            [0, 0, 0, 0, 30, 20, 10, 255, 255, 0, 0, 128]
        );
    }

    #[test]
    fn protected_or_missing_process_keeps_fallback_without_error() {
        assert!(executable_path(0).is_none());
        assert!(for_process(u32::MAX).icon_data_url.is_none());
    }

    #[test]
    fn current_process_path_is_stable_capture_identity() {
        let _fixture = NATIVE_FIXTURE.lock().unwrap();
        let path = executable_path(std::process::id()).expect("current executable is queryable");
        assert!(Path::new(&path).is_absolute());
        let apps = super::super::applications().expect("process inventory");
        let current = apps
            .iter()
            .find(|app| app.process_id == std::process::id())
            .expect("current process listed");
        assert_eq!(current.bundle_id, format!("process:{}", std::process::id()));
        assert!(
            matches!(&current.selector, crate::ApplicationSelector::ExecutablePath { path: selected } if selected == &super::super::process_identity::canonical_path(&path).unwrap())
        );
    }

    #[test]
    fn system_executable_has_real_description_and_decodable_png() {
        let _fixture = NATIVE_FIXTURE.lock().unwrap();
        let _com = unsafe { super::super::initialize_com() }.expect("COM");
        let system = std::env::var("SystemRoot").expect("Windows root");
        let path = wide(&format!("{system}\\System32\\cmd.exe"));
        assert!(
            !file_description(&path)
                .expect("cmd version description")
                .is_empty()
        );
        let icon = icon_data_url(&path).expect("cmd executable icon");
        let bytes = STANDARD
            .decode(icon.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let decoder = png::Decoder::new(bytes.as_slice());
        let reader = decoder.read_info().expect("valid PNG");
        assert_eq!((reader.info().width, reader.info().height), (32, 32));
    }

    #[test]
    fn repeated_native_metadata_releases_process_icon_and_gdi_handles() {
        use windows::Win32::System::Threading::{
            GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
            GetProcessHandleCount,
        };
        let _fixture = NATIVE_FIXTURE.lock().unwrap();
        let _com = unsafe { super::super::initialize_com() }.expect("COM");
        let path = wide(&format!(
            "{}\\System32\\cmd.exe",
            std::env::var("SystemRoot").unwrap()
        ));
        let counts = || {
            let process = unsafe { GetCurrentProcess() };
            let mut handles = 0;
            unsafe { GetProcessHandleCount(process, &raw mut handles) }.unwrap();
            (
                handles,
                unsafe { GetGuiResources(process, GR_GDIOBJECTS) },
                unsafe { GetGuiResources(process, GR_USEROBJECTS) },
            )
        };
        // Shell lazily initializes process-wide resources; warm before baseline.
        for _ in 0..5 {
            icon_data_url(&path).expect("warm icon");
        }
        let before = counts();
        for _ in 0..100 {
            executable_path(std::process::id()).expect("process path");
            icon_data_url(&path).expect("icon");
        }
        let after = counts();
        eprintln!("100 native icon/process-query cycles: handles/GDI/USER {before:?} -> {after:?}");
        assert!(
            after.0 <= before.0 && after.1 <= before.1 && after.2 <= before.2,
            "native handles grew: {before:?} -> {after:?}"
        );
    }
}

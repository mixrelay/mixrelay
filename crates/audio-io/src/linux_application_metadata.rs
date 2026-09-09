//! Bounded, passive desktop metadata lookup. Never execute a desktop entry's
//! `Exec`/`TryExec`, invoke a shell, or expose a filesystem URL to the webview.
//! PNG lookup covers the shared hicolor/Adwaita themes and pixmaps; unsupported
//! formats/themes honestly return no icon so the UI can retain its fallback.

use std::{
    env,
    fs::OpenOptions,
    io::{Cursor, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};

use super::PipeWireTarget;

const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_CACHE_ENTRIES: usize = 64;
const CACHE_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Metadata {
    pub display_name: Option<String>,
    pub icon_data_url: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    application_id: Option<String>,
    executable: Option<String>,
    icon_name: Option<String>,
}

struct Entry {
    key: Key,
    metadata: Metadata,
    created: Instant,
}

/// Both positive and negative results expire, including changed desktop files.
/// At most 64 entries of 64 KiB image data are retained; no process/PID cache is
/// needed and process exit does not accumulate permanent inventory state.
pub(super) fn resolve(target: &PipeWireTarget) -> Metadata {
    static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    let key = Key {
        application_id: bounded(target.application_id.as_deref()),
        executable: bounded(target.executable.as_deref()),
        icon_name: bounded(target.icon_name.as_deref()),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(entries) = cache.lock()
        && let Some(entry) = entries
            .iter()
            .find(|entry| entry.key == key && entry.created.elapsed() < CACHE_LIFETIME)
    {
        return entry.metadata.clone();
    }
    // No global lock during filesystem I/O: device discovery must not serialize
    // unrelated requests behind a slow user data directory.
    let metadata = lookup(&key, &data_roots(), locale().as_deref());
    if let Ok(mut entries) = cache.lock() {
        entries.retain(|entry| entry.key != key && entry.created.elapsed() < CACHE_LIFETIME);
        if entries.len() == MAX_CACHE_ENTRIES {
            entries.remove(0);
        }
        entries.push(Entry {
            key,
            metadata: metadata.clone(),
            created: Instant::now(),
        });
    }
    metadata
}

fn bounded(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(str::to_owned)
}

fn data_roots() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let user_data = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.as_ref().map(|path| path.join(".local/share")));
    let mut roots = user_data.into_iter().collect::<Vec<_>>();
    let dirs = env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    roots.extend(
        env::split_paths(&dirs)
            .filter(|path| path.is_absolute())
            .take(5),
    );
    if let Some(home) = home {
        roots.push(home.join(".local/share/flatpak/exports/share"));
    }
    roots.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    roots
}

fn locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.is_empty()))
        .and_then(|value| bounded(value.split('.').next()))
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
}

fn identifiers(key: &Key) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(id) = &key.application_id {
        names.push(id.trim_end_matches(".desktop").to_owned());
    }
    if let Some(binary) = &key.executable
        && let Some(name) = Path::new(binary).file_name().and_then(|name| name.to_str())
        && !names.iter().any(|id| id == name)
    {
        names.push(name.to_owned());
    }
    names.retain(|name| safe_name(name));
    names
}

fn lookup(key: &Key, roots: &[PathBuf], locale: Option<&str>) -> Metadata {
    let names = identifiers(key);
    let desktop = names.iter().find_map(|name| {
        let path = roots
            .iter()
            .map(|root| root.join("applications").join(format!("{name}.desktop")))
            .find(|path| path.is_file())?;
        // A user Hidden=true override must not resurrect a system desktop entry.
        let bytes = read_regular(&path)?;
        parse_desktop(std::str::from_utf8(&bytes).ok()?, locale)
    });
    let icon_names = key
        .icon_name
        .iter()
        .chain(desktop.as_ref().and_then(|entry| entry.1.as_ref()))
        .chain(names.iter());
    Metadata {
        icon_data_url: icon_names.into_iter().find_map(|name| icon(name, roots)),
        display_name: desktop.and_then(|entry| entry.0),
    }
}

fn parse_desktop(text: &str, locale: Option<&str>) -> Option<(Option<String>, Option<String>)> {
    let mut in_entry = false;
    let mut name = None;
    let mut localized = None;
    let mut language = None;
    let mut icon = None;
    let mut application = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Type" => application = value == "Application",
            "Hidden" if value == "true" => return None,
            "Name" => name = bounded(Some(value)),
            "Icon" => icon = bounded(Some(value)),
            key if locale.is_some_and(|locale| key == format!("Name[{locale}]")) => {
                localized = bounded(Some(value));
            }
            key if locale.is_some_and(|locale| {
                key == format!("Name[{}]", locale.split('_').next().unwrap_or(locale))
            }) =>
            {
                language = bounded(Some(value));
            }
            _ => {}
        }
    }
    application.then_some((localized.or(language).or(name), icon))
}

fn read_regular(path: &Path) -> Option<Vec<u8>> {
    // Check before open so FIFOs/devices from a bad entry cannot block discovery.
    // User-owned files may be symlinks (normal for Flatpak exports).
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_FILE_BYTES).then_some(bytes)
}

fn png(path: &Path) -> Option<String> {
    let bytes = read_regular(path)?;
    // Restrict the image contract to raster PNG. SVG/XPM are not embedded as
    // unchecked markup or converted by launching desktop commands.
    let valid_header = bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    let dimensions = valid_header
        && [16, 20].into_iter().all(|offset| {
            u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap_or_default()) <= 1024
                && bytes[offset..offset + 4] != [0; 4]
        });
    if !dimensions {
        return None;
    }
    let mut decoder = png::Decoder::new(Cursor::new(&bytes));
    decoder.set_limits(png::Limits {
        bytes: 4 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().ok()?;
    let output_size = reader.output_buffer_size();
    if output_size > 4 * 1024 * 1024 {
        return None;
    }
    reader.next_frame(&mut vec![0; output_size]).ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

fn icon(name: &str, roots: &[PathBuf]) -> Option<String> {
    let path = Path::new(name);
    if path.is_absolute() {
        return png(path);
    }
    if !safe_name(name) {
        return None;
    }
    let name = name.strip_suffix(".png").unwrap_or(name);
    roots.iter().find_map(|root| {
        for theme in ["hicolor", "Adwaita"] {
            for size in [
                "48x48", "32x32", "64x64", "128x128", "256x256", "512x512", "24x24", "16x16",
            ] {
                if let Some(icon) = png(&root
                    .join("icons")
                    .join(theme)
                    .join(size)
                    .join("apps")
                    .join(format!("{name}.png")))
                {
                    return Some(icon);
                }
            }
        }
        png(&root.join("pixmaps").join(format!("{name}.png")))
    })
}

#[cfg(test)]
#[path = "linux_application_metadata_tests.rs"]
mod tests;

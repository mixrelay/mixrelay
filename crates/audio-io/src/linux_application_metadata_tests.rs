use super::*;
use std::fs;

struct Fixture(tonenari_test_support::FixtureDir);

impl Fixture {
    fn new() -> Self {
        Self(tonenari_test_support::FixtureDir::new("icon").unwrap())
    }
    fn write(&self, path: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![self.0.path().to_owned()]
    }
}

fn key(id: Option<&str>, binary: Option<&str>, icon: Option<&str>) -> Key {
    Key {
        application_id: id.map(str::to_owned),
        executable: binary.map(str::to_owned),
        icon_name: icon.map(str::to_owned),
    }
}

fn image() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 128, 0, 255])
            .unwrap();
    }
    bytes
}

#[test]
fn desktop_id_resolves_localized_name_and_png_without_executing_commands() {
    let fixture = Fixture::new();
    fixture.write("applications/org.example.Player.desktop", b"[Desktop Entry]\nType=Application\nName=Player\nName[ko]=\xED\x94\x8C\xEB\xA0\x88\xEC\x9D\xB4\xEC\x96\xB4\nIcon=player\nExec=touch /not-executed\n[Desktop Action Stop]\nName=Wrong\nIcon=wrong");
    fixture.write("icons/hicolor/48x48/apps/player.png", &image());
    let metadata = lookup(
        &key(Some("org.example.Player"), None, None),
        &fixture.roots(),
        Some("ko_KR"),
    );
    assert_eq!(metadata.display_name.as_deref(), Some("플레이어"));
    assert_eq!(
        metadata.icon_data_url,
        Some(format!(
            "data:image/png;base64,{}",
            STANDARD.encode(image())
        ))
    );
}

#[test]
fn binary_basename_and_explicit_pipewire_icon_are_supported() {
    let fixture = Fixture::new();
    fixture.write(
        "applications/player.desktop",
        b"[Desktop Entry]\nType=Application\nName=Player\nIcon=missing",
    );
    fixture.write("pixmaps/stream-icon.png", &image());
    let metadata = lookup(
        &key(None, Some("/usr/bin/player"), Some("stream-icon")),
        &fixture.roots(),
        None,
    );
    assert_eq!(metadata.display_name.as_deref(), Some("Player"));
    assert!(metadata.icon_data_url.is_some());
}

#[test]
fn fallback_does_not_invent_metadata_or_follow_relative_traversal() {
    let fixture = Fixture::new();
    fixture.write(
        "outside.desktop",
        b"[Desktop Entry]\nType=Application\nName=Wrong",
    );
    assert_eq!(
        lookup(
            &key(Some("../outside"), None, Some("../outside")),
            &fixture.roots(),
            None
        ),
        Metadata::default()
    );
    assert_eq!(
        lookup(&key(Some("missing"), None, None), &fixture.roots(), None),
        Metadata::default()
    );
}

#[test]
fn rejects_svg_oversized_files_and_extreme_png_dimensions() {
    let fixture = Fixture::new();
    let svg = fixture.write("icon.svg", b"<svg onload='bad()'/>");
    assert!(icon(svg.to_str().unwrap(), &[]).is_none());
    let oversized = fixture.write("huge.png", &vec![1; MAX_FILE_BYTES + 1]);
    assert!(read_regular(&oversized).is_none());
    let mut bytes = image();
    bytes[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
    let bomb = fixture.write("bomb.png", &bytes);
    assert!(png(&bomb).is_none());
    assert!(read_regular(&fixture.0).is_none());
    let truncated = fixture.write("truncated.png", &image()[..24]);
    assert!(png(&truncated).is_none());
    let mut corrupted = image();
    corrupted[29] ^= 255; // IHDR CRC corruption is not a usable image.
    assert!(png(&fixture.write("corrupt.png", &corrupted)).is_none());
}

#[test]
fn desktop_parser_respects_sections_hidden_and_application_type() {
    assert_eq!(
        parse_desktop("[Desktop Entry]\nType=Link\nName=Wrong", None),
        None
    );
    assert_eq!(
        parse_desktop(
            "[Desktop Entry]\nType=Application\nHidden=true\nName=Wrong",
            None
        ),
        None
    );
    let parsed = parse_desktop(
        "[Desktop Entry]\nType=Application\nName=Default\nName[en]=English\nName[en_US]=American",
        Some("en_US"),
    );
    assert_eq!(parsed, Some((Some("American".into()), None)));
}

#[test]
fn xdg_root_precedence_keeps_user_metadata_first() {
    let user = Fixture::new();
    let system = Fixture::new();
    user.write(
        "applications/app.desktop",
        b"[Desktop Entry]\nType=Application\nName=User",
    );
    system.write(
        "applications/app.desktop",
        b"[Desktop Entry]\nType=Application\nName=System",
    );
    let metadata = lookup(
        &key(Some("app.desktop"), None, None),
        &[user.0.path().to_owned(), system.0.path().to_owned()],
        None,
    );
    assert_eq!(metadata.display_name.as_deref(), Some("User"));
    user.write(
        "applications/app.desktop",
        b"[Desktop Entry]\nType=Application\nHidden=true\nName=Deleted",
    );
    assert_eq!(
        lookup(
            &key(Some("app"), None, None),
            &[user.0.path().to_owned(), system.0.path().to_owned()],
            None
        ),
        Metadata::default()
    );
}

#[test]
fn identity_inputs_and_png_reads_have_size_limits() {
    assert_eq!(bounded(Some(&"x".repeat(513))), None);
    assert_eq!(bounded(Some("")), None);
    assert!(!safe_name("bad\nname"));
    assert!(!safe_name(".."));
    assert!(!safe_name(&"x".repeat(256)));
}

/// Opt-in desktop file read, not an application/audio launch or UI assertion.
#[test]
#[ignore = "requires an explicitly chosen installed desktop ID and PNG icon"]
fn installed_desktop_metadata_read_probe() {
    let id = env::var("TONENARI_LAB_METADATA_DESKTOP_ID").expect("explicit installed desktop ID");
    assert!(safe_name(&id));
    let metadata = lookup(&key(Some(&id), None, None), &data_roots(), None);
    assert!(
        metadata
            .display_name
            .as_ref()
            .is_some_and(|name| !name.is_empty())
    );
    let icon = metadata.icon_data_url.expect("installed PNG must decode");
    assert!(icon.starts_with("data:image/png;base64,"));
    println!(
        "desktop_id={id}; display_name={:?}; png_data_url_bytes={}",
        metadata.display_name,
        icon.len()
    );
}

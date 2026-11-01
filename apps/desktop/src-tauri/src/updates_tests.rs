use super::{
    Catalog, Download, LatestVersion, MAX_RESPONSE_BYTES, Platform, Product, Release, UpdateError,
    UpdateState, authorize_download, https_url, product_update, read_response, stable_version,
};

fn platform(os: &'static str, arch: &'static str) -> Platform {
    Platform {
        os,
        arch,
        package: if os == "linux" {
            super::Package::Deb
        } else {
            super::Package::Pkg
        },
    }
}

fn download(os: &str, arch: &str) -> Download {
    Download {
        os: os.into(),
        arch: arch.into(),
        url: format!(
            "https://downloads.example.test/{os}-{arch}.{}",
            if os == "linux" { "deb" } else { "pkg" }
        ),
        sha256: "a".repeat(64),
        size: 1024,
    }
}

fn catalog(product: Product, version: &str, downloads: Vec<Download>) -> Catalog {
    Catalog {
        product,
        latest: LatestVersion::Published(version.into()),
        releases: vec![Release {
            version: version.into(),
            published_at: "2026-10-06T00:00:00Z".into(),
            notes_url: Some("https://mixrelay.com/releases/1.2.10".into()),
            downloads,
        }],
    }
}

#[test]
fn empty_publication_is_not_current() {
    let catalog =
        serde_json::from_str::<Catalog>(r#"{"product":"mix-relay","latest":null,"releases":[]}"#)
            .expect("valid empty catalog");
    let update = product_update(
        Product::MixRelay,
        Some("0.1.0".into()),
        platform("macos", "arm64"),
        Ok(catalog),
    );
    assert_eq!(update.state, UpdateState::NotPublished);
    assert!(update.latest_version.is_none());
    assert!(update.downloads.is_empty());
}

#[test]
fn product_update_never_reports_unpublished_or_failed_checks_as_current() {
    for source in [
        Err(UpdateError::Unavailable),
        Ok(Catalog {
            product: Product::MixRelay,
            latest: LatestVersion::Empty(()),
            releases: Vec::new(),
        }),
    ] {
        let update = product_update(
            Product::MixRelay,
            Some("0.1.0".into()),
            platform("macos", "arm64"),
            source,
        );
        assert_eq!(update.product, Product::MixRelay);
        assert!(matches!(
            update.state(),
            UpdateState::Unavailable | UpdateState::NotPublished
        ));
        assert!(update.downloads().is_empty());
    }
}

#[test]
fn stable_versions_are_numeric_and_canonical() {
    let older = stable_version("1.2.9").expect("stable version");
    let newer = stable_version("1.2.10").expect("stable version");
    assert!(newer > older);
    for malformed in ["1.2", "01.2.3", "v1.2.3", "1.2.3-beta", "1.2.3+build", ""] {
        assert!(stable_version(malformed).is_err(), "{malformed}");
    }
    for (installed, expected) in [
        ("1.2.9", UpdateState::Available),
        ("1.2.10", UpdateState::Current),
        ("2.0.0", UpdateState::Current),
    ] {
        let update = product_update(
            Product::MixRelay,
            Some(installed.into()),
            platform("macos", "arm64"),
            Ok(catalog(
                Product::MixRelay,
                "1.2.10",
                vec![download("macos", "arm64")],
            )),
        );
        assert_eq!(update.state, expected);
    }
}

#[test]
fn assets_match_platform_architecture_and_macos_universal() {
    let downloads = vec![
        download("macos", "arm64"),
        download("macos", "x64"),
        download("macos", "universal"),
        download("linux", "arm64"),
        download("linux", "x64"),
        download("linux", "universal"),
    ];
    for (os, arch, expected) in [
        ("macos", "arm64", vec!["arm64", "universal"]),
        ("macos", "x64", vec!["x64", "universal"]),
        ("linux", "arm64", vec!["arm64"]),
        ("linux", "x64", vec!["x64"]),
    ] {
        let update = product_update(
            Product::MixRelay,
            Some("1.2.9".into()),
            platform(os, arch),
            Ok(catalog(Product::MixRelay, "1.2.10", downloads.clone())),
        );
        assert_eq!(update.state, UpdateState::Available);
        let architectures: Vec<&str> = update
            .downloads
            .iter()
            .map(|download| download.arch.as_str())
            .collect();
        assert_eq!(architectures, expected);
        assert!(update.downloads.iter().all(|download| download.os == os));
    }
}

#[test]
fn arch_updates_only_offer_and_authorize_the_contracts_arch_package() {
    let platform = Platform {
        os: "linux",
        arch: "x64",
        package: super::package::linux("ID=arch\n", "x64").unwrap(),
    };
    let deb = download("linux", "x64");
    let mut arch = deb.clone();
    arch.url = "https://downloads.example.test/mix-relay.pkg.tar.zst".into();
    let source = || catalog(Product::MixRelay, "1.2.10", vec![deb.clone(), arch.clone()]);
    let update = product_update(
        Product::MixRelay,
        Some("1.2.9".into()),
        platform,
        Ok(source()),
    );
    assert_eq!(update.state, UpdateState::Available);
    assert_eq!(update.downloads, vec![arch.clone()]);
    assert!(authorize_download(source(), Product::MixRelay, platform, &arch.url).is_ok());
    assert!(authorize_download(source(), Product::MixRelay, platform, &deb.url).is_err());
}

#[test]
fn missing_malformed_or_inapplicable_release_is_unavailable() {
    for body in [
        r#"{"product":"mix-relay","releases":[]}"#,
        r#"{"product":"mix-relay","latest":true,"releases":[]}"#,
        r#"{"product":"mix-relay","latest":"1.2.10","releases":[{"version":"1.2.10"}]}"#,
    ] {
        assert!(serde_json::from_str::<Catalog>(body).is_err());
    }
    let mut absent_release = catalog(Product::MixRelay, "1.2.10", vec![]);
    absent_release.releases.clear();
    let mut invalid_asset = download("macos", "arm64");
    invalid_asset.sha256 = "invalid".into();
    let catalogs = vec![
        absent_release,
        catalog(Product::MixRelay, "1.2.10", vec![]),
        catalog(Product::MixRelay, "1.2.10", vec![download("linux", "x64")]),
        catalog(Product::MixRelay, "1.2.10", vec![invalid_asset]),
        catalog(
            Product::MixRelay,
            "1.2.10-beta",
            vec![download("macos", "arm64")],
        ),
    ];
    for catalog in catalogs {
        let update = product_update(
            Product::MixRelay,
            Some("9.0.0".into()),
            platform("macos", "arm64"),
            Ok(catalog),
        );
        assert_eq!(update.state, UpdateState::Unavailable);
        assert!(update.downloads.is_empty());
    }
}

#[test]
fn download_urls_require_https_without_credentials() {
    assert!(https_url(
        "https://new-download-host.example.test/file.pkg?release=1"
    ));
    for unsafe_url in [
        "http://example.test/file.pkg",
        "file:///tmp/file.pkg",
        "javascript:alert(1)",
        "/downloads/file.pkg",
        "https://user@example.test/file.pkg",
        "https://user:secret@example.test/file.pkg",
        "https://example.test/\nfile.pkg",
    ] {
        assert!(!https_url(unsafe_url), "{unsafe_url}");
    }
}

#[test]
fn unknown_driver_version_reports_publication_without_inventing_app_version() {
    let update = product_update(
        Product::MixRelay,
        None,
        platform("linux", "x64"),
        Ok(catalog(
            Product::MixRelay,
            "1.2.10",
            vec![download("linux", "x64")],
        )),
    );
    assert_eq!(update.state, UpdateState::Published);
    assert!(update.installed_version.is_none());
    let serialized = serde_json::to_value(update).expect("serializable contract");
    assert_eq!(serialized["product"], "mix-relay");
    assert_eq!(serialized["installedVersion"], serde_json::Value::Null);
    assert_eq!(serialized["latestVersion"], "1.2.10");
    assert!(serialized.get("notesUrl").is_some());
}

#[test]
fn product_failure_does_not_suppress_other_product() {
    let platform = platform("macos", "arm64");
    let application = product_update(
        Product::MixRelay,
        Some("1.2.9".into()),
        platform,
        Err(UpdateError::Unavailable),
    );
    let driver = product_update(
        Product::MixRelay,
        Some("0.1.0".into()),
        platform,
        Ok(catalog(
            Product::MixRelay,
            "1.2.10",
            vec![download("macos", "universal")],
        )),
    );
    assert_eq!(application.state, UpdateState::Unavailable);
    assert_eq!(driver.state, UpdateState::Available);
}

#[test]
fn open_link_authorization_uses_current_product_and_platform_assets() {
    let platform = platform("macos", "arm64");
    for (url, accepted) in [
        ("https://downloads.example.test/macos-arm64.pkg", true),
        ("https://downloads.example.test/linux-x64.pkg", false),
        ("https://downloads.example.test/obsolete.pkg", false),
    ] {
        let result = authorize_download(
            catalog(
                Product::MixRelay,
                "1.2.10",
                vec![download("macos", "arm64"), download("linux", "x64")],
            ),
            Product::MixRelay,
            platform,
            url,
        );
        assert_eq!(result.is_ok(), accepted);
    }
}

#[tokio::test]
async fn response_body_limit_checks_declared_and_streamed_size() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    for declared in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("fixture connection");
            let mut request = [0; 1024];
            let received = socket.read(&mut request).await.expect("fixture request");
            assert!(
                received > 0,
                "client sent a request before the fixture response"
            );
            let length = MAX_RESPONSE_BYTES + 1;
            let header = if declared {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n")
            } else {
                format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{length:x}\r\n")
            };
            socket
                .write_all(header.as_bytes())
                .await
                .expect("fixture header");
            if !declared {
                // The client may close once it crosses the limit; that is the expected boundary.
                let _ = socket.write_all(&vec![b' '; length]).await;
                let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
            }
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("fixture client")
            .get(format!("http://{address}"))
            .send()
            .await
            .expect("fixture response");
        assert!(matches!(
            read_response(response).await,
            Err(UpdateError::ResponseTooLarge)
        ));
        server.await.expect("fixture server");
    }
}

#[test]
fn retired_separate_product_cannot_be_checked_or_downloaded() {
    assert!(serde_json::from_str::<Product>(r#""mix-relay-driver""#).is_err());
    assert!(
        serde_json::from_str::<Catalog>(
            r#"{"product":"mix-relay-driver","latest":null,"releases":[]}"#
        )
        .is_err()
    );
}

/// A stale fixture is rewritten and this fails; review the diff and commit it
/// (`pnpm fixtures:replies` reruns every fixture owner).
#[test]
fn updates_reply_fixture_matches_the_serialized_contract() {
    let update = product_update(
        Product::MixRelay,
        Some("1.2.9".into()),
        platform("macos", "arm64"),
        Ok(catalog(
            Product::MixRelay,
            "1.2.10",
            vec![download("macos", "universal")],
        )),
    );
    assert!(
        !mixrelay_test_support::reply_fixture::refresh_reply_fixture("updates", &[update]).unwrap(),
        "the updates reply fixture was stale and has been rewritten; review and commit it"
    );
}

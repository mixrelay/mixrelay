//! Public release checks and user-requested download links, outside the audio lifecycle.

use std::time::Duration;

use reqwest::{Client, Url};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod package;
use package::Package;

const PRODUCTS_ORIGIN: &str = "https://appserver.rawsector.com/products";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Independently distributed products accepted by the native update commands.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Product {
    /// The desktop application.
    MixRelay,
}

impl Product {
    fn slug(self) -> &'static str {
        match self {
            Self::MixRelay => "mix-relay",
        }
    }
}

/// Release observations; absent or failed publication is never reported as current.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateState {
    Available,
    Current,
    Published,
    NotPublished,
    Unavailable,
}

/// A publisher-provided installation package for this machine's platform.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Download {
    os: String,
    arch: String,
    url: String,
    sha256: String,
    size: u64,
}

impl Download {
    /// Returns the published link; opening it still requires catalog revalidation.
    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

/// One product's installed version and independently observed publication state.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductUpdate {
    product: Product,
    installed_version: Option<String>,
    latest_version: Option<String>,
    state: UpdateState,
    downloads: Vec<Download>,
    notes_url: Option<String>,
}

impl ProductUpdate {
    /// Returns the publication state, including failures and unpublished products.
    pub(crate) const fn state(&self) -> UpdateState {
        self.state
    }

    /// Returns the publisher's latest stable release version when one is available.
    pub(crate) fn latest_version(&self) -> Option<&str> {
        self.latest_version.as_deref()
    }

    /// Returns validated packages for the current platform.
    pub(crate) fn downloads(&self) -> &[Download] {
        &self.downloads
    }
}

/// Failures while reading authoritative catalogs or opening a validated package link.
#[derive(Debug, Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateError {
    /// The public service could not return a successful bounded response.
    #[error("The update service is unavailable")]
    Unavailable,
    /// The catalog does not describe a valid published release for this machine.
    #[error("The update catalog is invalid")]
    InvalidCatalog,
    /// The response exceeded the catalog size limit.
    #[error("The update catalog is too large")]
    ResponseTooLarge,
    /// The supplied URL is unsafe or absent from the current platform catalog.
    #[error("The download is no longer in the current update catalog")]
    InvalidDownload,
    /// The operating system could not open the validated download link.
    #[error("The download link could not be opened")]
    OpenFailed,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    product: Product,
    // An explicit null is a valid empty catalog; a missing field is a malformed response.
    latest: LatestVersion,
    releases: Vec<Release>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LatestVersion {
    Published(String),
    Empty(()),
}

#[derive(Debug, Deserialize)]
struct Release {
    version: String,
    published_at: String,
    notes_url: Option<String>,
    downloads: Vec<Download>,
}

#[derive(Clone, Copy, Debug)]
struct Platform {
    os: &'static str,
    arch: &'static str,
    package: Package,
}

impl Platform {
    fn current() -> Result<Self, UpdateError> {
        let os = match std::env::consts::OS {
            "macos" => "macos",
            "linux" => "linux",
            _ => return Err(UpdateError::Unavailable),
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            _ => return Err(UpdateError::Unavailable),
        };
        let package = if os == "linux" {
            let release =
                std::fs::read_to_string("/etc/os-release").map_err(|_| UpdateError::Unavailable)?;
            package::linux(&release, arch)?
        } else {
            Package::Pkg
        };
        Ok(Self { os, arch, package })
    }

    fn accepts(self, download: &Download) -> bool {
        download.os == self.os
            && (download.arch == self.arch || (self.os == "macos" && download.arch == "universal"))
            && self.package.accepts(&download.url)
    }
}

fn client() -> Result<Client, UpdateError> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| UpdateError::Unavailable)
}

async fn fetch_catalog(
    client: &Client,
    product: Product,
    platform: Platform,
) -> Result<Catalog, UpdateError> {
    let response = client
        .get(format!("{PRODUCTS_ORIGIN}/{}/releases", product.slug()))
        .query(&[("os", platform.os), ("arch", platform.arch)])
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| UpdateError::Unavailable)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(UpdateError::Unavailable);
    }
    let bytes = read_response(response).await?;
    serde_json::from_slice(&bytes).map_err(|_| UpdateError::InvalidCatalog)
}

async fn read_response(mut response: reqwest::Response) -> Result<Vec<u8>, UpdateError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(UpdateError::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| UpdateError::Unavailable)?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(UpdateError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn stable_version(value: &str) -> Result<Version, UpdateError> {
    let version = Version::parse(value).map_err(|_| UpdateError::InvalidCatalog)?;
    if !version.pre.is_empty() || !version.build.is_empty() || version.to_string() != value {
        return Err(UpdateError::InvalidCatalog);
    }
    Ok(version)
}

fn https_url(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
        && Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn validate_download(download: &Download) -> Result<(), UpdateError> {
    if !https_url(&download.url)
        || download.sha256.len() != 64
        || !download.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || download.size == 0
    {
        return Err(UpdateError::InvalidCatalog);
    }
    Ok(())
}

fn published_release(
    catalog: Catalog,
    product: Product,
    platform: Platform,
) -> Result<Option<Release>, UpdateError> {
    if catalog.product != product {
        return Err(UpdateError::InvalidCatalog);
    }
    let latest = match catalog.latest {
        LatestVersion::Empty(()) if catalog.releases.is_empty() => return Ok(None),
        LatestVersion::Empty(()) => return Err(UpdateError::InvalidCatalog),
        LatestVersion::Published(latest) => latest,
    };
    stable_version(&latest)?;
    let mut release = catalog
        .releases
        .into_iter()
        .find(|release| release.version == latest)
        .ok_or(UpdateError::InvalidCatalog)?;
    if release.published_at.trim().is_empty()
        || release
            .notes_url
            .as_deref()
            .is_some_and(|url| !https_url(url))
    {
        return Err(UpdateError::InvalidCatalog);
    }
    release
        .downloads
        .retain(|download| platform.accepts(download));
    if release.downloads.is_empty() {
        return Err(UpdateError::InvalidCatalog);
    }
    for download in &release.downloads {
        validate_download(download)?;
    }
    Ok(Some(release))
}

fn product_update(
    product: Product,
    installed_version: Option<String>,
    platform: Platform,
    catalog: Result<Catalog, UpdateError>,
) -> ProductUpdate {
    let mut update = unavailable_product(product, installed_version);
    match catalog.and_then(|catalog| published_release(catalog, product, platform)) {
        Ok(None) => update.state = UpdateState::NotPublished,
        Ok(Some(release)) => {
            let state = match update.installed_version.as_deref() {
                None => UpdateState::Published,
                Some(installed) => {
                    match (stable_version(installed), stable_version(&release.version)) {
                        (Ok(installed), Ok(latest)) if installed >= latest => UpdateState::Current,
                        (Ok(_), Ok(_)) => UpdateState::Available,
                        _ => return update,
                    }
                }
            };
            update.state = state;
            update.latest_version = Some(release.version);
            update.downloads = release.downloads;
            update.notes_url = release.notes_url;
        }
        Err(_) => {}
    }
    update
}

/// Checks the one complete `MixRelay` release for the background status menu.
pub(crate) async fn updates_check_driver() -> ProductUpdate {
    let product = Product::MixRelay;
    let installed =
        tauri::async_runtime::spawn_blocking(crate::driver_control::installed_mixrelay_version)
            .await
            .ok()
            .flatten();
    let (Ok(platform), Ok(client)) = (Platform::current(), client()) else {
        return unavailable_product(product, installed);
    };
    product_update(
        product,
        installed,
        platform,
        fetch_catalog(&client, product, platform).await,
    )
}

/// Checks the complete product; no license, devices or audio state are transmitted.
#[tauri::command]
pub(crate) async fn updates_check(app: tauri::AppHandle) -> Vec<ProductUpdate> {
    let product = Product::MixRelay;
    let installed = Some(app.package_info().version.to_string());
    let (Ok(platform), Ok(client)) = (Platform::current(), client()) else {
        return vec![unavailable_product(product, installed)];
    };
    vec![product_update(
        product,
        installed,
        platform,
        fetch_catalog(&client, product, platform).await,
    )]
}

fn unavailable_product(product: Product, installed_version: Option<String>) -> ProductUpdate {
    ProductUpdate {
        product,
        installed_version,
        latest_version: None,
        state: UpdateState::Unavailable,
        downloads: Vec::new(),
        notes_url: None,
    }
}

/// Open `url` only if it is still published for `product` and this machine's platform.
///
/// Re-fetches the current authoritative catalog before delegating to the OS browser. It never
/// downloads or executes an installer. Returns an error for unsafe/stale URLs, catalog failures,
/// unsupported platforms, or an OS link-opening failure.
#[tauri::command]
pub(crate) async fn updates_open_download(
    product: Product,
    url: String,
) -> Result<(), UpdateError> {
    if !https_url(&url) {
        return Err(UpdateError::InvalidDownload);
    }
    let platform = Platform::current()?;
    let catalog = fetch_catalog(&client()?, product, platform).await?;
    authorize_download(catalog, product, platform, &url)?;
    tauri::async_runtime::spawn_blocking(move || open::that(url))
        .await
        .map_err(|_| UpdateError::OpenFailed)?
        .map_err(|_| UpdateError::OpenFailed)
}

fn authorize_download(
    catalog: Catalog,
    product: Product,
    platform: Platform,
    url: &str,
) -> Result<(), UpdateError> {
    let release =
        published_release(catalog, product, platform)?.ok_or(UpdateError::InvalidDownload)?;
    if !release.downloads.iter().any(|download| download.url == url) {
        return Err(UpdateError::InvalidDownload);
    }
    Ok(())
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;

//! Fixed paths owned by the `MixRelay` administrator-installed distribution.

/// The application bundle owned by the installer.
pub const APP_PATH: &str = "/Applications/MixRelay Driver.app";
/// The persistent Driver service binary owned by the installer.
pub const DRIVER_PATH: &str = "/Library/Application Support/MixRelay/bin/mixrelay-driver-service";
/// The HAL bundle owned by the installer.
pub const HAL_PATH: &str = "/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver";
/// The launchd configuration owned by the installer.
pub const LAUNCHD_PATH: &str = "/Library/LaunchDaemons/io.mixrelay.audio.driver.plist";
/// The root-owned directory containing the three lifecycle lease inodes.
pub const LIFECYCLE_DIRECTORY: &str = "/Library/Application Support/MixRelay/lifecycle";
/// The fixed desktop ownership lease inode.
pub const APP_LEASE_PATH: &str = "/Library/Application Support/MixRelay/lifecycle/app.owner";
/// The independent Driver Manager process ownership lease inode.
pub const DRIVER_MANAGER_LEASE_PATH: &str =
    "/Library/Application Support/MixRelay/lifecycle/driver-manager.owner";
/// The fixed engine ownership lease inode.
pub const ENGINE_LEASE_PATH: &str = "/Library/Application Support/MixRelay/lifecycle/engine.owner";
/// The fixed exclusive installer-transaction lease inode.
pub const INSTALLER_LEASE_PATH: &str =
    "/Library/Application Support/MixRelay/lifecycle/installer.owner";

/// One exact path that a coordinated transaction may replace or remove.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedTarget {
    /// The application bundle.
    App,
    /// The persistent Driver service binary.
    Driver,
    /// The HAL bundle.
    Hal,
    /// The launchd property list.
    Launchd,
}

impl OwnedTarget {
    /// Returns the only absolute path associated with this owned target.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::App => APP_PATH,
            Self::Driver => DRIVER_PATH,
            Self::Hal => HAL_PATH,
            Self::Launchd => LAUNCHD_PATH,
        }
    }

    /// Returns every replaceable target in deterministic transaction order.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::App, Self::Driver, Self::Hal, Self::Launchd]
    }
}

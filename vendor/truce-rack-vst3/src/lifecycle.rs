//! Guard successfully initialized VST3 objects until the adapter takes ownership.

use truce_rack_core::error::{Error, Result};
use vst3::{
    ComPtr,
    Steinberg::{FUnknown, IPluginBase, IPluginBaseTrait, kResultOk},
};

/// Calls `terminate` if a successfully initialized object exits a partially built load path.
pub(super) struct InitializedPlugin {
    plugin: ComPtr<IPluginBase>,
    armed: bool,
}

impl InitializedPlugin {
    pub(super) fn initialize(plugin: &ComPtr<IPluginBase>, context: *mut FUnknown) -> Result<Self> {
        // SAFETY: `plugin` is retained and context is owned by HostServices through termination.
        if unsafe { plugin.initialize(context) } != kResultOk {
            return Err(Error::Other("VST3 IPluginBase::initialize failed".into()));
        }
        Ok(Self {
            plugin: plugin.clone(),
            armed: true,
        })
    }

    /// Transfer termination responsibility to the complete `Vst3Plugin` owner.
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InitializedPlugin {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: this guard is armed only after successful initialize and runs once before
            // the last retained interface/module owner is dropped on the creator thread.
            unsafe { self.plugin.terminate() };
        }
    }
}

use nanopython_core::Capability;
use nanopython_plugin_api::Plugin;

pub struct EnumLitePlugin;

impl Plugin for EnumLitePlugin {
    fn name(&self) -> &'static str {
        "enum-lite"
    }

    fn provides(&self) -> &'static [Capability] {
        &[Capability::EnumProvider]
    }
}

pub fn plugin() -> EnumLitePlugin {
    EnumLitePlugin
}

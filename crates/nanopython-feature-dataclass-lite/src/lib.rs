use nanopython_core::Capability;
use nanopython_plugin_api::Plugin;

pub struct DataclassLitePlugin;

impl Plugin for DataclassLitePlugin {
    fn name(&self) -> &'static str {
        "dataclass-lite"
    }

    fn provides(&self) -> &'static [Capability] {
        &[Capability::DataclassProvider]
    }
}

pub fn plugin() -> DataclassLitePlugin {
    DataclassLitePlugin
}

use std::collections::{HashMap, HashSet};

use nanopython_core::{Capability, NanoPythonError, Result};
use rustpython_vm::VirtualMachine;

const REQUIRED_SINGLETON_CAPABILITIES: &[Capability] =
    &[Capability::EnumProvider, Capability::DataclassProvider];

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn provides(&self) -> &'static [Capability];
    fn requires(&self) -> &'static [Capability] {
        &[]
    }

    fn install(&self, _vm: &mut VirtualMachine) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P>(&mut self, plugin: P) -> Result<()>
    where
        P: Plugin + 'static,
    {
        self.register_boxed(Box::new(plugin))
    }

    pub fn register_boxed(&mut self, plugin: Box<dyn Plugin>) -> Result<()> {
        if self.plugins.iter().any(|p| p.name() == plugin.name()) {
            return Err(NanoPythonError::PluginRegistration(format!(
                "duplicate plugin name: {}",
                plugin.name()
            )));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut providers: HashMap<Capability, Vec<&'static str>> = HashMap::new();
        for plugin in &self.plugins {
            for capability in plugin.provides() {
                providers
                    .entry(*capability)
                    .or_default()
                    .push(plugin.name());
            }
        }

        for capability in REQUIRED_SINGLETON_CAPABILITIES {
            let count = providers.get(capability).map_or(0, Vec::len);
            if count != 1 {
                let names = providers
                    .get(capability)
                    .map(|list| list.join(", "))
                    .unwrap_or_else(|| "<none>".to_string());
                return Err(NanoPythonError::PluginValidation(format!(
                    "capability {:?} requires exactly one provider, found {} ({})",
                    capability, count, names
                )));
            }
        }

        let available: HashSet<Capability> = providers.keys().copied().collect();
        for plugin in &self.plugins {
            for requirement in plugin.requires() {
                if !available.contains(requirement) {
                    return Err(NanoPythonError::PluginValidation(format!(
                        "plugin {} requires missing capability {:?}",
                        plugin.name(),
                        requirement
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn install_all(&self, vm: &mut VirtualMachine) -> Result<()> {
        for plugin in &self.plugins {
            plugin.install(vm)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnumPlugin;
    impl Plugin for EnumPlugin {
        fn name(&self) -> &'static str {
            "enum"
        }

        fn provides(&self) -> &'static [Capability] {
            &[Capability::EnumProvider]
        }
    }

    struct DataclassPlugin;
    impl Plugin for DataclassPlugin {
        fn name(&self) -> &'static str {
            "dataclass"
        }

        fn provides(&self) -> &'static [Capability] {
            &[Capability::DataclassProvider]
        }
    }

    #[test]
    fn validates_required_capabilities() {
        let mut registry = PluginRegistry::new();
        registry.register(EnumPlugin).unwrap();
        registry.register(DataclassPlugin).unwrap();
        registry.validate().unwrap();
    }

    #[test]
    fn fails_with_missing_required_capability() {
        let mut registry = PluginRegistry::new();
        registry.register(EnumPlugin).unwrap();
        let err = registry.validate().unwrap_err();
        assert!(format!("{err}").contains("DataclassProvider"));
    }

    #[test]
    fn fails_when_singleton_capability_has_multiple_providers() {
        struct EnumPlugin2;
        impl Plugin for EnumPlugin2 {
            fn name(&self) -> &'static str {
                "enum2"
            }

            fn provides(&self) -> &'static [Capability] {
                &[Capability::EnumProvider]
            }
        }

        let mut registry = PluginRegistry::new();
        registry.register(EnumPlugin).unwrap();
        registry.register(EnumPlugin2).unwrap();
        registry.register(DataclassPlugin).unwrap();

        let err = registry.validate().unwrap_err();
        assert!(format!("{err}").contains("exactly one provider"));
    }
}

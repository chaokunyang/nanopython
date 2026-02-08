use std::process::ExitCode;

use nanopython_plugin_api::PluginRegistry;
use nanopython_vm::NanoPythonVm;

fn main() -> ExitCode {
    let mut registry = PluginRegistry::new();

    if let Err(err) = registry.register(nanopython_stdlib_core::plugin()) {
        eprintln!("nanopython-extended failed to register stdlib-core plugin: {err}");
        return ExitCode::from(2);
    }

    if let Err(err) = registry.register(nanopython_feature_enum_lite::plugin()) {
        eprintln!("nanopython-extended failed to register enum plugin: {err}");
        return ExitCode::from(2);
    }

    if let Err(err) = registry.register(nanopython_feature_dataclass_lite::plugin()) {
        eprintln!("nanopython-extended failed to register dataclass plugin: {err}");
        return ExitCode::from(2);
    }

    NanoPythonVm::new(registry).run()
}

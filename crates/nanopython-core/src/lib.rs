use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    StdlibCore,
    EnumProvider,
    DataclassProvider,
}

impl Capability {
    pub const fn is_singleton(self) -> bool {
        matches!(self, Self::EnumProvider | Self::DataclassProvider)
    }
}

#[derive(Debug, Error)]
pub enum NanoPythonError {
    #[error("plugin registration failed: {0}")]
    PluginRegistration(String),
    #[error("plugin validation failed: {0}")]
    PluginValidation(String),
    #[error("plugin bootstrap failed in {plugin}: {message}")]
    PluginBootstrap {
        plugin: &'static str,
        message: String,
    },
    #[error("parse error: {0}")]
    Parse(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("io error: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, NanoPythonError>;

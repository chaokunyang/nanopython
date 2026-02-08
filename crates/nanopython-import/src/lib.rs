//! Import policy declarations for NanoPython profiles.

pub const BLOCKED_MODULE_ROOTS: &[&str] = &[
    "asyncio",
    "threading",
    "multiprocessing",
    "pickle",
    "socket",
];

pub fn is_blocked_module(fullname: &str) -> bool {
    let root = fullname.split('.').next().unwrap_or(fullname);
    BLOCKED_MODULE_ROOTS.iter().any(|blocked| blocked == &root)
}

#[cfg(test)]
mod tests {
    use super::is_blocked_module;

    #[test]
    fn blocks_roots_and_submodules() {
        assert!(is_blocked_module("asyncio"));
        assert!(is_blocked_module("threading.local"));
        assert!(!is_blocked_module("pathlib"));
    }
}

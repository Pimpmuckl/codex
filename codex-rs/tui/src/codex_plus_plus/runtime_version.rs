use codex_install_context::InstallContext;
use std::sync::OnceLock;

/// Resolve the public package version without stamping shared crate builds.
pub(crate) fn codex_cli_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| version_for_install(InstallContext::current()))
}

fn version_for_install(context: &InstallContext) -> String {
    context
        .package_manifest()
        .map(|manifest| manifest.version.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

#[cfg(test)]
#[path = "runtime_version_tests.rs"]
mod tests;

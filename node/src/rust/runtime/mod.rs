pub mod api_servers;
pub mod node_runtime;
pub mod servers_instances;
pub mod setup;
#[cfg(feature = "mettail-frontend")]
pub mod f1r3lang;

/// Unactivated source routes may not silently select the legacy compiler.
pub(crate) fn require_legacy_source_route(route: &str) -> Result<(), String> {
    if cfg!(feature = "mettail-frontend") {
        Err(format!(
            "{route} is not activated for F1R3Lang; use the funded eval/REPL route"
        ))
    } else {
        Ok(())
    }
}

// Node module - Main blockchain node implementation
// Empty module - ready for future implementation

pub mod api;
pub mod configuration;
pub mod diagnostics;
pub mod effects;
pub mod encode;
pub mod node_environment;
pub mod repl;
pub mod rho_trie_traverser;
pub mod runtime;
pub mod soak_observer;
pub mod state;

// Re-export for convenience
pub use encode::JsonEncoder;
pub mod consensus;

//! Embedded Rholang source for the genesis ceremony.
//!
//! Each constant holds the contents of a `.rho` / `.rhox` resource baked
//! into the binary at compile time via `include_str!`. The previous
//! file-search path (`fs::read_to_string` against an 8-element fallback
//! ladder of relative paths) has been removed: the production binary no
//! longer requires the workspace tree to be present at run time.
//!
//! `.rhox` constants hold the unsubstituted template; macro substitution
//! is performed by `CompiledRholangTemplate::new` at the call site.

pub const REGISTRY: &str = include_str!("../../../main/resources/Registry.rho");
pub const VERSIONED_REGISTRY: &str = include_str!("../../../main/resources/VersionedRegistry.rho");
pub const LIST_OPS: &str = include_str!("../../../main/resources/ListOps.rho");
pub const EITHER: &str = include_str!("../../../main/resources/Either.rho");
pub const NON_NEGATIVE_NUMBER: &str = include_str!("../../../main/resources/NonNegativeNumber.rho");
pub const MAKE_MINT: &str = include_str!("../../../main/resources/MakeMint.rho");
pub const AUTH_KEY: &str = include_str!("../../../main/resources/AuthKey.rho");
pub const SYSTEM_VAULT: &str = include_str!("../../../main/resources/SystemVault.rho");
pub const MULTI_SIG_SYSTEM_VAULT: &str =
    include_str!("../../../main/resources/MultiSigSystemVault.rho");
pub const STACK: &str = include_str!("../../../main/resources/Stack.rho");
pub const TOKEN_METADATA: &str = include_str!("../../../main/resources/TokenMetadata.rhox");
pub const POS: &str = include_str!("../../../main/resources/PoS.rhox");
pub const CAPABILITIES_REGISTRY: &str =
    include_str!("../../../main/resources/CapabilitiesRegistry.rhox");
pub const EXCHANGE: &str = include_str!("../../../main/resources/Exchange.rhox");

// File I/O FIP library agents (slice 19 genesis wiring — File I/O FIP
// §The Fs agent; composed into a single deploy via fs_genesis module).
pub const FILE: &str = include_str!("../../../main/resources/File.rho");
pub const DIR: &str = include_str!("../../../main/resources/Dir.rho");
pub const STREAM: &str = include_str!("../../../main/resources/Stream.rho");
pub const BUFFER: &str = include_str!("../../../main/resources/Buffer.rho");
pub const STDIN: &str = include_str!("../../../main/resources/Stdin.rho");
pub const STDOUT: &str = include_str!("../../../main/resources/Stdout.rho");
pub const FS: &str = include_str!("../../../main/resources/Fs.rho");

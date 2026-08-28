// Startup validation that the on-chain TokenMetadata contract values match
// the node's local `native-token-*` configuration.
//
// This guards against the "lying API" scenario where a node joins an existing
// network but has mismatched token metadata in its config: the protocol would
// still work (the values on-chain are the only ones that matter), but the
// node's `/api/status` responses would advertise values that disagree with
// what was baked into genesis state.
//
// Caught here, the node logs a clear error explaining which value(s) disagree
// and refuses to continue. Caught at genesis ceremony time instead, the node
// would fail to sign the UnapprovedBlock and the ceremony would stall without
// a clear reason.

use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::rho_type::{RhoNumber, RhoString};

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const TOKEN_METADATA_QUERY: &str = r#"
    new ret, rl(`rho:registry:lookup`), tmCh in {
      rl!(`rho:system:tokenMetadata`, *tmCh) |
      for (@(_, TokenMetadata) <- tmCh) {
        @TokenMetadata!("all", *ret)
      }
    }
"#;

/// Queries the on-chain TokenMetadata contract and returns
/// `(name, symbol, decimals)` read from the `rho:system:tokenMetadata`
/// registry entry.
pub async fn read_on_chain_token_metadata(
    runtime_manager: &RuntimeManager,
    post_state_hash: &StateHash,
) -> Result<(String, String, u32), CasperError> {
    let (result, _cost) = runtime_manager
        .play_exploratory_deploy(TOKEN_METADATA_QUERY.to_string(), post_state_hash, None)
        .await?;

    // The contract's "all" method returns a single tuple `(name, symbol, decimals)`
    // on the exploratory deploy return channel.
    let tuple_par = result.first().ok_or_else(|| {
        CasperError::RuntimeError("TokenMetadata exploratory deploy returned no values".to_string())
    })?;

    parse_all_tuple(tuple_par).ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "TokenMetadata contract returned an unexpected shape; expected (String, String, Int), got: {:?}",
            tuple_par
        ))
    })
}

fn parse_all_tuple(par: &Par) -> Option<(String, String, u32)> {
    let expr = par.exprs.first()?;
    let etuple = match expr.expr_instance.as_ref()? {
        models::rhoapi::expr::ExprInstance::ETupleBody(t) => t,
        _ => return None,
    };

    if etuple.ps.len() != 3 {
        return None;
    }

    let name = RhoString::unapply(&etuple.ps[0])?;
    let symbol = RhoString::unapply(&etuple.ps[1])?;
    let decimals = RhoNumber::unapply(&etuple.ps[2])?;

    if decimals < 0 || decimals > i64::from(u32::MAX) {
        return None;
    }

    Some((name, symbol, decimals as u32))
}

/// The CANONICAL read of the protocol fault-tolerance threshold: returns the
/// exact ppm numerator (θ = ppm / 1_000_000) that the finalization DECISION
/// runs on. The `f32` sibling below is display-only.
///
/// There is exactly ONE query mechanism and ONE validity gate, both owned by
/// `RuntimeOps::get_fault_tolerance_threshold_ppm` (the strict
/// `getFaultToleranceThresholdPpm` exploratory query + the `[-1e6, 1e6]` range
/// gate). This function adds the single POLICY on top of it: the on-chain value
/// is mandatory.
///
/// Absent (`None`) is a HARD ERROR, not a fall back to local configuration.
/// θ is a consensus value — the finalized-floor oracle runs on it and the floor
/// decides the multi-parent merge base — so a node that silently substituted its
/// own config would derive a different floor from its peers and permanently
/// invalidate their blocks (the `ComputedPreStateMismatch` → `UnknownRootError`
/// cascade). Failing closed also keeps `reconcile(local, onchain) = onchain` a
/// TOTAL second projection, which is exactly what Rocq
/// `FtProvenance.reconcile_agrees_on_onchain` proves: local config is not a fork
/// input. A fallback arm would make that theorem false at `None`.
///
/// This is safe to require: the parameter is baked into the PoS contract at
/// genesis, so only a chain whose genesis predates it could answer `None`, and
/// no such chain exists on this (unreleased) branch.
pub async fn read_on_chain_fault_tolerance_threshold_ppm(
    runtime_manager: &RuntimeManager,
    post_state_hash: &StateHash,
) -> Result<i64, CasperError> {
    runtime_manager
        .get_fault_tolerance_threshold_ppm(post_state_hash)
        .await?
        .ok_or_else(|| {
            CasperError::RuntimeError(
                "PoS contract exposes no getFaultToleranceThresholdPpm: the protocol \
                 fault-tolerance threshold is a consensus value and MUST come from chain \
                 state; refusing to fall back to local configuration (it would diverge \
                 this node's finalized-floor merge base from its peers')"
                    .to_string(),
            )
        })
}

/// LOSSY `f32` view of the on-chain ppm threshold. Retained for display /
/// back-compat; the exact DECISION path uses
/// [`read_on_chain_fault_tolerance_threshold_ppm`].
pub async fn read_on_chain_fault_tolerance_threshold(
    runtime_manager: &RuntimeManager,
    post_state_hash: &StateHash,
) -> Result<f32, CasperError> {
    let ppm = read_on_chain_fault_tolerance_threshold_ppm(runtime_manager, post_state_hash).await?;
    Ok((ppm as f64 / 1_000_000.0) as f32)
}

/// Compares the on-chain token metadata against the node's local config.
/// Returns `Err` with a descriptive message if any field disagrees.
///
/// This is called before a joining node constructs Casper or publishes the
/// Running state. A mismatch means the operator's config does not reflect the
/// values baked into this chain's genesis block, so startup fails before APIs
/// or consensus tasks can observe the node as ready.
pub async fn verify_token_metadata_matches_config(
    runtime_manager: &RuntimeManager,
    post_state_hash: &StateHash,
    config_name: &str,
    config_symbol: &str,
    config_decimals: u32,
) -> Result<(), CasperError> {
    let (on_chain_name, on_chain_symbol, on_chain_decimals) =
        read_on_chain_token_metadata(runtime_manager, post_state_hash).await?;

    // Track mismatches both as a machine-parseable list of field names
    // (used by integration tests via structured log fields) and as a
    // human-readable description (used in the returned error message).
    let mut mismatched_fields: Vec<&'static str> = Vec::new();
    let mut mismatch_descriptions: Vec<String> = Vec::new();
    if on_chain_name != config_name {
        mismatched_fields.push("native-token-name");
        mismatch_descriptions.push(format!(
            "native-token-name: config={:?}, on-chain={:?}",
            config_name, on_chain_name
        ));
    }
    if on_chain_symbol != config_symbol {
        mismatched_fields.push("native-token-symbol");
        mismatch_descriptions.push(format!(
            "native-token-symbol: config={:?}, on-chain={:?}",
            config_symbol, on_chain_symbol
        ));
    }
    if on_chain_decimals != config_decimals {
        mismatched_fields.push("native-token-decimals");
        mismatch_descriptions.push(format!(
            "native-token-decimals: config={}, on-chain={}",
            config_decimals, on_chain_decimals
        ));
    }

    if !mismatched_fields.is_empty() {
        // Emit a structured log event BEFORE returning the error so that
        // integration tests (and operators) can grep the JSON-formatted logs
        // for a stable event without regex-parsing English error text.
        // Field names are stable identifiers matching the HOCON key names.
        //
        // mismatched_fields is joined into a comma-separated string so that
        // the tracing JSON layer serializes it as a plain JSON string that
        // consumers (tests, log pipelines) can split on ',' rather than
        // parsing Rust Debug-formatted Vec syntax.
        let mismatched_fields_joined = mismatched_fields.join(",");
        tracing::error!(
            event = "native_token_metadata_mismatch",
            mismatched_fields = %mismatched_fields_joined,
            config_name = %config_name,
            on_chain_name = %on_chain_name,
            config_symbol = %config_symbol,
            on_chain_symbol = %on_chain_symbol,
            config_decimals = config_decimals,
            on_chain_decimals = on_chain_decimals,
            "native token metadata mismatch: configured values do not match \
             values baked into this network's genesis state"
        );

        return Err(CasperError::RuntimeError(format!(
            "Configured native token metadata does not match the values baked \
             into this network's genesis state. Mismatches: [{}]. \
             Update casper.genesis-block-data.native-token-* in your config to \
             match the on-chain values, or connect to a network whose genesis \
             was created with your configured values.",
            mismatch_descriptions.join("; ")
        )));
    }

    tracing::info!(
        event = "native_token_metadata_verified",
        native_token_name = %config_name,
        native_token_symbol = %config_symbol,
        native_token_decimals = config_decimals,
        "Verified on-chain token metadata matches local configuration"
    );

    Ok(())
}

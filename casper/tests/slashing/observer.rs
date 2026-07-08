// SlashingObserver — the read-only contract every tier of the
// slashing test architecture must satisfy.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.4
// (tier model, added by Track 9). Plan-agent design from session
// committed at fa29d33+.
//
// The principled architecture defines three tiers, each implementing
// `SlashingObserver`:
//
//   Tier 1 — Production: SlashingProductionAdapter wraps TestNode +
//            BlockDagKeyValueStorage + the dispatcher path.
//   Tier 2 — Oracle: RocqOracleAdapter wraps oracle.rs's pure
//            functions over (DagState, EqRecordSet, PoSState).
//   Tier 3 — Harness: SlashingTestHarness implements directly.
//
// Triple-bisim tests (Track 3) drive the same event sequence
// through all three implementations and assert agreement on every
// observable. Disagreement on any observable → harness/oracle/
// production drift, which the assertion locates by exception.
//
// Mutating operations (sign_block, dispatch, execute_slash) are
// NOT part of this trait — they are tier-specific because they
// take tier-specific input types (Tier 1: BlockMessage; Tier 2/3:
// BlockHash). Cross-tier driver code lives in
// `triple_bisim_driver.rs` (Track 3) and converts between tiers.

use std::collections::BTreeSet;

use super::types::{BlockHash, SeqNum, ValidatorId};

/// Read-only observation surface common to all three tiers.
///
/// Every method is a pure read against the tier's projected state.
/// Implementations must be deterministic in their argument tuple.
pub trait SlashingObserver {
    /// Validator's current bond. Zero for unknown validators.
    fn bond(&self, validator: &str) -> i64;

    /// Coop-vault balance — total forfeited stake.
    fn coop_vault(&self) -> i64;

    /// Whether `validator` is in the active set.
    fn is_active(&self, validator: &str) -> bool;

    /// Whether an `EquivocationRecord` exists at `(validator, base_seq)`.
    fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool;

    /// Witness set of the record at `(validator, base_seq)`. Empty
    /// when no record exists.
    fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash>;

    /// Validators counted in the GHOST estimator (active set minus
    /// those whose latest message is invalid). Sorted for
    /// deterministic comparison across tiers.
    fn fork_choice(&self) -> Vec<ValidatorId>;
}

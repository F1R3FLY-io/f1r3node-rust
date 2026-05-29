// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/ProofOfStake.scala

use super::validator::Validator;

// TODO: Eliminate validators argument if unnecessary. - OLD
// TODO: eliminate the default for epochLength. Now it is used in order to minimise the impact of adding this parameter - OLD
// TODO: Remove hardcoded keys from standard deploys: https://rchain.atlassian.net/browse/RCHAIN-3321?atlOrigin=eyJpIjoiNDc0NjE4YzYxOTRkNDcyYjljZDdlOWMxYjE1NWUxNjIiLCJwIjoiaiJ9 - OLD
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProofOfStake {
    pub minimum_bond: i64,
    pub maximum_bond: i64,
    pub validators: Vec<Validator>,
    pub epoch_length: i32,
    pub quarantine_length: i32,
    pub number_of_active_validators: u32,
    pub pos_multi_sig_public_keys: Vec<String>,
    pub pos_multi_sig_quorum: u32,
    /// Per-deploy hard cap on the number of cosigners in a multi-signature
    /// deploy. Substituted into the PoS contract at genesis as
    /// `$$maxCosignersPerDeploy$$`. Default `64`; configurable per shard via
    /// `casper_conf.rs::max_cosigners_per_deploy`. Defense-in-depth against
    /// adversarial deploys exhausting block resources via runaway cosigner
    /// lists.
    pub max_cosigners_per_deploy: u32,
    /// Initial phlogiston minted into a validator's draw wallet `@W_v` when
    /// it first bonds (Cost-Accounted Rho, spec Appendix B; DR-13). Drawn by
    /// the per-validator bootstrap `VB ≜ for(phlo<-@W_v){ VH | *phlo }`: an
    /// empty `@W_v` ⇒ `VB` blocks ⇒ the validator is effectively offline (the
    /// DR-3 halt mechanism). Substituted into the PoS contract at genesis as
    /// `$$initialPhlogiston$$`; configurable per shard via
    /// `casper_conf.rs::initial_phlogiston`. NOTE: `@W_v` (the draw channel)
    /// is DISTINCT from the supply pool `Σ⟦v⟧ = from_sig(Ground(pk))` the
    /// acceptance gate reads — the `Σ⟦v⟧` balance write is a Rust
    /// `produce_balance` in a later stage, never an in-Rholang write
    /// (`from_sig` is unnameable in Rholang).
    pub initial_phlogiston: i64,
    /// Phlogiston minted into each active validator's draw wallet `@W_v` at
    /// every epoch boundary (Cost-Accounted Rho, spec Appendix B / §4.7;
    /// DR-13). The renewable validator fuel matched to the desugared signed
    /// layers of the validator handler. Substituted into the PoS contract at
    /// genesis as `$$epochPhlogiston$$`; configurable per shard via
    /// `casper_conf.rs::epoch_phlogiston`. The epoch mint loop itself is a
    /// later stage; this field carries the per-epoch amount.
    pub epoch_phlogiston: i64,
}

impl ProofOfStake {
    // TODO: Determine how the "initial bonds" map can simulate transferring stake into the PoS contract
    //       when this must be done during genesis, under the authority of the genesisPk, which calls the
    //       linear receive in PoS.rho - OLD
    pub fn initial_bonds(validators: &[Validator]) -> String {
        let mut sorted_validators = validators.to_vec();
        sorted_validators.sort_by(|a, b| a.pk.bytes.cmp(&b.pk.bytes));

        let map_entries = sorted_validators
            .iter()
            .map(|validator| {
                let pk_string = hex::encode(validator.pk.bytes.clone());
                format!(" \"{}\".hexToBytes() : {}", pk_string, validator.stake)
            })
            .collect::<Vec<String>>()
            .join(", ");

        format!("{{{}}}", map_entries)
    }

    pub fn public_keys(pos_multi_sig_public_keys: &[String]) -> String {
        let indent_brackets = 12;
        let indent_keys = indent_brackets + 2;

        let pub_key_items = pos_multi_sig_public_keys
            .iter()
            .map(|pk| format!("{}\"{}\".hexToBytes()", " ".repeat(indent_keys), pk))
            .collect::<Vec<String>>()
            .join(",\n");

        format!("[\n{}\n{}]", pub_key_items, " ".repeat(indent_brackets))
    }
}

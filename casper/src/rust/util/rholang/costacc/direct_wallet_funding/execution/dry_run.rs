//! Added by DR-134 (DR-71): the user-facing exploratory estimate is a native
//! unfunded dry run. The term runs under the native meter of a funded offer:
//! the offered play runtime, the adopted schedule and contract, the production
//! trace limits and the record room of play. The dry run reads no funding,
//! runs no funding check and no settlement, resets to the start root,
//! publishes nothing, and returns the measured phlo. Its bound is the
//! operator's exploratory limit under `NativeBoundSource::SignedLimit`.
//!
//! The signer is the deployer, else the node's exploratory key. The signer
//! sets the funding signature, the deploy data and the normalizer environment
//! as for an owner-direct offer of that key, so the meter measures the same
//! COMMs and bytes as that offer at the same root (DR-71 obligation).
//!
//! Added by G3 (DR-135, S11): the dry run has no owner funding, so it
//! reports the work of the whole run. Under owner funding, the owners pay
//! their shares at a price of 1 or more from their own budgets, and the
//! signed limit of a caller needs to cover only the caller's part (item 13).

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrincipalId, GPrivate, GUnforgeable, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::normalizer_env::normalizer_env_from_v61_single_signer;
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{self, NativeRuntimeConfig};
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::{
    DeployAuthority, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::native_record_room;
use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::{exploratory_key_pair, RuntimeOps};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::offered_grants::offered_grant_issue_call_limits;
use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_trace_limits;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::tools::Tools;

const DRY_RUN_IDENTITY_DOMAIN: &[u8] = b"f1r3node:exploratory-dry-run:v1";

fn runtime_error(error: impl std::fmt::Display) -> CasperError {
    CasperError::RuntimeError(error.to_string())
}

/// The identity of a dry run: a domain-separated Blake2b-256 hash of the start
/// root, the signer and the term, each with its length.
fn dry_run_identity(start: &StateHash, signer: &PublicKey, term: &str) -> [u8; 32] {
    let parts: [&[u8]; 4] = [
        DRY_RUN_IDENTITY_DOMAIN,
        start.as_ref(),
        signer.bytes.as_ref(),
        term.as_bytes(),
    ];
    let mut preimage = Vec::with_capacity(parts.iter().map(|part| part.len() + 8).sum::<usize>());
    for part in parts {
        preimage.extend_from_slice(&(part.len() as u64).to_be_bytes());
        preimage.extend_from_slice(part);
    }
    Blake2b256::hash(preimage)
        .try_into()
        .expect("a Blake2b-256 digest is 32 bytes")
}

/// The native configuration of a dry run: the adopted schedule at the
/// minimum price, controls with the operator's limit, and the contract under
/// the signed-limit bound.
fn dry_run_config(
    adopted: &AdoptedResourcePolicy,
    limit: u64,
) -> Result<NativeRuntimeConfig, CasperError> {
    let price = adopted.genesis().minimum_price();
    let genesis_schedule = adopted
        .genesis()
        .record()
        .schedule()
        .map_err(runtime_error)?;
    let selected = PhloScheduleV1 {
        actual_price: price,
        ..genesis_schedule
    };
    let binding =
        PhloScheduleBinding::new(&selected, PhloGenesisPolicy::LIMITS).map_err(runtime_error)?;
    let schedule = binding.schedule();
    let schedules = [schedule];
    let owner_ceilings = [price];
    let controls = check_phlo_controls(
        schedule.environment,
        price,
        i64::MAX as u64,
        SignedPhloControls {
            limit,
            price_ceiling: price,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        schedule,
        limit,
    )
    .map_err(runtime_error)?;
    let contract =
        adopted.bind_execution_contract(controls, &binding, NativeBoundSource::SignedLimit)?;
    Ok(
        NativeRuntimeConfig::new(contract, offered_funded_v6_trace_limits())
            .with_record_room(native_record_room()),
    )
}

/// The return channel of a dry run: the first unforgeable name of its
/// identity-seeded random source, as for an offered deploy.
fn dry_run_return_name(identity: &[u8; 32]) -> Par {
    let mut rand = Tools::user_identity_rng(identity);
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: rand.next().into_iter().map(|b| b as u8).collect(),
        })),
    }])
}

impl RuntimeOps {
    /// The term run of a dry run, installed as `evaluate_native_envelope`
    /// installs an owner-direct offer of `signer`.
    async fn evaluate_native_dry_run(
        &mut self,
        term: &str,
        identity: [u8; 32],
        signer: &PublicKey,
        config: NativeRuntimeConfig,
    ) -> Result<EvaluateResult, CasperError> {
        self.runtime
            .set_deploy_data(SystemProcessDeployData {
                timestamp: 0,
                authority: DeployAuthority::Principal(GPrincipalId {
                    key_family: 1,
                    public_key: signer.bytes.to_vec(),
                }),
                deploy_id: identity.to_vec(),
            })
            .await;
        self.runtime.cost.set_unmetered(false);
        self.runtime
            .cost
            .set_deploy_id_funded(
                identity,
                accounting::funding_sig_single(&accounting::principal_ground_v61(&signer.bytes)),
            )
            .map_err(runtime_error)?;
        self.runtime
            .evaluate_with_native_phlo(
                term,
                normalizer_env_from_v61_single_signer(&identity, signer),
                Tools::user_identity_rng(&identity),
                None,
                config,
            )
            .await
            .map_err(CasperError::InterpreterError)
    }
}

impl RuntimeManager {
    /// DR-71: runs `term` at `start_state` as a native unfunded dry run and
    /// returns the data at its return channel and its measured phlo. The root
    /// after the dry run is the start root, and nothing is published.
    pub async fn play_exploratory_dry_run(
        &self,
        term: String,
        start_state: &StateHash,
        deployer: Option<PublicKey>,
        adopted: &AdoptedResourcePolicy,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let limit = u64::try_from(self.exploratory_deploy_phlo_limit())
            .map_err(|_| runtime_error("the exploratory phlo limit is negative"))?;
        let signer = deployer.unwrap_or_else(|| exploratory_key_pair().1.clone());
        let identity = dry_run_identity(start_state, &signer, &term);
        let config = dry_run_config(adopted, limit)?;
        let mut runtime = RuntimeOps::new(
            self.spawn_offered_play_runtime(offered_grant_issue_call_limits())
                .await?,
        );
        let start_root = Blake2b256Hash::from_bytes_prost(start_state);
        runtime.runtime.reset(&start_root).await?;
        let checkpoint = runtime.runtime.create_soft_checkpoint().await;
        let evaluation = runtime
            .evaluate_native_dry_run(&term, identity, &signer, config)
            .await;
        let outcome = match evaluation {
            Ok(evaluation) if !evaluation.errors.is_empty() => {
                Err(CasperError::InterpreterError(evaluation.errors[0].clone()))
            }
            Ok(evaluation) if !evaluation.byte_observations.has_complete_measurements() => Err(
                runtime_error("the exploratory dry run has incomplete byte measurements"),
            ),
            Ok(evaluation) => match evaluation.native_phlo_usage {
                Some(used) if runtime.runtime.get_root().await == start_root => {
                    let data = runtime.get_data_par(&dry_run_return_name(&identity)).await;
                    Ok((data, used))
                }
                Some(_) => Err(runtime_error(
                    "the exploratory dry run changed its start root",
                )),
                None => Err(runtime_error("the exploratory dry run has no phlo usage")),
            },
            Err(error) => Err(error),
        };
        runtime.runtime.revert_to_soft_checkpoint(checkpoint).await;
        outcome
    }
}

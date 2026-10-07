use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::{Cosigned, Cosigner};
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::{BindPattern, CostSignature, ListParWithRandom, Par};
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::phlo_grant_creation::{
    PhloGrantCreationLimits, PhloGrantCreationV1, VerifiedGrantCreationV1,
};
use models::rust::phlo_intent::{PhloFundingIntentV2Limits, PhloFundingIntentVersioned};
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use models::rust::utils::{new_freevar_par, new_gsys_auth_token_par};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList};
use rholang::rust::interpreter::system_processes::{byte_name, Definition};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use rspace_plus_plus::rspace::trace::Log;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::vault_payer::{vault_payer, VaultPayer};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const GRANT_CHANNEL_DOMAIN: &[u8] = b"f1r3node:offered-grant:v1";
const ALLOWANCE_KEY_DOMAIN: &[u8] = b"f1r3node:offered-grant:allowance:v1";
const NONCE_KEY_DOMAIN: &[u8] = b"f1r3node:offered-grant:nonce:v1";
const REPLACEMENT_DOMAIN: &[u8] = b"f1r3node:offered-grant:replacement:v1";
const RECORD_VERSION: u8 = 1;
const RECORD_BYTES: usize = 269;
const NONCE_VALUE: &[u8] = &[1];
const GRANT_ISSUE_CALL_DOMAIN: &[u8] = b"f1r3node:offered-grant-issue-call:v1";
const GRANT_ISSUE_BODY_REF: i64 = 160;
const GRANT_ISSUE_JOURNAL_CHANNEL: u8 = 161;

pub fn grant_issue_journal_channel() -> Par { byte_name(GRANT_ISSUE_JOURNAL_CHANNEL) }

pub fn count_grant_issue_requests(
    log: &[Event],
    limits: OfferedGrantLimits,
    budget: &HostWorkBudget,
) -> Result<usize, CasperError> {
    reserve(budget, HostWorkDimension::VerificationOperations, log.len())?;
    let hash = stable_hash_provider::hash(&grant_issue_journal_channel());
    let mut count = 0usize;
    for event in log {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == hash => {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| invalid("grant issue request count overflow"))?;
                if count > limits.uses {
                    return Err(invalid("grant issue request count exceeds limit"));
                }
            }
            Event::Comm(comm)
                if comm
                    .produces
                    .iter()
                    .any(|produce| produce.channel_hash == hash) =>
            {
                return Err(invalid("grant issue journal matched a continuation"));
            }
            _ => {}
        }
    }
    Ok(count)
}

pub fn grant_issue_definition(limits: GrantIssueCallLimits, budget: HostWorkBudget) -> Definition {
    Definition {
        urn: "rho:cost:grant:issue:v1".to_owned(),
        fixed_channel: byte_name(GRANT_ISSUE_BODY_REF as u8),
        arity: 1,
        body_ref: GRANT_ISSUE_BODY_REF,
        handler: Box::new(move |context| {
            let budget = budget.clone();
            Box::new(move |arguments| {
                let context = context.clone();
                let budget = budget.clone();
                Box::pin(async move {
                    if !context.cost.native_execution_active() {
                        return Err(InterpreterError::ReduceError(
                            "grant issuance requires native offered execution".to_owned(),
                        ));
                    }
                    let [message] = arguments.0.as_slice() else {
                        return Err(InterpreterError::ReduceError(
                            "grant issue expects one request".to_owned(),
                        ));
                    };
                    let [payload] = message.pars.as_slice() else {
                        return Err(InterpreterError::ReduceError(
                            "grant issue expects one byte payload".to_owned(),
                        ));
                    };
                    let [models::rhoapi::Expr {
                        expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(call)),
                    }] = payload.exprs.as_slice()
                    else {
                        return Err(InterpreterError::ReduceError(
                            "grant issue expects canonical bytes".to_owned(),
                        ));
                    };
                    if call.len() > limits.wire.total_bytes {
                        return Err(InterpreterError::ReduceError(
                            "grant issue exceeds syntax limit".to_owned(),
                        ));
                    }
                    reserve(&budget, HostWorkDimension::SearchStateBytes, call.len())
                        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
                    reserve(
                        &budget,
                        HostWorkDimension::VerificationOperations,
                        call.len(),
                    )
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
                    if *payload != RhoByteArray::create_par(call.clone()) {
                        return Err(InterpreterError::ReduceError(
                            "grant issue payload is noncanonical".to_owned(),
                        ));
                    }
                    let request = datum(call);
                    let produced = context
                        .space
                        .produce(grant_issue_journal_channel(), request, false)
                        .await
                        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
                    if produced.is_some() {
                        return Err(InterpreterError::ReduceError(
                            "grant issue journal matched a continuation".to_owned(),
                        ));
                    }
                    Ok(Vec::new())
                })
            })
        }),
        remainder: None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GrantIssueCallLimits {
    pub wire: PhloWireLimits,
    pub creation: PhloGrantCreationLimits,
}

pub fn offered_grant_issue_call_limits() -> GrantIssueCallLimits {
    let protocol = offered_funded_v6_limits();
    GrantIssueCallLimits {
        wire: protocol.envelope.payload.signing,
        creation: PhloGrantCreationLimits {
            wire: protocol.envelope.payload.funding.wire,
            grant_id_bytes: protocol.envelope.payload.funding.wire.field_bytes,
        },
    }
}

pub fn offered_grant_transition_limits() -> OfferedGrantLimits {
    let protocol = offered_funded_v6_limits();
    OfferedGrantLimits {
        uses: protocol.envelope.payload.funding.sources,
        grant_id_bytes: protocol.envelope.payload.funding.wire.field_bytes,
        batch_bytes: protocol.evidence.field_bytes,
    }
}

pub fn encode_grant_issue_call(
    envelope: &Cosigned<PhloGrantCreationV1>,
    limits: GrantIssueCallLimits,
) -> Result<Vec<u8>, CasperError> {
    envelope
        .validate_envelope()
        .map_err(|error| invalid(&format!("invalid grant owner signature: {error}")))?;
    let selected = envelope
        .selected_signers_v61()
        .map_err(|error| invalid(&format!("invalid grant owner signer: {error}")))?;
    if selected.len() != 1 || envelope.signers().len() != 1 {
        return Err(invalid("grant creation requires one owner signer"));
    }
    let signer = selected[0];
    if signer.sig_algorithm.name() != Secp256k1::name() {
        return Err(invalid("grant creation requires secp256k1"));
    }
    let mut encoder = PhloWireEncoder::new(limits.wire);
    for field in [
        GRANT_ISSUE_CALL_DOMAIN,
        envelope.data.encoded(),
        signer.pk.bytes.as_ref(),
        signer.sig.as_ref(),
    ] {
        encoder
            .bytes(field)
            .map_err(|error| invalid(&format!("grant issue call exceeds syntax limit: {error}")))?;
    }
    Ok(encoder.into_bytes())
}

pub fn decode_verified_grant_issue_call(
    call: &[u8],
    expected_network: &[u8],
    expected_shard: &[u8],
    limits: GrantIssueCallLimits,
    budget: &HostWorkBudget,
) -> Result<VerifiedGrantCreationV1, CasperError> {
    if call.len() > limits.wire.total_bytes {
        return Err(invalid("grant issue call exceeds syntax limit"));
    }
    reserve(budget, HostWorkDimension::SearchStateBytes, call.len())?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        call.len().saturating_add(128),
    )?;
    let mut decoder = PhloWireDecoder::new(call, limits.wire)
        .map_err(|error| invalid(&format!("invalid grant issue call: {error}")))?;
    if decoder
        .bytes()
        .map_err(|error| invalid(&format!("invalid grant issue domain: {error}")))?
        != GRANT_ISSUE_CALL_DOMAIN
    {
        return Err(invalid("unsupported grant issue call domain"));
    }
    let creation = PhloGrantCreationV1::decode(
        decoder
            .bytes()
            .map_err(|error| invalid(&format!("invalid grant creation field: {error}")))?,
        limits.creation,
    )
    .map_err(|error| invalid(&format!("invalid grant creation: {error}")))?;
    let public_key = decoder
        .bytes()
        .map_err(|error| invalid(&format!("invalid grant owner field: {error}")))?;
    let signature = decoder
        .bytes()
        .map_err(|error| invalid(&format!("invalid grant signature field: {error}")))?;
    decoder
        .finish()
        .map_err(|error| invalid(&format!("trailing grant issue data: {error}")))?;
    if public_key.len() != 65 || signature.is_empty() {
        return Err(invalid("invalid grant owner key or signature width"));
    }
    let signer = Cosigner {
        pk: PublicKey::from_bytes(public_key),
        sig: signature.to_vec().into(),
        sig_algorithm: Box::new(Secp256k1),
    };
    let envelope = Cosigned::from_envelope_signed_data_threshold(creation, vec![signer], 1)
        .map_err(|error| invalid(&format!("invalid grant owner signature: {error}")))?;
    VerifiedGrantCreationV1::verify(envelope, expected_network, expected_shard)
        .map_err(|error| invalid(&format!("unauthorized grant creation: {error}")))
}

pub fn grant_context_hash(network: &[u8], shard: &[u8], schedule: &[u8; 32]) -> [u8; 32] {
    Blake2b256::hash_stream(|feed| {
        feed(b"f1r3node:offered-grant:context:v1");
        for value in [network, shard, schedule.as_slice()] {
            feed(&(value.len() as u64).to_be_bytes());
            feed(value);
        }
    })
    .try_into()
    .expect("Blake2b256 emits 32 bytes")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferedGrantLimits {
    pub uses: usize,
    pub grant_id_bytes: usize,
    pub batch_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferedGrantRecord {
    pub authority_version: u64,
    pub cumulative_ceiling: u128,
    pub consumed: u128,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub issuer_public_key: [u8; 65],
    pub delegate_public_key: [u8; 65],
    pub custody: [u8; 32],
    pub context: [u8; 32],
    pub max_draw: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferedGrantDraw<'a> {
    pub grant_id: &'a [u8],
    pub delegate_public_key: [u8; 65],
    pub custody: [u8; 32],
    pub context: [u8; 32],
    pub authority_version: u64,
    pub operation_id: [u8; 32],
    pub max_draw: u128,
    pub cumulative_ceiling: u128,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub draw: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GrantValueKind {
    Allowance,
    Nonce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GrantSlot {
    key: [u8; 32],
    kind: GrantValueKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OfferedGrantSnapshot {
    root: [u8; 32],
    entries: Vec<(GrantSlot, Option<Vec<u8>>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedGrantChange {
    slot: GrantSlot,
    expected: Option<Vec<u8>>,
    replacement: Vec<u8>,
}

impl OfferedGrantChange {
    pub fn key(&self) -> [u8; 32] { self.slot.key }
    pub fn expected(&self) -> Option<&[u8]> { self.expected.as_deref() }
    pub fn replacement(&self) -> &[u8] { &self.replacement }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PreparedOfferedGrants {
    root: [u8; 32],
    changes: Vec<OfferedGrantChange>,
}

#[derive(Debug)]
struct VerifiedGrantSource {
    payer: VaultPayer,
    record: OfferedGrantRecord,
    grant_id: Vec<u8>,
    operation_id: [u8; 32],
    max_draw: u128,
    cumulative_ceiling: u128,
    valid_from: Option<u64>,
    valid_until: Option<u64>,
    source_debit_cap: u64,
}

#[derive(Debug)]
pub struct VerifiedOfferedGrantSources {
    root: [u8; 32],
    envelope_identity: [u8; 32],
    sources: BTreeMap<u32, VerifiedGrantSource>,
}

impl VerifiedOfferedGrantSources {
    pub fn root(&self) -> [u8; 32] { self.root }
    pub fn envelope_identity(&self) -> [u8; 32] { self.envelope_identity }
    pub fn payer_for_source(&self, source_index: u32) -> Option<&VaultPayer> {
        self.sources.get(&source_index).map(|source| &source.payer)
    }
    pub fn record_for_source(&self, source_index: u32) -> Option<&OfferedGrantRecord> {
        self.sources.get(&source_index).map(|source| &source.record)
    }
    pub fn source_indices(&self) -> impl Iterator<Item = u32> + '_ { self.sources.keys().copied() }
}

impl PreparedOfferedGrants {
    pub fn root(&self) -> [u8; 32] { self.root }
    pub fn changes(&self) -> &[OfferedGrantChange] { &self.changes }
    pub fn into_changes(self) -> Vec<OfferedGrantChange> { self.changes }
}

fn invalid(reason: &str) -> CasperError {
    CasperError::RuntimeError(format!("offered grant: {reason}"))
}

fn signed_grant_intent<'a>(
    offer: &'a Cosigned<OfferedFundedDeploy>,
    budget: &HostWorkBudget,
) -> Result<models::rust::phlo_intent::PhloFundingIntentV2<'a>, CasperError> {
    let protocol = offered_funded_v6_limits();
    let bytes = offer.data.funding_intent();
    if bytes.len() > protocol.envelope.payload.funding.wire.total_bytes {
        return Err(invalid("signed grant intent exceeds protocol syntax limit"));
    }
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes.len())?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        bytes
            .len()
            .checked_add(offer.signers().len().saturating_mul(128))
            .ok_or_else(|| invalid("signed grant verification work overflow"))?,
    )?;
    offer
        .validate_envelope()
        .map_err(|error| invalid(&format!("invalid offered signature: {error}")))?;
    let base = protocol.envelope.payload.funding;
    let limits = PhloFundingIntentV2Limits {
        wire: base.wire,
        base,
        grant_uses: base.wire.total_bytes / 8,
        grant_id_bytes: base.wire.field_bytes,
        quote_evidence_bytes: base.wire.field_bytes,
    };
    match PhloFundingIntentVersioned::decode(bytes, limits)
        .map_err(|error| invalid(&format!("invalid signed grant intent: {error}")))?
    {
        PhloFundingIntentVersioned::V2(intent) => Ok(intent),
        PhloFundingIntentVersioned::V1(_) => Err(invalid("signed grant use requires V2 intent")),
    }
}

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    budget
        .reserve(
            dimension,
            HostWorkUnits::new(u64::try_from(amount).map_err(|_| invalid("work overflow"))?),
        )
        .map(|_| ())
        .map_err(|error| invalid(&error.to_string()))
}

fn check_id(id: &[u8], limits: OfferedGrantLimits) -> Result<(), CasperError> {
    if id.is_empty() || id.len() > limits.grant_id_bytes {
        return Err(invalid("invalid grant identity length"));
    }
    Ok(())
}

fn slot(
    kind: GrantValueKind,
    id: &[u8],
    operation: Option<&[u8; 32]>,
    limits: OfferedGrantLimits,
    budget: &HostWorkBudget,
) -> Result<GrantSlot, CasperError> {
    check_id(id, limits)?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        id.len().saturating_add(64),
    )?;
    let domain = match kind {
        GrantValueKind::Allowance => ALLOWANCE_KEY_DOMAIN,
        GrantValueKind::Nonce => NONCE_KEY_DOMAIN,
    };
    let bytes = Blake2b256::hash_stream(|feed| {
        feed(domain);
        feed(&(id.len() as u64).to_be_bytes());
        feed(id);
        if let Some(operation) = operation {
            feed(operation);
        }
    });
    Ok(GrantSlot {
        key: bytes.try_into().expect("Blake2b256 emits 32 bytes"),
        kind,
    })
}

fn allowance_slot(
    id: &[u8],
    limits: OfferedGrantLimits,
    budget: &HostWorkBudget,
) -> Result<GrantSlot, CasperError> {
    slot(GrantValueKind::Allowance, id, None, limits, budget)
}

fn nonce_slot(
    id: &[u8],
    operation: &[u8; 32],
    limits: OfferedGrantLimits,
    budget: &HostWorkBudget,
) -> Result<GrantSlot, CasperError> {
    slot(GrantValueKind::Nonce, id, Some(operation), limits, budget)
}

fn channel(slot: GrantSlot) -> Par {
    RhoList::create_par(vec![
        new_gsys_auth_token_par(Vec::new(), false),
        RhoByteArray::create_par(GRANT_CHANNEL_DOMAIN.to_vec()),
        RhoByteArray::create_par(slot.key.to_vec()),
    ])
}

fn datum(bytes: &[u8]) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![RhoByteArray::create_par(bytes.to_vec())],
        random_state: Vec::new(),
        cost_authority: None,
        cost_stack: None,
    }
}

fn check_record(record: OfferedGrantRecord) -> Result<(), CasperError> {
    if record.consumed > record.cumulative_ceiling
        || record.max_draw == 0
        || record.max_draw > record.cumulative_ceiling
        || crypto::rust::public_key::PublicKey::validate_secp256k1_bytes(&record.issuer_public_key)
            .is_err()
        || crypto::rust::public_key::PublicKey::validate_secp256k1_bytes(
            &record.delegate_public_key,
        )
        .is_err()
        || record
            .valid_from
            .zip(record.valid_until)
            .is_some_and(|(from, until)| from > until)
    {
        return Err(invalid("invalid allowance record bounds"));
    }
    Ok(())
}

fn endpoint(value: Option<u64>) -> [u8; 9] {
    let mut bytes = [0; 9];
    if let Some(value) = value {
        bytes[0] = 1;
        bytes[1..].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

fn decode_endpoint(bytes: &[u8]) -> Result<Option<u64>, CasperError> {
    match bytes {
        [0, 0, 0, 0, 0, 0, 0, 0, 0] => Ok(None),
        [1, rest @ ..] => Ok(Some(u64::from_be_bytes(
            rest.try_into()
                .map_err(|_| invalid("invalid endpoint width"))?,
        ))),
        _ => Err(invalid("noncanonical allowance endpoint")),
    }
}

impl OfferedGrantRecord {
    pub fn from_verified_creation(
        verified: &VerifiedGrantCreationV1,
        budget: &HostWorkBudget,
    ) -> Result<Self, CasperError> {
        let creation = verified.creation();
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            creation.encoded().len().saturating_add(65),
        )?;
        reserve(budget, HostWorkDimension::SearchStateBytes, RECORD_BYTES)?;
        let principal = rholang::rust::interpreter::accounting::principal_ground_v61(
            creation.issuer_public_key(),
        );
        let payer = vault_payer(&CostSignature {
            value: Some(CostSignatureValue::Ground(principal)),
        })
        .map_err(|error| invalid(&format!("grant issuer has no payable owner: {error}")))?;
        if payer.custody_key != creation.custody() {
            return Err(invalid("grant issuer does not own declared custody"));
        }
        let record = Self {
            authority_version: creation.authority_version(),
            cumulative_ceiling: creation.cumulative_ceiling(),
            consumed: 0,
            valid_from: creation.valid_from(),
            valid_until: creation.valid_until(),
            issuer_public_key: creation
                .issuer_public_key()
                .try_into()
                .map_err(|_| invalid("invalid grant issuer public key width"))?,
            delegate_public_key: creation
                .delegate_public_key()
                .try_into()
                .map_err(|_| invalid("invalid grant delegate public key width"))?,
            custody: creation.custody(),
            context: grant_context_hash(
                creation.network(),
                creation.shard(),
                &creation.schedule_commitment(),
            ),
            max_draw: creation.max_draw(),
        };
        check_record(record)?;
        Ok(record)
    }

    pub fn encode(self) -> Result<[u8; RECORD_BYTES], CasperError> {
        check_record(self)?;
        let mut bytes = [0; RECORD_BYTES];
        bytes[0] = RECORD_VERSION;
        bytes[1..9].copy_from_slice(&self.authority_version.to_be_bytes());
        bytes[9..25].copy_from_slice(&self.cumulative_ceiling.to_be_bytes());
        bytes[25..41].copy_from_slice(&self.consumed.to_be_bytes());
        bytes[41..50].copy_from_slice(&endpoint(self.valid_from));
        bytes[50..59].copy_from_slice(&endpoint(self.valid_until));
        bytes[59..124].copy_from_slice(&self.issuer_public_key);
        bytes[124..189].copy_from_slice(&self.delegate_public_key);
        bytes[189..221].copy_from_slice(&self.custody);
        bytes[221..253].copy_from_slice(&self.context);
        bytes[253..269].copy_from_slice(&self.max_draw.to_be_bytes());
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CasperError> {
        if bytes.len() != RECORD_BYTES || bytes[0] != RECORD_VERSION {
            return Err(invalid("invalid allowance record encoding"));
        }
        let record = Self {
            authority_version: u64::from_be_bytes(bytes[1..9].try_into().unwrap()),
            cumulative_ceiling: u128::from_be_bytes(bytes[9..25].try_into().unwrap()),
            consumed: u128::from_be_bytes(bytes[25..41].try_into().unwrap()),
            valid_from: decode_endpoint(&bytes[41..50])?,
            valid_until: decode_endpoint(&bytes[50..59])?,
            issuer_public_key: bytes[59..124].try_into().unwrap(),
            delegate_public_key: bytes[124..189].try_into().unwrap(),
            custody: bytes[189..221].try_into().unwrap(),
            context: bytes[221..253].try_into().unwrap(),
            max_draw: u128::from_be_bytes(bytes[253..269].try_into().unwrap()),
        };
        check_record(record)?;
        Ok(record)
    }
}

fn decode_data(
    data: &[Datum<ListParWithRandom>],
    kind: GrantValueKind,
) -> Result<Option<&[u8]>, CasperError> {
    let [value] = data else {
        return if data.is_empty() {
            Ok(None)
        } else {
            Err(invalid("multiple stored values"))
        };
    };
    let [par] = value.a.pars.as_slice() else {
        return Err(invalid("malformed stored value"));
    };
    let [models::rhoapi::Expr {
        expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(bytes)),
    }] = par.exprs.as_slice()
    else {
        return Err(invalid("stored value is not bytes"));
    };
    if value.persist || value.a != datum(bytes) {
        return Err(invalid("noncanonical stored value"));
    }
    match kind {
        GrantValueKind::Allowance => {
            OfferedGrantRecord::decode(bytes)?;
        }
        GrantValueKind::Nonce if bytes == NONCE_VALUE => {}
        GrantValueKind::Nonce => return Err(invalid("noncanonical nonce marker")),
    }
    Ok(Some(bytes))
}

impl OfferedGrantSnapshot {
    pub fn root(&self) -> [u8; 32] { self.root }

    pub fn verified_source_payers(
        &self,
        expected_root: [u8; 32],
        offer: &Cosigned<OfferedFundedDeploy>,
        authenticated_time: u64,
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<VerifiedOfferedGrantSources, CasperError> {
        if expected_root != self.root {
            return Err(invalid("stale grant state root"));
        }
        let intent = signed_grant_intent(offer, budget)?;
        if intent.grant_uses.len() > limits.uses {
            return Err(invalid("grant-use count exceeds limit"));
        }
        let signers = offer
            .selected_signers_v61()
            .map_err(|error| invalid(&format!("invalid delegate signer: {error}")))?;
        if signers.len() != 1 {
            return Err(invalid("delegated grant requires one selected signer"));
        }
        let delegate = signers[0].pk.bytes.as_ref();
        let policy = offered_funded_v6_limits().envelope.payload.funding;
        let mut matched_context = None;
        for schedule in &intent.base.controls.permitted_schedules {
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                policy.controls.wire.field_bytes,
            )?;
            let digest = schedule
                .digest(policy.controls.schedule(policy.controls.total_classes))
                .map_err(|error| invalid(&format!("invalid grant schedule: {error}")))?;
            if digest == intent.base.schedule_commitment {
                if schedule.shard != offer.data.body().shard_id.as_bytes() {
                    return Err(invalid("grant schedule shard differs from offered body"));
                }
                matched_context = Some(grant_context_hash(
                    schedule.network,
                    schedule.shard,
                    &digest,
                ));
                break;
            }
        }
        let context = matched_context.ok_or_else(|| invalid("signed grant schedule is absent"))?;
        let mut sources = BTreeMap::new();
        for use_record in &intent.grant_uses {
            let source = intent
                .base
                .sources
                .get(use_record.source_index as usize)
                .ok_or_else(|| invalid("grant source index is absent"))?;
            let allowance = allowance_slot(use_record.grant_id, limits, budget)?;
            let nonce = nonce_slot(
                use_record.grant_id,
                &use_record.operation_id,
                limits,
                budget,
            )?;
            if self.value(nonce)?.is_some() {
                return Err(invalid("grant operation ID was already used"));
            }
            let encoded = self
                .value(allowance)?
                .ok_or_else(|| invalid("grant allowance is absent"))?;
            reserve(budget, HostWorkDimension::SearchStateBytes, encoded.len())?;
            let record = OfferedGrantRecord::decode(encoded)?;
            if record.delegate_public_key.as_slice() != delegate
                || record.custody.as_slice() != source.custody()
                || record.context != context
                || record.authority_version != use_record.authority_version
                || use_record.max_draw == 0
                || use_record.max_draw > record.max_draw
                || use_record.cumulative_ceiling > record.cumulative_ceiling
                || use_record.max_draw > use_record.cumulative_ceiling
            {
                return Err(invalid("signed grant use differs from rooted authority"));
            }
            for (from, until) in [
                (record.valid_from, record.valid_until),
                (use_record.valid_from, use_record.valid_until),
            ] {
                if from.is_some_and(|from| authenticated_time < from)
                    || until.is_some_and(|until| authenticated_time > until)
                {
                    return Err(invalid("grant is not valid at authenticated time"));
                }
            }
            reserve(budget, HostWorkDimension::VerificationOperations, 65)?;
            let principal = rholang::rust::interpreter::accounting::principal_ground_v61(
                &record.issuer_public_key,
            );
            let payer = vault_payer(&CostSignature {
                value: Some(CostSignatureValue::Ground(principal)),
            })
            .map_err(|error| invalid(&format!("grant issuer has no payable owner: {error}")))?;
            if payer.custody_key != record.custody {
                return Err(invalid("rooted grant custody differs from issuer"));
            }
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                use_record
                    .grant_id
                    .len()
                    .saturating_add(size_of::<VerifiedGrantSource>()),
            )?;
            let entry = VerifiedGrantSource {
                payer,
                record,
                grant_id: use_record.grant_id.to_vec(),
                operation_id: use_record.operation_id,
                max_draw: use_record.max_draw,
                cumulative_ceiling: use_record.cumulative_ceiling,
                valid_from: use_record.valid_from,
                valid_until: use_record.valid_until,
                source_debit_cap: source.debit_cap(),
            };
            if sources.insert(use_record.source_index, entry).is_some() {
                return Err(invalid("multiple grants for one source are unsupported"));
            }
        }
        let envelope_identity = offer
            .envelope_commitment()
            .map_err(|error| invalid(&format!("invalid offered identity: {error}")))?
            .as_ref()
            .try_into()
            .map_err(|_| invalid("invalid offered identity width"))?;
        Ok(VerifiedOfferedGrantSources {
            root: self.root,
            envelope_identity,
            sources,
        })
    }

    pub fn prepare_measured_draws(
        &self,
        verified: &VerifiedOfferedGrantSources,
        authenticated_time: u64,
        measured_debits: &[(u32, u128)],
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedOfferedGrants, CasperError> {
        if verified.root != self.root || measured_debits.len() > verified.sources.len() {
            return Err(invalid("stale or incomplete measured grant sources"));
        }
        let mut used = BTreeSet::new();
        let mut draws = Vec::new();
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            measured_debits
                .len()
                .saturating_mul(size_of::<OfferedGrantDraw>()),
        )?;
        for &(index, debit) in measured_debits {
            if !used.insert(index) {
                return Err(invalid("duplicate measured grant source"));
            }
            let source = verified
                .sources
                .get(&index)
                .ok_or_else(|| invalid("measured grant source is absent"))?;
            if debit > u128::from(source.source_debit_cap) {
                return Err(invalid("measured draw exceeds signed source debit cap"));
            }
            if debit == 0 {
                continue;
            }
            draws.push(OfferedGrantDraw {
                grant_id: &source.grant_id,
                delegate_public_key: source.record.delegate_public_key,
                custody: source.record.custody,
                context: source.record.context,
                authority_version: source.record.authority_version,
                operation_id: source.operation_id,
                max_draw: source.max_draw,
                cumulative_ceiling: source.cumulative_ceiling,
                valid_from: source.valid_from,
                valid_until: source.valid_until,
                draw: debit,
            });
        }
        if verified.sources.keys().any(|index| !used.contains(index)) {
            return Err(invalid("measured grant source is missing"));
        }
        self.prepare_draws(self.root, authenticated_time, &draws, limits, budget)
    }

    fn value(&self, slot: GrantSlot) -> Result<Option<&[u8]>, CasperError> {
        self.entries
            .binary_search_by_key(&slot, |entry| entry.0)
            .map(|index| self.entries[index].1.as_deref())
            .map_err(|_| invalid("grant key was not captured at the state root"))
    }

    pub fn prepare_issuance(
        &self,
        expected_root: [u8; 32],
        verified: &VerifiedGrantCreationV1,
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedOfferedGrants, CasperError> {
        self.prepare_issuances(
            expected_root,
            std::slice::from_ref(verified),
            limits,
            budget,
        )
    }

    pub fn prepare_issuances(
        &self,
        expected_root: [u8; 32],
        verified: &[VerifiedGrantCreationV1],
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedOfferedGrants, CasperError> {
        if expected_root != self.root || verified.len() > limits.uses {
            return Err(invalid(
                "stale grant state root or issuance count exceeds limit",
            ));
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(verified.len().saturating_mul(2))
            .map_err(|_| invalid("grant issuance allocation failed"))?;
        for authority in verified {
            let creation = authority.creation();
            let allowance = allowance_slot(creation.grant_id(), limits, budget)?;
            let nonce = nonce_slot(
                creation.grant_id(),
                &creation.operation_id(),
                limits,
                budget,
            )?;
            if self.value(allowance)?.is_some() || self.value(nonce)?.is_some() {
                return Err(invalid(
                    "grant identity or issuance operation was already used",
                ));
            }
            let record = OfferedGrantRecord::from_verified_creation(authority, budget)?;
            changes.push(OfferedGrantChange {
                slot: allowance,
                expected: None,
                replacement: record.encode()?.to_vec(),
            });
            changes.push(OfferedGrantChange {
                slot: nonce,
                expected: None,
                replacement: NONCE_VALUE.to_vec(),
            });
        }
        changes.sort_by_key(|change| change.slot);
        if changes.windows(2).any(|pair| pair[0].slot == pair[1].slot) {
            return Err(invalid(
                "grant issuance batch repeats an identity or operation",
            ));
        }
        let bytes = changes
            .iter()
            .try_fold(0usize, |sum, change| {
                sum.checked_add(size_of::<OfferedGrantChange>())
                    .and_then(|sum| sum.checked_add(change.replacement.len()))
            })
            .ok_or_else(|| invalid("grant issuance size overflow"))?;
        if bytes > limits.batch_bytes {
            return Err(invalid("grant issuance byte limit exceeded"));
        }
        reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        Ok(PreparedOfferedGrants {
            root: self.root,
            changes,
        })
    }

    pub(crate) fn prepare_draws(
        &self,
        expected_root: [u8; 32],
        authenticated_time: u64,
        draws: &[OfferedGrantDraw<'_>],
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedOfferedGrants, CasperError> {
        if expected_root != self.root {
            return Err(invalid("stale grant state root"));
        }
        if draws.len() > limits.uses {
            return Err(invalid("grant-use count exceeds limit"));
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            draws.len().saturating_mul(2),
        )?;
        let mut allowances: BTreeMap<GrantSlot, (Vec<u8>, OfferedGrantRecord)> = BTreeMap::new();
        let mut used_nonces = BTreeSet::new();
        for draw in draws {
            let allowance = allowance_slot(draw.grant_id, limits, budget)?;
            let nonce = nonce_slot(draw.grant_id, &draw.operation_id, limits, budget)?;
            if !used_nonces.insert(nonce) || self.value(nonce)?.is_some() {
                return Err(invalid("grant operation ID was already used"));
            }
            if !allowances.contains_key(&allowance) {
                let old = self
                    .value(allowance)?
                    .ok_or_else(|| invalid("grant allowance is absent"))?;
                reserve(budget, HostWorkDimension::SearchStateBytes, old.len())?;
                allowances.insert(allowance, (old.to_vec(), OfferedGrantRecord::decode(old)?));
            }
            let (_, record) = allowances.get_mut(&allowance).unwrap();
            if draw.authority_version != record.authority_version {
                return Err(invalid("stale grant authority version"));
            }
            if draw.custody != record.custody
                || draw.context != record.context
                || draw.delegate_public_key != record.delegate_public_key
            {
                return Err(invalid("grant delegate, custody, or context differs"));
            }
            if draw.draw == 0
                || draw.max_draw == 0
                || draw.draw > draw.max_draw
                || draw.max_draw > record.max_draw
                || draw.max_draw > draw.cumulative_ceiling
                || draw.cumulative_ceiling > record.cumulative_ceiling
                || draw
                    .valid_from
                    .zip(draw.valid_until)
                    .is_some_and(|(from, until)| from > until)
            {
                return Err(invalid("invalid grant draw bounds"));
            }
            for (from, until) in [
                (record.valid_from, record.valid_until),
                (draw.valid_from, draw.valid_until),
            ] {
                if from.is_some_and(|from| authenticated_time < from)
                    || until.is_some_and(|until| authenticated_time > until)
                {
                    return Err(invalid("grant is not valid at authenticated time"));
                }
            }
            record.consumed = record
                .consumed
                .checked_add(draw.draw)
                .ok_or_else(|| invalid("grant cumulative draw overflow"))?;
            if record.consumed > draw.cumulative_ceiling {
                return Err(invalid("grant cumulative ceiling exceeded"));
            }
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(allowances.len().saturating_add(used_nonces.len()))
            .map_err(|_| invalid("grant transition allocation failed"))?;
        for (slot, (old, record)) in allowances {
            changes.push(OfferedGrantChange {
                slot,
                expected: Some(old),
                replacement: record.encode()?.to_vec(),
            });
        }
        for slot in used_nonces {
            changes.push(OfferedGrantChange {
                slot,
                expected: None,
                replacement: NONCE_VALUE.to_vec(),
            });
        }
        changes.sort_by_key(|change| change.slot);
        let bytes = changes
            .iter()
            .try_fold(0usize, |sum, change| {
                sum.checked_add(size_of::<OfferedGrantChange>())
                    .and_then(|sum| sum.checked_add(change.expected.as_ref().map_or(0, Vec::len)))
                    .and_then(|sum| sum.checked_add(change.replacement.len()))
            })
            .ok_or_else(|| invalid("grant transition size overflow"))?;
        if bytes > limits.batch_bytes {
            return Err(invalid("grant transition byte limit exceeded"));
        }
        reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        Ok(PreparedOfferedGrants {
            root: self.root,
            changes,
        })
    }
}

impl RuntimeManager {
    pub fn capture_offered_grants_from_signed(
        &self,
        pre_state_root: [u8; 32],
        offer: &Cosigned<OfferedFundedDeploy>,
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<OfferedGrantSnapshot, CasperError> {
        let intent = signed_grant_intent(offer, budget)?;
        if intent.grant_uses.len() > limits.uses {
            return Err(invalid("grant-use count exceeds limit"));
        }
        let slots_len = intent
            .grant_uses
            .len()
            .checked_mul(2)
            .ok_or_else(|| invalid("grant snapshot count overflow"))?;
        let bytes = slots_len
            .checked_mul(
                size_of::<GrantSlot>() + size_of::<(GrantSlot, Option<Vec<u8>>)>() + RECORD_BYTES,
            )
            .ok_or_else(|| invalid("grant snapshot size overflow"))?;
        if bytes > limits.batch_bytes {
            return Err(invalid("grant snapshot byte limit exceeded"));
        }
        reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(slots_len)
            .map_err(|_| invalid("grant snapshot allocation failed"))?;
        for use_record in &intent.grant_uses {
            slots.push(allowance_slot(use_record.grant_id, limits, budget)?);
            slots.push(nonce_slot(
                use_record.grant_id,
                &use_record.operation_id,
                limits,
                budget,
            )?);
        }
        self.capture_offered_slots(pre_state_root, slots, budget)
    }

    pub fn capture_offered_grants(
        &self,
        pre_state_root: [u8; 32],
        draws: &[OfferedGrantDraw<'_>],
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<OfferedGrantSnapshot, CasperError> {
        if draws.len() > limits.uses {
            return Err(invalid("grant-use count exceeds limit"));
        }
        let slots_len = draws
            .len()
            .checked_mul(2)
            .ok_or_else(|| invalid("grant snapshot count overflow"))?;
        let bytes = slots_len
            .checked_mul(
                size_of::<GrantSlot>() + size_of::<(GrantSlot, Option<Vec<u8>>)>() + RECORD_BYTES,
            )
            .ok_or_else(|| invalid("grant snapshot size overflow"))?;
        if bytes > limits.batch_bytes {
            return Err(invalid("grant snapshot byte limit exceeded"));
        }
        reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(slots_len)
            .map_err(|_| invalid("grant snapshot allocation failed"))?;
        for draw in draws {
            slots.push(allowance_slot(draw.grant_id, limits, budget)?);
            slots.push(nonce_slot(
                draw.grant_id,
                &draw.operation_id,
                limits,
                budget,
            )?);
        }
        self.capture_offered_slots(pre_state_root, slots, budget)
    }

    pub fn capture_offered_grant_issuance(
        &self,
        pre_state_root: [u8; 32],
        verified: &VerifiedGrantCreationV1,
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<OfferedGrantSnapshot, CasperError> {
        self.capture_offered_grant_issuances(
            pre_state_root,
            std::slice::from_ref(verified),
            limits,
            budget,
        )
    }

    pub fn capture_offered_grant_issuances(
        &self,
        pre_state_root: [u8; 32],
        verified: &[VerifiedGrantCreationV1],
        limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<OfferedGrantSnapshot, CasperError> {
        if verified.len() > limits.uses {
            return Err(invalid("grant issuance count exceeds limit"));
        }
        let slots_len = verified
            .len()
            .checked_mul(2)
            .ok_or_else(|| invalid("grant snapshot count overflow"))?;
        let bytes = slots_len
            .checked_mul(
                size_of::<GrantSlot>() + size_of::<(GrantSlot, Option<Vec<u8>>)>() + RECORD_BYTES,
            )
            .ok_or_else(|| invalid("grant snapshot size overflow"))?;
        if bytes > limits.batch_bytes {
            return Err(invalid("grant snapshot byte limit exceeded"));
        }
        reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(slots_len)
            .map_err(|_| invalid("grant snapshot allocation failed"))?;
        for authority in verified {
            let creation = authority.creation();
            slots.push(allowance_slot(creation.grant_id(), limits, budget)?);
            slots.push(nonce_slot(
                creation.grant_id(),
                &creation.operation_id(),
                limits,
                budget,
            )?);
        }
        self.capture_offered_slots(pre_state_root, slots, budget)
    }

    fn capture_offered_slots(
        &self,
        pre_state_root: [u8; 32],
        mut slots: Vec<GrantSlot>,
        budget: &HostWorkBudget,
    ) -> Result<OfferedGrantSnapshot, CasperError> {
        let slots_len = slots.len();
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            slots_len.saturating_mul(1 + slots_len.checked_ilog2().unwrap_or(0) as usize),
        )?;
        slots.sort_unstable();
        slots.dedup();
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        if !self
            .history_repo
            .contains_root(&root)
            .map_err(|error| invalid(&format!("grant root lookup failed: {error}")))?
        {
            return Err(invalid("grant snapshot requires a registered state root"));
        }
        let reader = self
            .history_repo
            .get_history_reader(&root)
            .map_err(|error| invalid(&format!("grant root read failed: {error}")))?;
        if reader.root() != root {
            return Err(invalid("grant snapshot reader returned another root"));
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(slots.len())
            .map_err(|_| invalid("grant entry allocation failed"))?;
        for slot in slots {
            reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
            let data = reader
                .get_data(&stable_hash_provider::hash(&channel(slot)))
                .map_err(|error| invalid(&format!("grant snapshot read failed: {error}")))?;
            let value = decode_data(&data, slot.kind)?;
            if let Some(value) = value {
                reserve(budget, HostWorkDimension::SearchStateBytes, value.len())?;
                reserve(
                    budget,
                    HostWorkDimension::VerificationOperations,
                    value.len(),
                )?;
            }
            entries.push((slot, value.map(<[u8]>::to_vec)));
        }
        if reader.root() != root {
            return Err(invalid("grant snapshot reader changed its root"));
        }
        Ok(OfferedGrantSnapshot {
            root: pre_state_root,
            entries,
        })
    }
}

fn replacement_id(change: &OfferedGrantChange) -> Vec<u8> {
    Blake2b256::hash_stream(|feed| {
        feed(REPLACEMENT_DOMAIN);
        feed(&change.slot.key);
        for value in [
            change.expected.as_deref(),
            Some(change.replacement.as_slice()),
        ] {
            match value {
                None => feed(&[0]),
                Some(bytes) => {
                    feed(&[1]);
                    feed(&(bytes.len() as u64).to_be_bytes());
                    feed(bytes);
                }
            }
        }
    })
}

impl RuntimeOps {
    pub async fn collect_grant_issue_requests(
        &mut self,
        evaluation_succeeded: bool,
        execution_log: &[Event],
        expected_network: &[u8],
        expected_shard: &[u8],
        call_limits: GrantIssueCallLimits,
        grant_limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<Vec<VerifiedGrantCreationV1>, CasperError> {
        if !evaluation_succeeded {
            return Err(invalid("failed user execution cannot issue a grant"));
        }
        let observed_count = count_grant_issue_requests(execution_log, grant_limits, budget)?;
        self.drain_grant_issue_requests(
            observed_count,
            expected_network,
            expected_shard,
            call_limits,
            grant_limits,
            budget,
        )
        .await
    }

    async fn drain_grant_issue_requests(
        &mut self,
        observed_count: usize,
        expected_network: &[u8],
        expected_shard: &[u8],
        call_limits: GrantIssueCallLimits,
        grant_limits: OfferedGrantLimits,
        budget: &HostWorkBudget,
    ) -> Result<Vec<VerifiedGrantCreationV1>, CasperError> {
        if observed_count > grant_limits.uses {
            return Err(invalid("grant issue request count exceeds limit"));
        }
        let mut verified = Vec::new();
        verified
            .try_reserve_exact(observed_count)
            .map_err(|_| invalid("grant issue request allocation failed"))?;
        for _ in 0..observed_count {
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                call_limits.wire.total_bytes,
            )?;
            let (_, values) = self
                .consume_result(grant_issue_journal_channel(), BindPattern {
                    patterns: vec![new_freevar_par(0, Vec::new())],
                    remainder: None,
                    free_count: 1,
                })
                .await?
                .ok_or_else(|| invalid("observed grant issue request is absent"))?;
            let [value] = values.as_slice() else {
                return Err(invalid("grant issue request has multiple values"));
            };
            let [payload] = value.pars.as_slice() else {
                return Err(invalid("grant issue request has malformed payload"));
            };
            let [models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(call)),
            }] = payload.exprs.as_slice()
            else {
                return Err(invalid("grant issue request is not bytes"));
            };
            if call.len() > call_limits.wire.total_bytes || *value != datum(call) {
                return Err(invalid("grant issue request is noncanonical"));
            }
            verified.push(decode_verified_grant_issue_call(
                call,
                expected_network,
                expected_shard,
                call_limits,
                budget,
            )?);
        }
        reserve(
            budget,
            HostWorkDimension::VerificationOperations,
            observed_count.saturating_mul(1 + observed_count.checked_ilog2().unwrap_or(0) as usize),
        )?;
        verified.sort_by(|a, b| {
            a.creation()
                .grant_id()
                .cmp(b.creation().grant_id())
                .then_with(|| {
                    a.creation()
                        .operation_id()
                        .cmp(&b.creation().operation_id())
                })
        });
        if verified
            .windows(2)
            .any(|pair| pair[0].creation().grant_id() == pair[1].creation().grant_id())
        {
            return Err(invalid("grant issue batch repeats a grant identity"));
        }
        Ok(verified)
    }

    pub async fn apply_offered_grant_transitions(
        &mut self,
        prepared: &PreparedOfferedGrants,
        authenticated_funding_root: [u8; 32],
        limits: OfferedGrantLimits,
    ) -> Result<Log, CasperError> {
        if authenticated_funding_root != prepared.root {
            return Err(invalid("grant proof belongs to another funding root"));
        }
        if prepared.changes.len() > limits.uses.saturating_mul(2) {
            return Err(invalid("grant transition count exceeds limit"));
        }
        let mut bytes = 0usize;
        for change in &prepared.changes {
            bytes = bytes
                .checked_add(size_of::<OfferedGrantChange>())
                .and_then(|sum| sum.checked_add(change.expected.as_ref().map_or(0, Vec::len)))
                .and_then(|sum| sum.checked_add(change.replacement.len()))
                .ok_or_else(|| invalid("grant transition size overflow"))?;
            match change.slot.kind {
                GrantValueKind::Allowance => {
                    OfferedGrantRecord::decode(&change.replacement)?;
                    if let Some(old) = &change.expected {
                        OfferedGrantRecord::decode(old)?;
                    }
                }
                GrantValueKind::Nonce
                    if change.expected.is_none() && change.replacement == NONCE_VALUE => {}
                GrantValueKind::Nonce => return Err(invalid("invalid nonce transition")),
            }
        }
        if bytes > limits.batch_bytes
            || prepared
                .changes
                .windows(2)
                .any(|pair| pair[0].slot >= pair[1].slot)
        {
            return Err(invalid("grant transition limit or canonical order failed"));
        }
        for change in &prepared.changes {
            let data = self
                .runtime
                .reducer
                .space
                .get_data(&channel(change.slot))
                .await;
            if decode_data(&data, change.slot.kind)? != change.expected.as_deref() {
                return Err(invalid("grant value differs from captured root"));
            }
        }
        if prepared.changes.is_empty() {
            return Ok(Vec::new());
        }
        let checkpoint = self.runtime.create_soft_checkpoint().await;
        let result = async {
            for change in &prepared.changes {
                let name = channel(change.slot);
                if change.expected.is_some() {
                    self.runtime
                        .reducer
                        .space
                        .remove_data_at_recorded(&name, 0, &replacement_id(change))
                        .await
                        .map_err(|error| {
                            invalid(&format!("recorded grant removal failed: {error}"))
                        })?;
                }
                let matched = self
                    .runtime
                    .reducer
                    .space
                    .produce(name, datum(&change.replacement), false)
                    .await
                    .map_err(|error| invalid(&format!("grant replacement failed: {error}")))?;
                if matched.is_some() {
                    return Err(invalid("protected grant matched a continuation"));
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            self.runtime.revert_to_soft_checkpoint(checkpoint).await;
            return Err(error);
        }
        let mut log = checkpoint.log;
        log.extend(self.runtime.take_event_log().await);
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use models::rust::phlo_grant_creation::PhloGrantCreationTerms;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rspace_plus_plus::rspace::rspace::RSpaceStore;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    const LIMITS: OfferedGrantLimits = OfferedGrantLimits {
        uses: 4,
        grant_id_bytes: 64,
        batch_bytes: 8192,
    };

    fn budget() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
    }

    fn key(byte: u8) -> [u8; 65] {
        Secp256k1
            .to_public(&PrivateKey::from_bytes(&[byte; 32]))
            .bytes
            .as_ref()
            .try_into()
            .unwrap()
    }

    fn creation_limits() -> GrantIssueCallLimits {
        GrantIssueCallLimits {
            wire: PhloWireLimits {
                total_bytes: 2048,
                field_bytes: 1024,
            },
            creation: PhloGrantCreationLimits {
                wire: PhloWireLimits {
                    total_bytes: 1024,
                    field_bytes: 256,
                },
                grant_id_bytes: 64,
            },
        }
    }

    fn signed_creation() -> Cosigned<PhloGrantCreationV1> {
        let issuer = key(1);
        let principal = rholang::rust::interpreter::accounting::principal_ground_v61(&issuer);
        let payer = vault_payer(&CostSignature {
            value: Some(CostSignatureValue::Ground(principal)),
        })
        .unwrap();
        let creation = PhloGrantCreationV1::new(
            PhloGrantCreationTerms {
                grant_id: b"grant",
                issuer_public_key: &issuer,
                delegate_public_key: &key(2),
                custody: payer.custody_key,
                network: b"network",
                shard: b"shard",
                schedule_commitment: [4; 32],
                authority_version: 7,
                max_draw: 10,
                cumulative_ceiling: 100,
                valid_from: Some(10),
                valid_until: Some(20),
                operation_id: [5; 32],
            },
            creation_limits().creation,
        )
        .unwrap();
        Cosigned::create_single_envelope(
            creation,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    #[test]
    fn signed_grant_issue_call_is_canonical_and_owner_authenticated() {
        let envelope = signed_creation();
        let encoded = encode_grant_issue_call(&envelope, creation_limits()).unwrap();
        let verified = decode_verified_grant_issue_call(
            &encoded,
            b"network",
            b"shard",
            creation_limits(),
            &budget(),
        )
        .unwrap();
        assert_eq!(verified.creation(), &envelope.data);
        assert!(decode_verified_grant_issue_call(
            &encoded,
            b"another-network",
            b"shard",
            creation_limits(),
            &budget(),
        )
        .is_err());
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_verified_grant_issue_call(
            &trailing,
            b"network",
            b"shard",
            creation_limits(),
            &budget(),
        )
        .is_err());
        let mut tampered = encoded;
        *tampered.last_mut().unwrap() ^= 1;
        assert!(decode_verified_grant_issue_call(
            &tampered,
            b"network",
            b"shard",
            creation_limits(),
            &budget(),
        )
        .is_err());
    }

    fn record() -> OfferedGrantRecord {
        OfferedGrantRecord {
            authority_version: 7,
            cumulative_ceiling: 100,
            consumed: 12,
            valid_from: Some(10),
            valid_until: Some(20),
            issuer_public_key: key(1),
            delegate_public_key: key(2),
            custody: [3; 32],
            context: [4; 32],
            max_draw: 10,
        }
    }

    fn draw<'a>(grant_id: &'a [u8], operation_id: [u8; 32]) -> OfferedGrantDraw<'a> {
        OfferedGrantDraw {
            grant_id,
            delegate_public_key: key(2),
            custody: [3; 32],
            context: [4; 32],
            authority_version: 7,
            operation_id,
            max_draw: 9,
            cumulative_ceiling: 100,
            valid_from: Some(10),
            valid_until: Some(20),
            draw: 9,
        }
    }

    async fn fixture() -> (RuntimeManager, RuntimeOps) {
        let stores = RSpaceStore {
            history: Arc::new(InMemoryKeyValueStore::new()),
            roots: Arc::new(InMemoryKeyValueStore::new()),
            cold: Arc::new(InMemoryKeyValueStore::new()),
        };
        let (manager, _) = RuntimeManager::create_with_history(
            stores,
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let runtime = RuntimeOps::new(manager.spawn_runtime().await.unwrap());
        (manager, runtime)
    }

    async fn seeded() -> (RuntimeManager, RuntimeOps, [u8; 32]) {
        let (manager, mut runtime) = fixture().await;
        let allowance = allowance_slot(b"grant", LIMITS, &budget()).unwrap();
        assert!(runtime
            .runtime
            .reducer
            .space
            .produce(
                channel(allowance),
                datum(&record().encode().unwrap()),
                false
            )
            .await
            .unwrap()
            .is_none());
        let root = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .unwrap();
        (manager, runtime, root)
    }

    #[tokio::test]
    async fn grant_transition_accepts_user_root_advance_and_rejects_foreign_or_stale_state() {
        let (manager, mut runtime, funding_root) = seeded().await;
        let use_record = draw(b"grant", [8; 32]);
        let snapshot = manager
            .capture_offered_grants(funding_root, &[use_record], LIMITS, &budget())
            .unwrap();
        let prepared = snapshot
            .prepare_draws(funding_root, 15, &[use_record], LIMITS, &budget())
            .unwrap();
        assert!(runtime
            .runtime
            .reducer
            .space
            .produce(
                RhoByteArray::create_par(b"unrelated-user-state".to_vec()),
                datum(b"value"),
                false,
            )
            .await
            .unwrap()
            .is_none());
        let settlement_root: [u8; 32] = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .unwrap();
        assert_ne!(settlement_root, funding_root);
        assert!(runtime
            .apply_offered_grant_transitions(&prepared, [0; 32], LIMITS)
            .await
            .is_err());
        assert_eq!(
            runtime.runtime.get_root().await.bytes().as_slice(),
            settlement_root.as_slice()
        );
        runtime
            .apply_offered_grant_transitions(&prepared, funding_root, LIMITS)
            .await
            .unwrap();
        assert!(runtime
            .apply_offered_grant_transitions(&prepared, funding_root, LIMITS)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn signed_issuance_is_rooted_once_only_and_outer_rollback_restores_absence() {
        let (manager, mut runtime) = fixture().await;
        let root: [u8; 32] = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .unwrap();
        let signed = signed_creation();
        let verified = decode_verified_grant_issue_call(
            &encode_grant_issue_call(&signed, creation_limits()).unwrap(),
            b"network",
            b"shard",
            creation_limits(),
            &budget(),
        )
        .unwrap();
        let snapshot = manager
            .capture_offered_grant_issuance(root, &verified, LIMITS, &budget())
            .unwrap();
        let prepared = snapshot
            .prepare_issuance(root, &verified, LIMITS, &budget())
            .unwrap();
        assert!(snapshot
            .prepare_issuance([0; 32], &verified, LIMITS, &budget())
            .is_err());
        let outer = runtime.runtime.create_soft_checkpoint().await;
        runtime
            .apply_offered_grant_transitions(&prepared, root, LIMITS)
            .await
            .unwrap();
        assert!(runtime
            .apply_offered_grant_transitions(&prepared, root, LIMITS)
            .await
            .is_err());
        runtime.runtime.revert_to_soft_checkpoint(outer).await;
        let reverted: [u8; 32] = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .unwrap();
        assert_eq!(reverted, root);
        assert!(manager
            .capture_offered_grant_issuance(reverted, &verified, LIMITS, &budget())
            .unwrap()
            .prepare_issuance(reverted, &verified, LIMITS, &budget())
            .is_ok());
    }

    #[tokio::test]
    async fn grant_issue_collector_drains_exact_observed_count_only_after_success() {
        let (_, mut runtime) = fixture().await;
        let call = encode_grant_issue_call(&signed_creation(), creation_limits()).unwrap();
        assert!(runtime
            .runtime
            .reducer
            .space
            .produce(grant_issue_journal_channel(), datum(&call), false)
            .await
            .unwrap()
            .is_none());
        let execution_log = runtime.runtime.take_event_log().await;
        assert_eq!(
            count_grant_issue_requests(&execution_log, LIMITS, &budget()).unwrap(),
            1
        );
        assert!(runtime
            .collect_grant_issue_requests(
                false,
                &execution_log,
                b"network",
                b"shard",
                creation_limits(),
                LIMITS,
                &budget(),
            )
            .await
            .is_err());
        assert_eq!(
            runtime
                .runtime
                .reducer
                .space
                .get_data(&grant_issue_journal_channel())
                .await
                .len(),
            1
        );
        let verified = runtime
            .collect_grant_issue_requests(
                true,
                &execution_log,
                b"network",
                b"shard",
                creation_limits(),
                LIMITS,
                &budget(),
            )
            .await
            .unwrap();
        assert_eq!(verified.len(), 1);
        assert!(runtime
            .runtime
            .reducer
            .space
            .get_data(&grant_issue_journal_channel())
            .await
            .is_empty());
        assert!(!runtime.runtime.take_event_log().await.is_empty());
    }

    #[test]
    fn allowance_record_has_one_canonical_bounded_encoding() {
        let encoded = record().encode().unwrap();
        assert_eq!(OfferedGrantRecord::decode(&encoded).unwrap(), record());
        let mut malformed = encoded;
        malformed[0] = 2;
        assert!(OfferedGrantRecord::decode(&malformed).is_err());
        malformed = encoded;
        malformed[41] = 0;
        assert!(OfferedGrantRecord::decode(&malformed).is_err());
        malformed = encoded;
        malformed[25..41].copy_from_slice(&101u128.to_be_bytes());
        assert!(OfferedGrantRecord::decode(&malformed).is_err());
    }

    #[tokio::test]
    async fn rooted_grant_draw_enforces_version_time_draw_ceiling_and_once_only_nonce() {
        let (manager, mut runtime, root) = seeded().await;
        let use_one = draw(b"grant", [1; 32]);
        let snapshot = manager
            .capture_offered_grants(root, &[use_one], LIMITS, &budget())
            .unwrap();
        assert!(snapshot
            .prepare_draws([0; 32], 10, &[use_one], LIMITS, &budget())
            .is_err());
        assert!(snapshot
            .prepare_draws(root, 9, &[use_one], LIMITS, &budget())
            .is_err());
        assert!(snapshot
            .prepare_draws(root, 21, &[use_one], LIMITS, &budget())
            .is_err());
        let mut stale_version = use_one;
        stale_version.authority_version = 6;
        assert!(snapshot
            .prepare_draws(root, 15, &[stale_version], LIMITS, &budget())
            .is_err());
        let mut excessive = use_one;
        excessive.draw = 10;
        assert!(snapshot
            .prepare_draws(root, 15, &[excessive], LIMITS, &budget())
            .is_err());
        let mut ceiling = use_one;
        ceiling.cumulative_ceiling = 20;
        assert!(snapshot
            .prepare_draws(root, 15, &[ceiling], LIMITS, &budget())
            .is_err());
        assert!(snapshot
            .prepare_draws(root, 15, &[use_one, use_one], LIMITS, &budget())
            .is_err());
        let prepared = snapshot
            .prepare_draws(root, 10, &[use_one], LIMITS, &budget())
            .unwrap();
        assert_eq!(prepared.root(), root);
        assert_eq!(prepared.changes().len(), 2);
        let allowance = prepared
            .changes()
            .iter()
            .find(|change| change.slot.kind == GrantValueKind::Allowance)
            .unwrap();
        assert_eq!(
            OfferedGrantRecord::decode(allowance.expected().unwrap()).unwrap(),
            record()
        );
        assert_eq!(
            OfferedGrantRecord::decode(allowance.replacement())
                .unwrap()
                .consumed,
            21
        );
        runtime
            .apply_offered_grant_transitions(&prepared, root, LIMITS)
            .await
            .unwrap();
        let after: [u8; 32] = runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .unwrap();
        assert_ne!(root, after);
        assert!(runtime
            .apply_offered_grant_transitions(&prepared, root, LIMITS)
            .await
            .is_err());
        let replayed = manager
            .capture_offered_grants(after, &[use_one], LIMITS, &budget())
            .unwrap();
        assert!(replayed
            .prepare_draws(after, 15, &[use_one], LIMITS, &budget())
            .is_err());
    }

    #[tokio::test]
    async fn stale_candidate_cannot_consume_after_independent_grant_change() {
        let (manager, mut runtime, root) = seeded().await;
        let first = draw(b"grant", [1; 32]);
        let second = draw(b"grant", [2; 32]);
        let snapshot = manager
            .capture_offered_grants(root, &[first, second], LIMITS, &budget())
            .unwrap();
        let accepted = snapshot
            .prepare_draws(root, 15, &[first], LIMITS, &budget())
            .unwrap();
        let stale = snapshot
            .prepare_draws(root, 15, &[second], LIMITS, &budget())
            .unwrap();
        runtime
            .apply_offered_grant_transitions(&accepted, root, LIMITS)
            .await
            .unwrap();
        assert!(runtime
            .apply_offered_grant_transitions(&stale, root, LIMITS)
            .await
            .is_err());
    }
}

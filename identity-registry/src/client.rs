// Thin wrapper over F1r3flyApi for invoking the identity registry contract
// operations.

use std::time::Duration;

use node_cli::{DeployOptions, Endpoint, F1r3flyApi, ProposeResult, SigningKey};
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use crate::error::IdentityRegistryError;
use crate::payload::{
    admin_relink_payload, register_payload, rotate_payload, NAME_LEN, NONCE_LEN, PK_LEN,
};
use crate::reply::{
    extract_elist, extract_emap, extract_gbytearray, extract_gint, extract_gstring, fetch_reply,
    parse_tagged_reply,
};
use crate::signing::{admin_sign, sign_payload, signature_der, uncompressed_pk_bytes};
use crate::term::{
    admin_relink_term, new_reply_slot, register_term, resolve_current_term, resolve_history_term,
    rotate_term,
};

/// What `resolve_current` returns on success.
#[derive(Debug, Clone)]
pub struct ResolvedCurrent {
    pub pk: [u8; PK_LEN],
    pub seq: u64,
    /// Block height at which the name was minted.
    pub block_height: u64,
}

/// A single entry in the identity's append-only history list.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub pk: [u8; PK_LEN],
    pub op: String,
    pub seq: u64,
    /// Block height at which this entry was recorded.
    pub block_height: u64,
    pub admin_evidence: Option<Vec<u8>>,
    pub admin_signers: Option<Vec<[u8; PK_LEN]>>,
}

pub struct IdentityRegistryClient {
    pub api: F1r3flyApi,
    pub node_host: String,
    pub grpc_port: u16,
    pub registry_uri: Option<String>,
    /// Opt-in delay after every deploy. Lets the caller force consecutive
    /// deploys into separate blocks when the cluster would otherwise batch
    /// them. Defaults to zero.
    pub post_deploy_wait: Duration,
}

impl IdentityRegistryClient {
    /// Builds a client against a running F1R3FLY validator node.
    /// `registry_uri` is the registry to address, usually
    /// `rho:system:identityRegistry`.
    pub fn new(
        signing_key: &str,
        node_host: &str,
        grpc_port: u16,
        registry_uri: Option<String>,
    ) -> Result<Self, IdentityRegistryError> {
        let url = format!("http://{node_host}:{grpc_port}");
        let endpoint =
            Endpoint::parse(&url).map_err(|e| IdentityRegistryError::Node(format!("{e:?}")))?;
        let key = SigningKey::from_hex(signing_key)
            .map_err(|e| IdentityRegistryError::Node(format!("{e:?}")))?;
        let api = F1r3flyApi::new(endpoint)
            .map_err(|e| IdentityRegistryError::Node(format!("{e:?}")))?
            .with_signing_key(&key);
        Ok(Self {
            api,
            node_host: node_host.to_string(),
            grpc_port,
            registry_uri,
            post_deploy_wait: Duration::ZERO,
        })
    }

    fn require_uri(&self) -> Result<&str, IdentityRegistryError> {
        self.registry_uri
            .as_deref()
            .ok_or_else(|| IdentityRegistryError::Node("registry_uri not set".into()))
    }

    /// Signs and submits a register call for a fresh identity. Returns the
    /// derived name on success.
    pub async fn register(
        &self,
        identity_sk: &SecretKey,
        nonce: &[u8; NONCE_LEN],
    ) -> Result<[u8; NAME_LEN], IdentityRegistryError> {
        let uri = self.require_uri()?;
        let pk = PublicKey::from_secret_key(&Secp256k1::new(), identity_sk);
        let pk_bytes = uncompressed_pk_bytes(&pk);
        let payload = register_payload(&pk_bytes, nonce);
        let sig = sign_payload(&payload, identity_sk);
        let sig_der = signature_der(&sig);

        let slot = new_reply_slot();
        let term = register_term(uri, &pk_bytes, nonce, &sig_der, &slot);
        let block_hash = self.deploy(&term).await?;

        let value = parse_tagged_reply(
            fetch_reply(&self.node_host, self.grpc_port, &slot, &block_hash).await?,
        )?;
        let name_bytes = extract_gbytearray(&value)?;
        bytes_to_array::<NAME_LEN>(name_bytes, "name")
    }

    /// Signs and submits a rotate call. `new_sk` signs the payload a second
    /// time as proof the submitter holds the new key. Returns the new seq
    /// on success.
    pub async fn rotate(
        &self,
        current_sk: &SecretKey,
        name: &[u8; NAME_LEN],
        new_sk: &SecretKey,
        seq: u64,
    ) -> Result<u64, IdentityRegistryError> {
        let uri = self.require_uri()?;
        let new_pk = PublicKey::from_secret_key(&Secp256k1::new(), new_sk);
        let new_pk_bytes = uncompressed_pk_bytes(&new_pk);
        let payload = rotate_payload(name, &new_pk_bytes, seq);
        let sig = sign_payload(&payload, current_sk);
        let sig_der = signature_der(&sig);
        let new_sig = sign_payload(&payload, new_sk);
        let new_sig_der = signature_der(&new_sig);

        let slot = new_reply_slot();
        let term = rotate_term(uri, name, &new_pk_bytes, seq, &sig_der, &new_sig_der, &slot);
        let block_hash = self.deploy(&term).await?;

        let value = parse_tagged_reply(
            fetch_reply(&self.node_host, self.grpc_port, &slot, &block_hash).await?,
        )?;
        gint_as_u64(&value, "new_seq")
    }

    /// Signs the admin_relink payload with each of `admin_sks` and submits it
    /// with `admin_relink_with_sigs`. Returns the new seq.
    pub async fn admin_relink(
        &self,
        admin_sks: &[SecretKey],
        name: &[u8; NAME_LEN],
        new_sk: &SecretKey,
        seq: u64,
        admin_evidence: &[u8],
    ) -> Result<u64, IdentityRegistryError> {
        let new_pk_bytes =
            uncompressed_pk_bytes(&PublicKey::from_secret_key(&Secp256k1::new(), new_sk));
        let admin_sigs: Vec<_> = admin_sks
            .iter()
            .map(|sk| admin_sign(sk, name, &new_pk_bytes, seq, admin_evidence))
            .collect();
        self.admin_relink_with_sigs(&admin_sigs, name, new_sk, seq, admin_evidence)
            .await
    }

    /// Submits an admin_relink call with admin `(pk, sig)` pairs, for example
    /// from `admin_sign`. `new_sk` signs the payload as proof of consent
    /// (IR-D5). The deployer key only pays for the deploy. Returns the new seq.
    pub async fn admin_relink_with_sigs(
        &self,
        admin_sigs: &[([u8; PK_LEN], Vec<u8>)],
        name: &[u8; NAME_LEN],
        new_sk: &SecretKey,
        seq: u64,
        admin_evidence: &[u8],
    ) -> Result<u64, IdentityRegistryError> {
        let uri = self.require_uri()?;
        let new_pk = PublicKey::from_secret_key(&Secp256k1::new(), new_sk);
        let new_pk_bytes = uncompressed_pk_bytes(&new_pk);
        let payload = admin_relink_payload(name, &new_pk_bytes, seq, admin_evidence);
        let admin_sig_refs: Vec<(&[u8], &[u8])> = admin_sigs
            .iter()
            .map(|(pk, sig)| (pk.as_slice(), sig.as_slice()))
            .collect();
        let new_sig_der = signature_der(&sign_payload(&payload, new_sk));

        let slot = new_reply_slot();
        let term = admin_relink_term(
            uri,
            name,
            &new_pk_bytes,
            seq,
            admin_evidence,
            &admin_sig_refs,
            &new_sig_der,
            &slot,
        );
        let block_hash = self.deploy(&term).await?;

        let value = parse_tagged_reply(
            fetch_reply(&self.node_host, self.grpc_port, &slot, &block_hash).await?,
        )?;
        gint_as_u64(&value, "new_seq")
    }

    /// Resolves the current signing key and sequence for an identity.
    pub async fn resolve_current(
        &self,
        name: &[u8; NAME_LEN],
    ) -> Result<ResolvedCurrent, IdentityRegistryError> {
        let uri = self.require_uri()?;
        let slot = new_reply_slot();
        let term = resolve_current_term(uri, name, &slot);
        let block_hash = self.deploy(&term).await?;

        let value = parse_tagged_reply(
            fetch_reply(&self.node_host, self.grpc_port, &slot, &block_hash).await?,
        )?;
        // Shape of `value`: an EMap with keys "pk" (ByteArray) and "seq" (Int).
        // Deferring full EMap decoding for the prototype - see decode_resolve_current.
        decode_resolve_current(value)
    }

    /// Returns the identity's append-only history as a typed list of
    /// entries, oldest first.
    pub async fn resolve_history(
        &self,
        name: &[u8; NAME_LEN],
    ) -> Result<Vec<HistoryEntry>, IdentityRegistryError> {
        let uri = self.require_uri()?;
        let slot = new_reply_slot();
        let term = resolve_history_term(uri, name, &slot);
        let block_hash = self.deploy(&term).await?;

        let value = parse_tagged_reply(
            fetch_reply(&self.node_host, self.grpc_port, &slot, &block_hash).await?,
        )?;
        decode_history(value)
    }

    pub async fn deploy(&self, term: &str) -> Result<String, IdentityRegistryError> {
        let options = DeployOptions::default().with_phlo_limit(5_000_000_000);
        self.api
            .deploy(term, &options)
            .await
            .map_err(|e| IdentityRegistryError::Node(format!("{e:?}")))?;
        let propose_result = self
            .api
            .propose()
            .await
            .map_err(|e| IdentityRegistryError::Node(format!("{e:?}")))?;
        let block_hash = match propose_result {
            ProposeResult::Proposed(hash) => hash,
            ProposeResult::Skipped(msg) => {
                return Err(IdentityRegistryError::Node(format!(
                    "propose skipped: {msg}"
                )));
            }
        };
        if !self.post_deploy_wait.is_zero() {
            tokio::time::sleep(self.post_deploy_wait).await;
        }
        Ok(block_hash)
    }
}

fn bytes_to_array<const N: usize>(
    bytes: Vec<u8>,
    label: &str,
) -> Result<[u8; N], IdentityRegistryError> {
    if bytes.len() != N {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "expected {N}-byte {label}, got {} bytes",
            bytes.len()
        )));
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn gint_as_u64(par: &models::rhoapi::Par, label: &str) -> Result<u64, IdentityRegistryError> {
    let n = extract_gint(par)?;
    if n < 0 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "{label} is negative: {n}"
        )));
    }
    Ok(n as u64)
}

fn decode_resolve_current(
    value: models::rhoapi::Par,
) -> Result<ResolvedCurrent, IdentityRegistryError> {
    let map = extract_emap(&value)?;
    let mut pk: Option<Vec<u8>> = None;
    let mut seq: Option<i64> = None;
    let mut block_height: Option<i64> = None;
    for kv in &map.kvs {
        let key_par = kv
            .key
            .as_ref()
            .ok_or_else(|| IdentityRegistryError::ResponseParse("map key missing".into()))?;
        let key = extract_gstring(key_par)?;
        let value = kv
            .value
            .as_ref()
            .ok_or_else(|| IdentityRegistryError::ResponseParse("map value missing".into()))?;
        match key.as_str() {
            "pk" => pk = Some(extract_gbytearray(value)?),
            "seq" => seq = Some(extract_gint(value)?),
            "blockHeight" => block_height = Some(extract_gint(value)?),
            _ => {}
        }
    }

    let pk_bytes = pk.ok_or_else(|| IdentityRegistryError::ResponseParse("missing pk".into()))?;
    let seq_val = seq.ok_or_else(|| IdentityRegistryError::ResponseParse("missing seq".into()))?;
    let block_height_val = block_height
        .ok_or_else(|| IdentityRegistryError::ResponseParse("missing blockHeight".into()))?;
    if seq_val < 0 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "seq is negative: {seq_val}"
        )));
    }
    if block_height_val < 0 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "blockHeight is negative: {block_height_val}"
        )));
    }
    Ok(ResolvedCurrent {
        pk: bytes_to_array::<PK_LEN>(pk_bytes, "pk")?,
        seq: seq_val as u64,
        block_height: block_height_val as u64,
    })
}

fn decode_history(value: models::rhoapi::Par) -> Result<Vec<HistoryEntry>, IdentityRegistryError> {
    let list = extract_elist(&value)?;
    list.ps.iter().map(decode_history_entry).collect()
}

fn decode_history_entry(par: &models::rhoapi::Par) -> Result<HistoryEntry, IdentityRegistryError> {
    let map = extract_emap(par)?;
    let mut pk: Option<Vec<u8>> = None;
    let mut op: Option<String> = None;
    let mut seq: Option<i64> = None;
    let mut block_height: Option<i64> = None;
    let mut admin_evidence: Option<Vec<u8>> = None;
    let mut admin_signers: Option<Vec<[u8; PK_LEN]>> = None;
    for kv in &map.kvs {
        let key_par = kv
            .key
            .as_ref()
            .ok_or_else(|| IdentityRegistryError::ResponseParse("map key missing".into()))?;
        let key = extract_gstring(key_par)?;
        let value = kv
            .value
            .as_ref()
            .ok_or_else(|| IdentityRegistryError::ResponseParse("map value missing".into()))?;
        match key.as_str() {
            "pk" => pk = Some(extract_gbytearray(value)?),
            "op" => op = Some(extract_gstring(value)?),
            "seq" => seq = Some(extract_gint(value)?),
            "blockHeight" => block_height = Some(extract_gint(value)?),
            "admin_evidence" => admin_evidence = Some(extract_gbytearray(value)?),
            "admin_signers" => {
                admin_signers = Some(
                    extract_elist(value)?
                        .ps
                        .iter()
                        .map(|p| bytes_to_array::<PK_LEN>(extract_gbytearray(p)?, "admin_signer"))
                        .collect::<Result<_, _>>()?,
                )
            }
            _ => {}
        }
    }
    let pk_bytes = pk.ok_or_else(|| IdentityRegistryError::ResponseParse("missing pk".into()))?;
    let op_str = op.ok_or_else(|| IdentityRegistryError::ResponseParse("missing op".into()))?;
    let seq_val = seq.ok_or_else(|| IdentityRegistryError::ResponseParse("missing seq".into()))?;
    let block_height_val = block_height
        .ok_or_else(|| IdentityRegistryError::ResponseParse("missing blockHeight".into()))?;
    if seq_val < 0 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "seq is negative: {seq_val}"
        )));
    }
    if block_height_val < 0 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "blockHeight is negative: {block_height_val}"
        )));
    }
    Ok(HistoryEntry {
        pk: bytes_to_array::<PK_LEN>(pk_bytes, "pk")?,
        op: op_str,
        seq: seq_val as u64,
        block_height: block_height_val as u64,
        admin_evidence,
        admin_signers,
    })
}

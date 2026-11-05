use std::collections::{BTreeMap, BTreeSet, HashSet};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::Bond;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use prost::bytes::Bytes;
use prost::Message;

use crate::rust::safety::clique_oracle::FtThreshold;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRecord {
    pub previous: Vec<u8>,
    pub active: BTreeSet<Bytes>,
    pub bonds: BTreeMap<Bytes, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certification {
    pub validator: Bytes,
    pub handoff: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandoffError {
    #[error("malformed handoff record: {0}")]
    Malformed(String),
    #[error("the next record does not name the held record as its previous record")]
    NotLinked,
    #[error("the held committee has no stake")]
    EmptyCommittee,
    #[error("certified stake {certified} of {total} does not exceed the threshold")]
    BelowThreshold { certified: i64, total: i64 },
}

pub fn handoff_hash(record: &[u8]) -> Vec<u8> { Blake2b256::hash(record.to_vec()) }

fn single(par: &Par, what: &str) -> Result<ExprInstance, HandoffError> {
    match par.exprs.as_slice() {
        [expr] if par.sends.is_empty() && par.receives.is_empty() && par.news.is_empty() => expr
            .expr_instance
            .clone()
            .ok_or_else(|| HandoffError::Malformed(format!("{what} is empty"))),
        _ => Err(HandoffError::Malformed(format!("{what} is not a single expression"))),
    }
}

fn bytes_of(par: &Par, what: &str) -> Result<Vec<u8>, HandoffError> {
    match single(par, what)? {
        ExprInstance::GByteArray(bytes) => Ok(bytes),
        _ => Err(HandoffError::Malformed(format!("{what} is not a byte array"))),
    }
}

impl HandoffRecord {
    pub fn decode(record: &[u8]) -> Result<Self, HandoffError> {
        let par = Par::decode(record).map_err(|e| HandoffError::Malformed(e.to_string()))?;
        let ExprInstance::ETupleBody(tuple) = single(&par, "the record")? else {
            return Err(HandoffError::Malformed("the record is not a tuple".into()));
        };
        let [previous, active, bonds] = tuple.ps.as_slice() else {
            return Err(HandoffError::Malformed("the record is not a triple".into()));
        };
        let previous = bytes_of(previous, "the previous hash")?;
        let ExprInstance::ESetBody(set) = single(active, "the active set")? else {
            return Err(HandoffError::Malformed("the active set is not a set".into()));
        };
        let active = ParSetTypeMapper::eset_to_par_set(set)
            .ps
            .map_iter(|p| bytes_of(p, "an active key").map(Bytes::from))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let ExprInstance::EMapBody(map) = single(bonds, "the bonds")? else {
            return Err(HandoffError::Malformed("the bonds are not a map".into()));
        };
        let bonds = ParMapTypeMapper::emap_to_par_map(map)
            .ps
            .map_iter(|(key, stake)| {
                let key = bytes_of(key, "a bonded key")?;
                match single(stake, "a stake")? {
                    ExprInstance::GInt(stake) => Ok((Bytes::from(key), stake)),
                    _ => Err(HandoffError::Malformed("a stake is not an integer".into())),
                }
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(HandoffRecord {
            previous,
            active,
            bonds,
        })
    }

    pub fn committee(&self) -> Vec<Bond> {
        self.active
            .iter()
            .filter_map(|key| {
                self.bonds
                    .get(key)
                    .filter(|stake| **stake > 0)
                    .map(|stake| Bond {
                        validator: key.clone(),
                        stake: *stake,
                    })
            })
            .collect()
    }
}

pub fn verify_handoff(
    held: &[u8],
    next: &[u8],
    certifications: &[Certification],
    threshold: FtThreshold,
) -> Result<HandoffRecord, HandoffError> {
    let held_record = HandoffRecord::decode(held)?;
    let next_record = HandoffRecord::decode(next)?;
    if next_record.previous != handoff_hash(held) {
        return Err(HandoffError::NotLinked);
    }
    let committee = held_record.committee();
    let total: i64 = committee.iter().map(|bond| bond.stake).sum();
    if total <= 0 {
        return Err(HandoffError::EmptyCommittee);
    }
    let certified_hash = handoff_hash(next);
    let signers: HashSet<&Bytes> = certifications
        .iter()
        .filter(|c| c.handoff == certified_hash)
        .map(|c| &c.validator)
        .collect();
    let certified: i64 = committee
        .iter()
        .filter(|bond| signers.contains(&bond.validator))
        .map(|bond| bond.stake)
        .sum();
    if (certified as i128) * (threshold.den as i128) <= (threshold.num as i128) * (total as i128) {
        return Err(HandoffError::BelowThreshold { certified, total });
    }
    Ok(next_record)
}

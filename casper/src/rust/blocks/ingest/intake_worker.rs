use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::pending_blocks::{PendingClass, PendingInsert};
use comm::rust::peer_node::PeerNode;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::Message;

use super::checked_block::CheckedBlock;
use super::dependency_tracker::{BodyAt, HeldMeta};
use super::ingest_state::{IngestInput, PendingRefusal, Refusal, Refused};
use super::ingest_step::IngestStep;
use super::ingest_view::IngestView;
use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::validate::Validate;

/// Whether an arriving block is worth keeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfInterestVerdict {
    Fresh,
    AlreadyProcessed,
    WrongShard,
    WrongVersion,
    OldUnsolicited,
}

pub(crate) fn of_interest_verdict(
    already_processed: bool,
    shard_of_interest: bool,
    version_of_interest: bool,
    old_block: bool,
    solicited: bool,
) -> OfInterestVerdict {
    if already_processed {
        OfInterestVerdict::AlreadyProcessed
    } else if !shard_of_interest {
        OfInterestVerdict::WrongShard
    } else if !version_of_interest {
        OfInterestVerdict::WrongVersion
    } else if old_block && !solicited {
        OfInterestVerdict::OldUnsolicited
    } else {
        OfInterestVerdict::Fresh
    }
}

/// Whether a block's sender is bonded in the block's own main parent. Only a
/// validated main parent can say `Unauthorized`; any other state is `Unknown`
/// and leaves the verdict to full validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderAuthorization {
    Authorized,
    Unauthorized,
    Unknown,
}

/// The checks a block passes before block ingest holds it, and the store
/// write: to the block store when its sender is authorized, else to the
/// pending area.
#[derive(Clone)]
pub struct Intake {
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    view: IngestView,
    approved_block: BlockMessage,
}

impl Intake {
    pub fn new(
        block_store: KeyValueBlockStore,
        dag: BlockDagKeyValueStorage,
        view: IngestView,
        approved_block: BlockMessage,
    ) -> Self {
        Self {
            block_store,
            dag,
            view,
            approved_block,
        }
    }

    /// Check and store `checked`, delivered by `from` (none for a block read
    /// from the local block store).
    pub fn admit(
        &self,
        checked: &CheckedBlock,
        from: Option<PeerNode>,
    ) -> Result<IngestInput, CasperError> {
        let block = checked.block();
        let hash = block.block_hash.clone();
        let solicited = self.view.is_solicited(&hash);
        let size = block.to_proto().encoded_len();
        let refused = |reason: Refusal| {
            IngestInput::IntakeRefused(Refused {
                hash: hash.clone(),
                from: from.clone(),
                unsolicited: !solicited,
                size,
                sender: block.sender.clone(),
                height: proto_util::block_number(block),
                reason,
            })
        };

        let verdict = self.of_interest(block, solicited)?;
        if verdict != OfInterestVerdict::Fresh {
            tracing::debug!(
                block = %PrettyPrinter::build_string_bytes(&hash),
                ?verdict,
                "Block is not of interest. Dropped."
            );
            return Ok(refused(Refusal::NotOfInterest));
        }

        let authorization = self.sender_authorization(block)?;
        if authorization == SenderAuthorization::Unauthorized {
            tracing::warn!(
                "Ignoring block {} because block creator {} has 0 weight in main parent.",
                PrettyPrinter::build_string_bytes(&hash),
                PrettyPrinter::build_string_bytes(&block.sender)
            );
            return Ok(refused(Refusal::Unauthorized));
        }

        let trusted = self.bonded_in_last_finalized(block)?;
        let class = match (solicited, trusted) {
            (true, _) => PendingClass::Solicited,
            (false, true) => PendingClass::KnownSender,
            (false, false) => PendingClass::Unsolicited,
        };
        let body = match IngestStep::Storage.time(|| self.store(block, authorization, class))? {
            Ok(body) => body,
            Err(refusal) => {
                tracing::warn!(
                    "Dropping block {} from {}: pending area refused it ({:?}).",
                    PrettyPrinter::build_string_bytes(&hash),
                    PrettyPrinter::build_string_bytes(&block.sender),
                    refusal
                );
                return Ok(refused(Refusal::Pending(refusal)));
            }
        };

        let mut deps = proto_util::dependencies_hashes_of(block);
        deps.sort_unstable();
        deps.dedup();
        let meta = HeldMeta {
            hash,
            sender: block.sender.clone(),
            height: proto_util::block_number(block),
            deps,
            parents: block.header.parents_hash_list.clone(),
            justification_validators: block
                .justifications
                .iter()
                .map(|justification| justification.validator.clone())
                .collect(),
            deferred: false,
            body,
            size,
            trusted,
            bonded_at_anchor: self
                .approved_block
                .body
                .state
                .bonds
                .iter()
                .any(|bond| bond.validator == block.sender),
            solicited,
            pushed_by: if solicited { None } else { from },
        };
        Ok(IngestInput::Received(meta))
    }

    /// Store an authorized block on disk and any other in the pending area,
    /// which may refuse it.
    fn store(
        &self,
        block: &BlockMessage,
        authorization: SenderAuthorization,
        class: PendingClass,
    ) -> Result<Result<BodyAt, PendingRefusal>, CasperError> {
        if authorization == SenderAuthorization::Authorized {
            self.block_store.put_block_message(block)?;
            return Ok(Ok(BodyAt::Disk));
        }
        Ok(match self.block_store.put_pending(block, class)? {
            PendingInsert::Accepted => Ok(BodyAt::Pending),
            PendingInsert::AreaFull => Err(PendingRefusal::AreaFull),
            PendingInsert::SenderLimitReached => Err(PendingRefusal::SenderLimitReached),
            PendingInsert::TooLarge => Err(PendingRefusal::TooLarge),
        })
    }

    fn of_interest(
        &self,
        block: &BlockMessage,
        solicited: bool,
    ) -> Result<OfInterestVerdict, CasperError> {
        let approved = &self.approved_block;
        Ok(of_interest_verdict(
            self.dag.contains(&block.block_hash)? || self.view.is_held(&block.block_hash),
            approved.shard_id.eq_ignore_ascii_case(&block.shard_id),
            Validate::version(block, approved.header.version),
            proto_util::block_number(block) < proto_util::block_number(approved),
            solicited,
        ))
    }

    pub fn sender_authorization(
        &self,
        block: &BlockMessage,
    ) -> Result<SenderAuthorization, CasperError> {
        let Some(main_parent) = block.header.parents_hash_list.first() else {
            return Ok(SenderAuthorization::Unknown);
        };
        Ok(match self.dag.lookup(main_parent)? {
            Some(parent) if !parent.is_invalid() => match parent.weight_map.get(&block.sender) {
                Some(stake) if !stake.is_zero() => SenderAuthorization::Authorized,
                _ => SenderAuthorization::Unauthorized,
            },
            _ => SenderAuthorization::Unknown,
        })
    }

    /// The signature is already checked: a block is trusted when its sender
    /// is bonded in this node's last finalized block.
    fn bonded_in_last_finalized(&self, block: &BlockMessage) -> Result<bool, CasperError> {
        let Some(last_finalized) = self.dag.last_finalized_block() else {
            return Ok(false);
        };
        Ok(self.dag.lookup(&last_finalized)?.is_some_and(|meta| {
            meta.weight_map
                .get(&block.sender)
                .is_some_and(|stake| !stake.is_zero())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solicited_old_block_is_of_interest_and_an_unsolicited_one_is_not() {
        assert_eq!(
            of_interest_verdict(false, true, true, true, true),
            OfInterestVerdict::Fresh
        );
        assert_eq!(
            of_interest_verdict(false, true, true, true, false),
            OfInterestVerdict::OldUnsolicited
        );
        assert_eq!(
            of_interest_verdict(true, true, true, false, true),
            OfInterestVerdict::AlreadyProcessed
        );
        assert_eq!(
            of_interest_verdict(false, false, true, false, true),
            OfInterestVerdict::WrongShard
        );
        assert_eq!(
            of_interest_verdict(false, true, false, false, true),
            OfInterestVerdict::WrongVersion
        );
    }
}

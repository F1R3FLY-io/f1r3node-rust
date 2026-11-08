use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use super::durable_rspace_authority::{
    assert_empty, fixture, poll_checkpoint_with, poll_once, populate, publication_limits,
    store_files,
};
use super::owner_incarnation::journal_bags;
use super::trusted_successor_production::{PublicationEffects, observed_effects};
use crate::rspace::errors::{HistoryError, RootError};
use crate::rspace::history::history_repository::HistoryRepositoryInstances;
use crate::rspace::hot_store_action::{HotStoreAction, InsertAction, InsertData};
use crate::rspace::internal::Datum;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableRSpaceTransactionError, GenerationCheckpointPublicationHookPointV1,
    JournalPublicationPhaseV1, ViewAttachmentRefusalV1,
    arm_generation_checkpoint_publication_failpoint,
};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

fn no_effects() -> PublicationEffects {
    PublicationEffects {
        trace: Vec::new(),
        cold_admissions: 0,
        index_constructions: 0,
    }
}

fn refused<T>(
    result: Result<T, Box<DurableRSpaceTransactionError>>,
) -> Option<DurableRSpaceTransactionError> {
    result.err().map(|error| *error)
}

fn attachment_refusal<T>(
    result: Result<T, Box<DurableRSpaceTransactionError>>,
) -> Option<ViewAttachmentRefusalV1> {
    match refused(result) {
        Some(DurableRSpaceTransactionError::Attachment(refusal)) => Some(refusal),
        _ => None,
    }
}

fn legacy_insert(value: &str) -> HotStoreAction<String, String, String, String> {
    let channel = "channel".to_owned();
    HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        data: vec![Datum::create(&channel, value.to_owned(), false)],
        channel,
    }))
}

#[test]
fn a_foreign_gate_permit_is_refused_before_capture_without_mutation() {
    let foreign = Arc::new(RwLock::new(()));
    let (ready, ready_expected) = {
        let (scratch, hot, gate, mut owner) = fixture();
        let attachment = owner.attach(&hot).expect("attach to the ready owner");
        populate(&hot);
        let old = owner.state(&hot).expect("read the ready owner's state");
        let key = owner.generation_key();
        let journals = journal_bags(&hot);
        let files = store_files(scratch.path());
        let captured = Rc::new(Cell::new(false));
        let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
            let captured = Rc::clone(&captured);
            move || captured.set(true)
        });
        let (result, effects) = observed_effects(|| {
            poll_once(
                owner.checkpoint_view(
                    Arc::clone(&foreign)
                        .try_write_owned()
                        .expect("take the free foreign gate"),
                    attachment,
                    &gate,
                    &hot,
                    publication_limits(),
                ),
            )
            .map_err(Box::new)
        });
        drop(hook);
        let observed = (
            matches!(refused(result), Some(DurableRSpaceTransactionError::ForeignPermit)),
            captured.get(),
            effects,
            owner.phase(&hot),
            owner.state(&hot).ok(),
            owner.generation_key() == key,
            journal_bags(&hot) == journals,
            store_files(scratch.path()) == files,
            matches!(gate.try_write(), Ok(_)),
            matches!(foreign.try_write(), Ok(_)),
            owner.validate_attachment(attachment),
        );
        (
            observed,
            (
                true,
                false,
                no_effects(),
                JournalPublicationPhaseV1::Ready,
                Some(old),
                true,
                true,
                true,
                true,
                true,
                Ok(()),
            ),
        )
    };
    let (pending, pending_expected) = {
        let (scratch, hot, gate, mut owner) = fixture();
        let attachment = owner.attach(&hot).expect("attach to the ready owner");
        populate(&hot);
        arm_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        );
        let uncertain =
            matches!(poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits()), Err(_));
        let terminal = owner.pending_terminal();
        let journals = journal_bags(&hot);
        let files = store_files(scratch.path());
        let (result, effects) = observed_effects(|| {
            poll_once(
                owner.checkpoint_view(
                    Arc::clone(&foreign)
                        .try_write_owned()
                        .expect("take the free foreign gate"),
                    attachment,
                    &gate,
                    &hot,
                    publication_limits(),
                ),
            )
            .map_err(Box::new)
        });
        let held = (
            matches!(refused(result), Some(DurableRSpaceTransactionError::ForeignPermit)),
            effects,
            owner.phase(&hot),
            owner.pending_terminal() == terminal,
            journal_bags(&hot) == journals,
            store_files(scratch.path()) == files,
            matches!(gate.try_write(), Err(_)),
            matches!(foreign.try_write(), Ok(_)),
        );
        let resolved =
            owner.reconcile(publication_limits()).ok() == terminal.map(|slot| slot.state());
        let released = matches!(gate.try_write(), Ok(_));
        (
            (uncertain, held, resolved, released),
            (
                true,
                (
                    true,
                    no_effects(),
                    JournalPublicationPhaseV1::RecoveryRequired,
                    true,
                    true,
                    true,
                    true,
                    true,
                ),
                true,
                true,
            ),
        )
    };
    assert_eq!(
        (ready, pending),
        (ready_expected, pending_expected),
        "a permit on another gate must be refused before capture, on a ready owner and on one \
         whose pending publication holds the view gate, changing no phase, state, pin, terminal, \
         journal, file or gate; the ready owner still accepts its attachment and the pending one \
         reconciles to its terminal"
    );
}

#[test]
fn a_stale_base_is_refused_like_a_legacy_root_conflict_without_mutation() {
    let (durable, durable_expected) = {
        let (scratch, hot, gate, mut owner) = fixture();
        let stale = owner.attach(&hot).expect("attach at the first base");
        populate(&hot);
        let advanced = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
            .expect("an intervening checkpoint advances the base");
        let fresh = owner.attach(&hot).expect("attach at the advanced base");
        let channel = "channel".to_owned();
        hot.put_datum(&channel, Datum::create(&channel, "stale".to_owned(), false));
        let key = owner.generation_key();
        let journals = journal_bags(&hot);
        let files = store_files(scratch.path());
        let (result, effects) = observed_effects(|| {
            poll_once(
                owner.checkpoint_view(
                    Arc::clone(&gate)
                        .try_write_owned()
                        .expect("take the free view gate"),
                    stale,
                    &gate,
                    &hot,
                    publication_limits(),
                ),
            )
            .map_err(Box::new)
        });
        let refusal = (
            fresh.base().state().generation() == stale.base().state().generation() + 1,
            attachment_refusal(result),
            effects,
            owner.phase(&hot),
            owner.state(&hot).ok(),
            owner.generation_key() == key,
            journal_bags(&hot) == journals,
            store_files(scratch.path()) == files,
            matches!(gate.try_write(), Ok(_)),
        );
        let retried = poll_once(
            owner.checkpoint_view(
                Arc::clone(&gate)
                    .try_write_owned()
                    .expect("take the free view gate"),
                fresh,
                &gate,
                &hot,
                publication_limits(),
            ),
        )
        .map(|state| state.generation())
        .ok();
        (
            (refusal, retried),
            (
                (
                    true,
                    Some(ViewAttachmentRefusalV1::StaleBase {
                        attached: Box::new(stale.base()),
                        current: Box::new(fresh.base()),
                    }),
                    no_effects(),
                    JournalPublicationPhaseV1::Ready,
                    Some(advanced),
                    true,
                    true,
                    true,
                    true,
                ),
                Some(advanced.generation() + 1),
            ),
        )
    };
    let (legacy, legacy_expected) = {
        let repository = HistoryRepositoryInstances::<String, String, String, String>::repository(
            RSpaceStateAuthority::in_memory(),
        )
        .expect("open an in-memory legacy repository");
        let expected = repository
            .current_authority_root()
            .expect("read the legacy root");
        let next = repository
            .checkpoint_at(&expected, vec![legacy_insert("intervening")])
            .expect("an intervening legacy checkpoint advances the root");
        let actual = next
            .current_authority_root()
            .expect("read the advanced legacy root");
        let conflict = next
            .checkpoint_at(&expected, vec![legacy_insert("stale")])
            .err();
        let unchanged = next.current_authority_root().ok() == Some(actual.clone());
        let retried = match next.checkpoint_at(&actual, vec![legacy_insert("stale")]) {
            Ok(retried) => retried
                .current_authority_root()
                .ok()
                .map(|root| root.generation()),
            Err(_) => None,
        };
        (
            (actual.generation() == expected.generation() + 1, conflict, unchanged, retried),
            (
                true,
                Some(HistoryError::RootError(RootError::RootCatalogConflict {
                    expected_generation: Some(expected.generation()),
                    actual_generation: Some(actual.generation()),
                })),
                true,
                Some(actual.generation() + 1),
            ),
        )
    };
    assert_eq!(
        (durable, legacy),
        (durable_expected, legacy_expected),
        "an attachment one generation behind must be refused before capture, naming both bases \
         and changing nothing, and a fresh attachment must publish the next generation, as the \
         legacy repository refuses a stale expected root with a root-catalog conflict and accepts \
         the refreshed one"
    );
}

#[tokio::test]
async fn checkpointing_under_the_held_view_gate_and_owner_mutex_completes() {
    let (scratch, hot, gate, owner) = fixture();
    populate(&hot);
    let old = owner.state(&hot).expect("read the ready owner's state");
    let files = store_files(scratch.path());
    let owner = Arc::new(Mutex::new(owner));
    let held = Rc::new(Cell::new(None));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
        let held = Rc::clone(&held);
        let gate = Arc::clone(&gate);
        let owner = Arc::clone(&owner);
        move || {
            held.set(Some((matches!(gate.try_read(), Err(_)), matches!(owner.try_lock(), Err(_)))))
        }
    });
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        let permit = Arc::clone(&gate).write_owned().await;
        let mut owner = owner.lock().await;
        let attachment = owner.attach(&hot).expect("attach under the view gate");
        owner
            .checkpoint_view(permit, attachment, &gate, &hot, publication_limits())
            .await
            .map(|state| state.generation())
            .ok()
    })
    .await
    .ok();
    drop(hook);
    let owner = owner
        .try_lock()
        .expect("the checkpoint released the owner mutex");
    assert_eq!(
        (
            completed,
            held.get(),
            owner.phase(&hot),
            owner.state(&hot).ok().map(|state| state.generation()),
            matches!(gate.try_write(), Ok(_)),
            store_files(scratch.path()) == files,
        ),
        (
            Some(Some(old.generation() + 1)),
            Some((true, true)),
            JournalPublicationPhaseV1::Ready,
            Some(old.generation() + 1),
            true,
            false,
        ),
        "a caller holding the view gate's permit and then the owner mutex must complete its \
         checkpoint, with both held at capture and both free afterwards"
    );
    assert_empty(&hot);
}

#[test]
fn a_pending_publication_refuses_another_gates_own_permit() {
    let other = Arc::new(RwLock::new(()));
    let (scratch, hot, gate, mut owner) = fixture();
    let attachment = owner.attach(&hot).expect("attach to the ready owner");
    populate(&hot);
    arm_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    );
    let uncertain =
        matches!(poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits()), Err(_));
    let terminal = owner.pending_terminal();
    let journals = journal_bags(&hot);
    let files = store_files(scratch.path());
    let (result, effects) = observed_effects(|| {
        poll_once(
            owner.checkpoint_view(
                Arc::clone(&other)
                    .try_write_owned()
                    .expect("take the free other gate"),
                attachment,
                &other,
                &hot,
                publication_limits(),
            ),
        )
        .map_err(Box::new)
    });
    assert_eq!(
        (
            uncertain,
            matches!(
                refused(result),
                Some(DurableRSpaceTransactionError::Unresolved(
                    JournalPublicationPhaseV1::RecoveryRequired
                ))
            ),
            effects,
            owner.pending_terminal() == terminal,
            journal_bags(&hot) == journals,
            store_files(scratch.path()) == files,
            matches!(gate.try_write(), Err(_)),
            matches!(other.try_write(), Ok(_)),
        ),
        (true, true, no_effects(), true, true, true, true, true),
        "a checkpoint through another gate's own permit must be refused as unresolved while a \
         publication is pending, leaving the terminal, journals and files unchanged and the view \
         gate held by the pending publication"
    );
}

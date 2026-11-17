use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, TestCaseError, TestRng, TestRunner};

use super::checkpoint_wal::{
    fixture as checkpoint_source_fixture, fixture_with_retained_union_cardinality,
};
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
    publish_data_successor,
};
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::durable_rspace_authority::{
    Profile, fixture as owner_fixture, poll_checkpoint_with, populate, publication_limits,
};
use super::support::{TestSemanticProfile, trial_scratch};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::durable_history_repository::DurableHistoryRepositoryV1;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoError, DurablePublishedCoreV1,
    DurableStateRootV1, DurableViewReadHandleV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1 as PublicationPoint, ReclamationCandidateV1,
    SemanticallyAdmittedDurableWriterV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, index_file_name, pack_file_name,
    recover_read_authority,
};
use crate::rspace::persistence::retention::v2_format::{RetentionAnchorV2, RetentionFormat};
use crate::rspace::persistence::retention::v2_reader::admit_retention_history_v2;
use crate::rspace::persistence::retention::{
    KnownRootLeaseError, KnownRootLeaseV1, RetainedRootKind, RetainedRootRegistryV1,
    RetainedRootV1, RetentionRegistryError, RootLeaseV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogSelectionV1, CatalogSlotV1, GenerationLeaseError,
    GenerationLeaseKeyV1, GenerationLeaseRegistryV1, GenerationLeaseV1, IndexGenerationV1,
    LogPackDecodeLimits, ObjectDigest, ObjectRefV1, PackId, RetentionManifestV1,
    RetentionManifestView, verify_structural_closure,
};

type GcWriter = SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogRole {
    Old,
    New,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    Present,
    Absent,
    Partial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntentRole {
    Absent,
    Exact,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinClass {
    OldExact,
    OldAlias,
    New,
    Other,
}

impl PinClass {
    const fn pins_old(self) -> bool { matches!(self, Self::OldExact | Self::OldAlias) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinOrigin {
    Prelude,
    Hook(PublicationPoint),
    Settlement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PinRecord {
    lease_id: u64,
    class: PinClass,
    origin: PinOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeOutcome {
    NoIntent,
    Refused,
    Deferred,
    Reclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GcMoment {
    Admitted,
    Hook(PublicationPoint),
    Interrupted(PublicationPoint),
    Published,
    Crashed,
    Recovered,
    Readmitted,
    Probed(ProbeOutcome),
    Released,
    Settled,
}

#[derive(Debug)]
struct GcObservation {
    moment: GcMoment,
    crashed: bool,
    crashes: usize,
    catalog: CatalogRole,
    old_generation: Presence,
    intent: IntentRole,
    pins: Vec<PinRecord>,
    registry_leases: usize,
    pins_registered: bool,
    refused_releases: usize,
}

impl GcObservation {
    fn holds_old_pin(&self) -> bool { self.pins.iter().any(|pin| pin.class.pins_old()) }

    fn holds_pin(&self, class: PinClass) -> bool { self.pins.iter().any(|pin| pin.class == class) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinPrelude {
    Unpinned,
    Exact,
    Alias,
    ExactAndAlias,
}

impl PinPrelude {
    const fn classes(self) -> &'static [PinClass] {
        match self {
            Self::Unpinned => &[],
            Self::Exact => &[PinClass::OldExact],
            Self::Alias => &[PinClass::OldAlias],
            Self::ExactAndAlias => &[PinClass::OldExact, PinClass::OldAlias],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aftermath {
    Survive,
    Crash,
}

#[derive(Clone, Copy, Debug)]
struct GcTrialSpec {
    retained_union_objects: usize,
    earlier_publications: usize,
    prelude: PinPrelude,
    churn: bool,
    interruption: Option<usize>,
    aftermath: Aftermath,
}

#[derive(Debug)]
struct GcTrace {
    spec: GcTrialSpec,
    observations: Vec<GcObservation>,
    refused_old_acquisitions: usize,
    halt: Option<String>,
}

struct HeldPin {
    lease: GenerationLeaseV1,
    origin: PinOrigin,
}

struct GcRecorder {
    root: PathBuf,
    old: GenerationLeaseKeyV1,
    alias: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
    old_files: Vec<(PathBuf, Vec<u8>)>,
    registry: GenerationLeaseRegistryV1,
    pins: Vec<HeldPin>,
    churn: bool,
    crashed: bool,
    crashes: usize,
    refused_releases: usize,
    refused_old_acquisitions: usize,
    last_hook: Option<usize>,
    halt: Option<String>,
    observations: Vec<GcObservation>,
}

impl GcRecorder {
    fn catalog_role(&self) -> CatalogRole {
        match selected_slot(&self.root).map(GenerationLeaseKeyV1::from_catalog) {
            Ok(key) if key == self.old => CatalogRole::Old,
            Ok(key) if key == self.successor => CatalogRole::New,
            Ok(_) | Err(_) => CatalogRole::Other,
        }
    }

    fn intent_role(&self) -> IntentRole {
        match fs::read(self.root.join("reclaim.v1")) {
            Err(error) if error.kind() == ErrorKind::NotFound => IntentRole::Absent,
            Err(_) => IntentRole::Other,
            Ok(bytes) => match ReclamationCandidateV1::decode(&bytes) {
                Ok(candidate)
                    if candidate.old() == self.old && candidate.successor() == self.successor =>
                {
                    IntentRole::Exact
                }
                Ok(_) | Err(_) => IntentRole::Other,
            },
        }
    }

    fn classify(&self, key: GenerationLeaseKeyV1) -> PinClass {
        let same_generation = (key.first_live_pack(), key.active_pack(), key.index_generation()) ==
            (self.old.first_live_pack(), self.old.active_pack(), self.old.index_generation());
        match (key == self.old, key == self.successor, same_generation) {
            (true, _, _) => PinClass::OldExact,
            (false, true, _) => PinClass::New,
            (false, false, true) => PinClass::OldAlias,
            (false, false, false) => PinClass::Other,
        }
    }

    fn observe(&mut self, moment: GcMoment) {
        let pins = self
            .pins
            .iter()
            .map(|pin| PinRecord {
                lease_id: pin.lease.lease_id(),
                class: self.classify(pin.lease.key()),
                origin: pin.origin,
            })
            .collect();
        let pins_registered = self
            .pins
            .iter()
            .all(|pin| matches!(self.registry.has_active(pin.lease.key()), Ok(true)));
        let observation = GcObservation {
            moment,
            crashed: self.crashed,
            crashes: self.crashes,
            catalog: self.catalog_role(),
            old_generation: generation_presence(&self.old_files),
            intent: self.intent_role(),
            pins,
            registry_leases: self.registry.active_lease_count_for_test(),
            pins_registered,
            refused_releases: self.refused_releases,
        };
        self.observations.push(observation);
    }

    fn acquire(&mut self, class: PinClass, origin: PinOrigin) -> Result<(), String> {
        let key = match class {
            PinClass::OldExact => self.old,
            PinClass::OldAlias => self.alias,
            PinClass::New => self.successor,
            PinClass::Other => {
                return Err("the engine never pins an unclassified generation".to_owned());
            }
        };
        match (self.registry.acquire(key), class.pins_old(), generation_presence(&self.old_files)) {
            (Ok(lease), _, _) => {
                self.pins.push(HeldPin { lease, origin });
                Ok(())
            }
            (Err(GenerationLeaseError::Retired(retired)), true, Presence::Absent)
                if retired == key =>
            {
                self.refused_old_acquisitions += 1;
                Ok(())
            }
            (Err(error), _, presence) => Err(format!(
                "the registry refused a {class:?} pin that the model enables (old generation \
                 {presence:?}): {error:?}"
            )),
        }
    }

    fn release(&mut self, position: usize) {
        let mut pin = self.pins.remove(position);
        if pin.lease.release().is_err() {
            self.refused_releases += 1;
        }
    }

    fn at_hook(&mut self, index: usize) {
        let point = FORMAL_PUBLICATION_PHASE_ORDER[index];
        self.last_hook = Some(index);
        if self.churn {
            let class = match (index % 2, self.catalog_role()) {
                (0, _) => PinClass::OldExact,
                (_, CatalogRole::New) => PinClass::New,
                (_, CatalogRole::Old | CatalogRole::Other) => PinClass::OldAlias,
            };
            if let Err(reason) = self.acquire(class, PinOrigin::Hook(point)) {
                self.halt = self.halt.take().or(Some(reason));
            }
            let mut churned = self.pins.iter().enumerate().filter_map(|(position, pin)| {
                matches!(pin.origin, PinOrigin::Hook(_)).then_some(position)
            });
            if let (Some(oldest), Some(_), Some(_)) =
                (churned.next(), churned.next(), churned.next())
            {
                self.release(oldest);
            }
        }
        self.observe(GcMoment::Hook(point));
    }
}

fn generation_presence(files: &[(PathBuf, Vec<u8>)]) -> Presence {
    let mut intact = 0_usize;
    let mut missing = 0_usize;
    for (path, expected) in files {
        match fs::read(path) {
            Ok(bytes) if bytes == *expected => intact += 1,
            Err(error) if error.kind() == ErrorKind::NotFound => missing += 1,
            Ok(_) | Err(_) => {}
        }
    }
    match (intact == files.len(), missing == files.len()) {
        (true, _) => Presence::Present,
        (false, true) => Presence::Absent,
        (false, false) => Presence::Partial,
    }
}

fn generation_files(
    root: &Path,
    key: GenerationLeaseKeyV1,
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let packs = usize::try_from(key.active_pack() - key.first_live_pack())
        .map_err(|error| format!("the live pack range does not fit memory: {error}"))?;
    let mut files = Vec::with_capacity(packs + 2);
    for pack in key.first_live_pack()..=key.active_pack() {
        let pack = PackId::new(pack).map_err(|error| format!("invalid live pack: {error:?}"))?;
        files.push(read_generation_file(root.join(pack_file_name(pack)))?);
    }
    let index = IndexGenerationV1::new(key.index_generation())
        .map_err(|error| format!("invalid index generation: {error:?}"))?;
    files.push(read_generation_file(root.join(index_file_name(index)))?);
    Ok(files)
}

fn read_generation_file(path: PathBuf) -> Result<(PathBuf, Vec<u8>), String> {
    let bytes =
        fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok((path, bytes))
}

fn selected_slot(root: &Path) -> Result<CatalogSlotV1, String> {
    let bytes = fs::read(root.join("catalog.v1"))
        .map_err(|error| format!("cannot read the catalog: {error}"))?;
    let image = <[u8; CATALOG_IMAGE_LEN]>::try_from(bytes.as_slice())
        .map_err(|_| format!("the catalog holds {} bytes", bytes.len()))?;
    match CatalogSelectionV1::select(&image) {
        Ok(Some(selection)) => Ok(selection.slot()),
        Ok(None) => Err("the catalog selects no slot".to_owned()),
        Err(error) => Err(format!("the catalog does not parse: {error:?}")),
    }
}

fn admit_checkpoint(root: &Path) -> Result<GcWriter, String> {
    DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("admission was refused: {error:?}"))
}

fn publish_checkpoint(writer: GcWriter) -> Result<GcWriter, String> {
    writer
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("publication was refused: {error:?}"))
}

fn start_gc_trial(spec: GcTrialSpec, root: &Path) -> Result<(GcWriter, GcRecorder), String> {
    install_checkpoint_image(
        root,
        &checkpoint_image_from_source(fixture_with_retained_union_cardinality(
            spec.retained_union_objects,
        )),
    );
    let mut writer = admit_checkpoint(root)?;
    for _ in 0..spec.earlier_publications {
        writer = publish_checkpoint(writer)?;
    }
    let old = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let alias = GenerationLeaseKeyV1::new(
        old.epoch()
            .checked_sub(1)
            .ok_or_else(|| "the admitted catalog epoch has no predecessor".to_owned())?,
        old.first_live_pack(),
        old.active_pack(),
        old.index_generation(),
    );
    let successor = GenerationLeaseKeyV1::new(
        old.epoch() + 1,
        old.active_pack() + 1,
        old.active_pack() + 1,
        old.index_generation() + 1,
    );
    let mut recorder = GcRecorder {
        root: root.to_path_buf(),
        old,
        alias,
        successor,
        old_files: generation_files(root, old)?,
        registry: writer.generation_leases(),
        pins: Vec::with_capacity(8),
        churn: spec.churn,
        crashed: false,
        crashes: 0,
        refused_releases: 0,
        refused_old_acquisitions: 0,
        last_hook: None,
        halt: None,
        observations: Vec::with_capacity(64),
    };
    for class in spec.prelude.classes() {
        recorder.acquire(*class, PinOrigin::Prelude)?;
    }
    recorder.observe(GcMoment::Admitted);
    Ok((writer, recorder))
}

fn arm_observation_chain(recorder: Rc<RefCell<GcRecorder>>, index: usize, last: usize) {
    arm_generation_checkpoint_publication_hook(FORMAL_PUBLICATION_PHASE_ORDER[index], move || {
        recorder.borrow_mut().at_hook(index);
        if index < last {
            arm_observation_chain(recorder, index + 1, last);
        }
    });
}

fn publish_observed(
    writer: GcWriter,
    interruption: Option<usize>,
    recorder: &Rc<RefCell<GcRecorder>>,
) -> Result<Option<GcWriter>, String> {
    let last = interruption.unwrap_or(FORMAL_PUBLICATION_PHASE_ORDER.len() - 1);
    recorder.borrow_mut().last_hook = None;
    arm_observation_chain(Rc::clone(recorder), 0, last);
    if let Some(index) = interruption {
        arm_generation_checkpoint_publication_failpoint(FORMAL_PUBLICATION_PHASE_ORDER[index]);
    }
    let published = publish_checkpoint(writer);
    let mut state = recorder.borrow_mut();
    if let Some(reason) = state.halt.take() {
        return Err(reason);
    }
    match (published, interruption) {
        (Ok(writer), None) if state.last_hook == Some(last) => {
            state.observe(GcMoment::Published);
            Ok(Some(writer))
        }
        (Err(_), Some(index)) if state.last_hook == Some(index) => {
            state.observe(GcMoment::Interrupted(FORMAL_PUBLICATION_PHASE_ORDER[index]));
            Ok(None)
        }
        (Ok(_), _) => Err(format!(
            "publication completed although interruption {interruption:?} was armed or a point \
             was not observed (last observed {:?})",
            state.last_hook
        )),
        (Err(reason), _) => Err(format!(
            "publication failed after observation point {:?} instead of at {interruption:?}: \
             {reason}",
            state.last_hook
        )),
    }
}

fn probe(recorder: &Rc<RefCell<GcRecorder>>) -> Result<(), String> {
    let root = recorder.borrow().root.clone();
    let mut lease = DurableDirectoryV1::open(root.as_path())
        .and_then(DurableDirectoryV1::try_acquire_writer)
        .map_err(|error| format!("the probe cannot take the writer lock: {error:?}"))?;
    let candidate = lease
        .read_reclamation_candidate()
        .map_err(|error| format!("the probe cannot read the intent: {error:?}"))?;
    let outcome = match candidate {
        None => ProbeOutcome::NoIntent,
        Some(candidate) => {
            match lease.reclaim_reclamation_candidate(candidate, selected_slot(&root)?) {
                Ok(reclaimed) if reclaimed.deferred_for_reader => ProbeOutcome::Deferred,
                Ok(_) => ProbeOutcome::Reclaimed,
                Err(DurableIoError::CatalogAuthorityChanged { .. }) => ProbeOutcome::Refused,
                Err(error) => return Err(format!("the probe's reclamation failed: {error:?}")),
            }
        }
    };
    drop(lease);
    recorder.borrow_mut().observe(GcMoment::Probed(outcome));
    Ok(())
}

fn crash(recorder: &Rc<RefCell<GcRecorder>>) -> Result<(), String> {
    let mut state = recorder.borrow_mut();
    state.pins.clear();
    state.registry = GenerationLeaseRegistryV1::default();
    let directory = DurableDirectoryV1::open(state.root.as_path())
        .map_err(|error| format!("the crashed store directory does not open: {error:?}"))?;
    state.registry = directory.generation_leases();
    state.crashed = true;
    state.crashes += 1;
    state.observe(GcMoment::Crashed);
    Ok(())
}

fn recover(recorder: &Rc<RefCell<GcRecorder>>) -> Result<GcWriter, String> {
    let root = recorder.borrow().root.clone();
    let recovered = admit_checkpoint(&root);
    let mut state = recorder.borrow_mut();
    match recovered {
        Ok(writer) => {
            state.registry = writer.generation_leases();
            state.crashed = false;
            state.observe(GcMoment::Recovered);
            Ok(writer)
        }
        Err(reason) => {
            state.observe(GcMoment::Crashed);
            Err(format!("recovery after a crash was refused: {reason}"))
        }
    }
}

fn readmit(recorder: &Rc<RefCell<GcRecorder>>) -> Result<GcWriter, String> {
    let root = recorder.borrow().root.clone();
    let writer = admit_checkpoint(&root)
        .map_err(|reason| format!("re-admission with live pins was refused: {reason}"))?;
    let mut state = recorder.borrow_mut();
    if !state.registry.shares_registry(&writer.generation_leases()) {
        return Err("the re-admitted writer does not share the registry of the pins".to_owned());
    }
    state.observe(GcMoment::Readmitted);
    Ok(writer)
}

fn settle(recorder: &Rc<RefCell<GcRecorder>>) -> Result<(), String> {
    let root = recorder.borrow().root.clone();
    let writer = admit_checkpoint(&root)
        .map_err(|reason| format!("the settling admission was refused: {reason}"))?;
    let mut state = recorder.borrow_mut();
    state.registry = writer.generation_leases();
    state.acquire(PinClass::OldExact, PinOrigin::Settlement)?;
    state.acquire(PinClass::OldAlias, PinOrigin::Settlement)?;
    state.observe(GcMoment::Settled);
    match state.observations.last() {
        Some(GcObservation {
            catalog: CatalogRole::New,
            old_generation: Presence::Absent,
            intent: IntentRole::Absent,
            pins,
            ..
        }) if pins.is_empty() => Ok(()),
        settled => Err(format!("the trial did not settle: {settled:?}")),
    }
}

fn drive_gc_trial(
    spec: GcTrialSpec,
    writer: GcWriter,
    recorder: &Rc<RefCell<GcRecorder>>,
) -> Result<(), String> {
    let published = publish_observed(writer, spec.interruption, recorder)?;
    let writer = match (published, spec.aftermath) {
        (Some(writer), Aftermath::Survive) => writer,
        (None, Aftermath::Survive) => {
            probe(recorder)?;
            readmit(recorder)?
        }
        (published, Aftermath::Crash) => {
            drop(published);
            crash(recorder)?;
            drop(recover(recorder)?);
            crash(recorder)?;
            let writer = recover(recorder)?;
            for class in spec.prelude.classes() {
                recorder.borrow_mut().acquire(*class, PinOrigin::Prelude)?;
            }
            writer
        }
    };
    let catalog = recorder.borrow().catalog_role();
    let writer = match catalog {
        CatalogRole::Old => publish_observed(writer, None, recorder)?
            .ok_or_else(|| "the retried publication returned no writer".to_owned())?,
        CatalogRole::New | CatalogRole::Other => writer,
    };
    drop(writer);
    while !recorder.borrow().pins.is_empty() {
        {
            let mut state = recorder.borrow_mut();
            state.release(0);
            state.observe(GcMoment::Released);
        }
        probe(recorder)?;
    }
    settle(recorder)
}

fn run_gc_trial(spec: GcTrialSpec) -> GcTrace {
    let scratch = trial_scratch("gc-protection-");
    let (writer, recorder) = match start_gc_trial(spec, scratch.path()) {
        Ok(started) => started,
        Err(reason) => {
            return GcTrace {
                spec,
                observations: Vec::new(),
                refused_old_acquisitions: 0,
                halt: Some(reason),
            };
        }
    };
    let recorder = Rc::new(RefCell::new(recorder));
    let driven = drive_gc_trial(spec, writer, &recorder);
    let mut state = recorder.borrow_mut();
    GcTrace {
        spec,
        observations: std::mem::take(&mut state.observations),
        refused_old_acquisitions: state.refused_old_acquisitions,
        halt: driven.err(),
    }
}

fn gc_trial_specs() -> Vec<GcTrialSpec> {
    const PINNED_PLANS: [(PinPrelude, bool); 4] = [
        (PinPrelude::Exact, true),
        (PinPrelude::Alias, true),
        (PinPrelude::ExactAndAlias, true),
        (PinPrelude::Unpinned, true),
    ];
    let slots = FORMAL_PUBLICATION_PHASE_ORDER.len() + 1;
    let mut specs = Vec::with_capacity(slots * 4);
    for slot in 0..slots {
        for aftermath in [Aftermath::Survive, Aftermath::Crash] {
            for (prelude, churn) in
                [(PinPrelude::Unpinned, false), PINNED_PLANS[slot % PINNED_PLANS.len()]]
            {
                specs.push(GcTrialSpec {
                    retained_union_objects: 2 + specs.len() % 3,
                    earlier_publications: specs.len() / 3 % 2,
                    prelude,
                    churn,
                    interruption: slot.checked_sub(1),
                    aftermath,
                });
            }
        }
    }
    specs
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    match (panic.downcast_ref::<String>(), panic.downcast_ref::<&str>()) {
        (Some(message), _) => format!("the trial panicked: {message}"),
        (None, Some(message)) => format!("the trial panicked: {message}"),
        (None, None) => "the trial panicked".to_owned(),
    }
}

fn gc_trial_sample() -> &'static [GcTrace] {
    static SAMPLE: OnceLock<Vec<GcTrace>> = OnceLock::new();
    SAMPLE.get_or_init(|| {
        let specs = gc_trial_specs();
        let mut traces = Vec::with_capacity(specs.len());
        for spec in specs {
            let trace = std::thread::Builder::new()
                .name(format!("gc-protection-trial-{}", traces.len()))
                .spawn(move || run_gc_trial(spec))
                .expect("spawn a GC protection trial thread")
                .join()
                .unwrap_or_else(|panic| GcTrace {
                    spec,
                    observations: Vec::new(),
                    refused_old_acquisitions: 0,
                    halt: Some(panic_message(panic.as_ref())),
                });
            traces.push(trace);
        }
        traces
    })
}

fn checked_sample(property: &str, holds: impl Fn(&GcObservation) -> bool) -> &'static [GcTrace] {
    let sample = gc_trial_sample();
    for trace in sample {
        if let Some(violation) = trace
            .observations
            .iter()
            .find(|observation| !holds(observation))
        {
            panic!("{property} is violated in trial {:?}: {violation:?}", trace.spec);
        }
    }
    for trace in sample {
        if let Some(reason) = &trace.halt {
            panic!("trial {:?} halted before settling: {reason}", trace.spec);
        }
    }
    sample
}

fn sample_observations(sample: &[GcTrace]) -> impl Iterator<Item = &GcObservation> {
    sample.iter().flat_map(|trace| trace.observations.iter())
}

#[test]
fn catalog_selection_and_generation_pins_stay_typed_through_publication_and_crash() {
    let sample = checked_sample("TypeInvariant", |observation| {
        let mut lease_ids = observation
            .pins
            .iter()
            .map(|pin| pin.lease_id)
            .collect::<Vec<_>>();
        lease_ids.sort_unstable();
        lease_ids.dedup();
        matches!(observation.catalog, CatalogRole::Old | CatalogRole::New) &&
            matches!(observation.old_generation, Presence::Present | Presence::Absent) &&
            matches!(observation.intent, IntentRole::Absent | IntentRole::Exact) &&
            observation
                .pins
                .iter()
                .all(|pin| !matches!(pin.class, PinClass::Other)) &&
            lease_ids.len() == observation.pins.len() &&
            observation.registry_leases == observation.pins.len() &&
            observation.pins_registered &&
            observation.refused_releases == 0
    });
    assert!(
        sample_observations(sample).any(|observation| {
            observation.holds_old_pin() && observation.holds_pin(PinClass::New)
        }),
        "no observation held old and new pins at once",
    );
    assert!(
        sample_observations(sample).any(|observation| observation.holds_pin(PinClass::OldAlias)),
        "no observation held an older-epoch alias pin",
    );
    assert!(
        sample_observations(sample).any(|observation| observation.crashed),
        "no crashed state was observed",
    );
    assert!(
        sample_observations(sample).any(|observation| observation.moment == GcMoment::Released),
        "no pin release was observed",
    );
}

#[test]
fn old_generation_is_reclaimed_only_after_the_successor_is_published() {
    let sample = checked_sample("PublicationBeforeReclamation", |observation| {
        matches!(observation.old_generation, Presence::Present) ||
            matches!(observation.catalog, CatalogRole::New)
    });
    assert!(
        sample_observations(sample)
            .any(|observation| observation.old_generation == Presence::Absent),
        "no reclaimed old generation was observed",
    );
    assert!(
        sample_observations(sample).any(|observation| {
            observation.moment == GcMoment::Probed(ProbeOutcome::Refused) &&
                observation.catalog == CatalogRole::Old &&
                observation.intent == IntentRole::Exact &&
                !observation.holds_old_pin()
        }),
        "no unpinned reclamation request met an intent written before the catalog swap",
    );
    assert!(
        sample_observations(sample).any(|observation| {
            matches!(
                observation.moment,
                GcMoment::Hook(
                    PublicationPoint::AfterCatalogSlotWrite |
                        PublicationPoint::AfterCatalogFileSyncBeforeDirectorySync
                )
            ) && observation.catalog == CatalogRole::New &&
                observation.old_generation == Presence::Present
        }),
        "no published catalog was observed before reclamation",
    );
}

#[test]
fn exact_old_generation_pins_defer_reclamation_until_released() {
    let sample = checked_sample("ExactOldPinsProtected", |observation| {
        !observation.holds_old_pin() || matches!(observation.old_generation, Presence::Present)
    });
    assert!(
        sample_observations(sample).any(|observation| {
            observation.catalog == CatalogRole::New &&
                observation.intent == IntentRole::Exact &&
                observation.holds_old_pin()
        }),
        "no old pin held a published successor's reclamation pending",
    );
    assert!(
        sample_observations(sample)
            .any(|observation| observation.moment == GcMoment::Probed(ProbeOutcome::Deferred)),
        "no reclamation request was deferred by a pin",
    );
    assert!(
        sample_observations(sample).any(|observation| {
            let mut old_pins = observation
                .pins
                .iter()
                .filter(|pin| pin.class.pins_old())
                .peekable();
            observation.moment == GcMoment::Hook(PublicationPoint::AfterCatalogDirectorySync) &&
                observation.old_generation == Presence::Present &&
                old_pins.peek().is_some() &&
                old_pins.all(|pin| {
                    matches!(
                        pin.origin,
                        PinOrigin::Hook(
                            PublicationPoint::AfterCatalogSlotWrite |
                                PublicationPoint::AfterCatalogFileSyncBeforeDirectorySync
                        )
                    )
                })
        }),
        "no pin taken after the catalog swap alone deferred the publication's reclamation",
    );
    assert!(
        sample
            .iter()
            .any(|trace| trace.refused_old_acquisitions > 0),
        "no pin on a reclaimed old generation was refused",
    );
}

#[test]
fn crash_before_publication_keeps_the_old_generation_for_retry() {
    let sample = checked_sample("CrashRetryAuthority", |observation| {
        !matches!(
            (observation.crashed, observation.catalog, observation.old_generation),
            (true, CatalogRole::Old, Presence::Absent | Presence::Partial)
        )
    });
    assert!(
        sample_observations(sample).any(|observation| {
            observation.crashed &&
                observation.catalog == CatalogRole::Old &&
                observation.intent == IntentRole::Exact
        }),
        "no crash left an intent written before the catalog swap",
    );
    assert!(
        sample_observations(sample).any(|observation| {
            observation.crashed &&
                observation.catalog == CatalogRole::Old &&
                observation.intent == IntentRole::Absent
        }),
        "no crash happened before the intent was written",
    );
    assert!(
        sample_observations(sample)
            .any(|observation| observation.crashed && observation.catalog == CatalogRole::New),
        "no crash happened after the catalog swap",
    );
    assert!(
        sample_observations(sample).any(|observation| {
            observation.crashed &&
                observation.crashes >= 2 &&
                observation.catalog == CatalogRole::Old
        }),
        "no second crash followed a recovery of the old authority",
    );
}

const OPERATION_ROLES: [RetainedRootKind; 3] =
    [RetainedRootKind::Reader, RetainedRootKind::Export, RetainedRootKind::Import];

const OPERATION_RELEASES: [OperationRelease; 3] = [
    OperationRelease::BeforePublication,
    OperationRelease::AfterPublication,
    OperationRelease::AfterRestart,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationRelease {
    BeforePublication,
    AfterPublication,
    AfterRestart,
}

#[derive(Clone, Copy, Debug)]
struct OperationRoot {
    role_slot: usize,
    root: prop::sample::Index,
    release: OperationRelease,
}

#[derive(Clone, Debug)]
struct QuotientCase {
    retained_union_objects: usize,
    earlier_publications: usize,
    operations: Vec<OperationRoot>,
}

#[derive(Debug, PartialEq, Eq)]
struct ProtectionRecord {
    registered_seeds: Vec<ObjectRefV1>,
    released_seeds: Vec<ObjectRefV1>,
    revisions: (u64, u64),
    leased_roots: Vec<(ObjectRefV1, u64)>,
    generation_lease_ids: Vec<u64>,
    registration_current: bool,
    registration_current_after_release: bool,
    registration_wrote_nothing: bool,
    releases_accepted: bool,
    old_after_publication: Presence,
    old_after_first_restart: Presence,
    old_after_second_restart: Presence,
    final_slot: CatalogSlotV1,
    final_directory: BTreeMap<String, Vec<u8>>,
}

struct ProtectionRun {
    record: ProtectionRecord,
    rows: Vec<RetainedRootV1>,
    manifest_roots: Vec<ObjectRefV1>,
    operation_roots: Vec<(ObjectRefV1, OperationRelease)>,
}

struct HeldOperation {
    release: OperationRelease,
    root_lease: RootLeaseV1,
    generation_lease: GenerationLeaseV1,
}

#[derive(Default)]
struct QuotientCoverage {
    shared_roots: usize,
    manifest_root_operations: usize,
    releases: [usize; 3],
    deferred: usize,
}

fn quotient_case() -> impl Strategy<Value = QuotientCase> {
    (
        2_usize..=4,
        0_usize..=1,
        prop::collection::vec(
            (0..OPERATION_ROLES.len(), any::<prop::sample::Index>(), 0..OPERATION_RELEASES.len()),
            1..=4,
        ),
    )
        .prop_map(|(retained_union_objects, earlier_publications, operations)| QuotientCase {
            retained_union_objects,
            earlier_publications,
            operations: operations
                .into_iter()
                .map(|(role_slot, root, release)| OperationRoot {
                    role_slot,
                    root,
                    release: OPERATION_RELEASES[release],
                })
                .collect(),
        })
}

fn release_operations(held: &mut Vec<HeldOperation>, point: OperationRelease) -> bool {
    let mut accepted = true;
    held.retain_mut(|operation| {
        if operation.release != point {
            return true;
        }
        let root_released = operation.root_lease.release().is_ok();
        let generation_released = operation.generation_lease.release().is_ok();
        accepted = accepted && root_released && generation_released;
        false
    });
    accepted
}

fn admitted_retention_manifest(writer: &GcWriter) -> Result<RetentionManifestV1, TestCaseError> {
    let view =
        RetentionManifestView::parse(writer.retention_manifest_bytes()).map_err(|error| {
            TestCaseError::fail(format!(
                "the admitted retention manifest does not parse: {error:?}"
            ))
        })?;
    let historical = view
        .historical()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            TestCaseError::fail(format!("a retained state does not parse: {error:?}"))
        })?;
    RetentionManifestV1::new(view.current(), historical).map_err(|error| {
        TestCaseError::fail(format!("the retention manifest does not rebuild: {error:?}"))
    })
}

fn expected_seeds(
    manifest_roots: &[ObjectRefV1],
    operation_roots: impl Iterator<Item = ObjectRefV1>,
) -> Vec<ObjectRefV1> {
    let mut seeds = manifest_roots.to_vec();
    seeds.extend(operation_roots);
    seeds.sort_unstable();
    seeds
}

fn run_protection(
    case: &QuotientCase,
    rotation: Option<usize>,
) -> Result<ProtectionRun, TestCaseError> {
    let scratch = trial_scratch("gc-quotient-");
    let root = scratch.path();
    let source = fixture_with_retained_union_cardinality(case.retained_union_objects);
    let objects = source
        .objects
        .iter()
        .map(|(reference, _)| *reference)
        .collect::<Vec<_>>();
    install_checkpoint_image(root, &checkpoint_image_from_source(source));
    let mut writer = admit_checkpoint(root).map_err(TestCaseError::fail)?;
    for _ in 0..case.earlier_publications {
        writer = publish_checkpoint(writer).map_err(TestCaseError::fail)?;
    }
    let manifest = admitted_retention_manifest(&writer)?;
    let historical_roots = manifest
        .historical()
        .iter()
        .map(|retained| retained.state().manifest())
        .collect::<BTreeSet<_>>();
    let mut manifest_roots = Vec::with_capacity(historical_roots.len() + 1);
    manifest_roots.push(manifest.current().state().manifest());
    manifest_roots.extend(historical_roots);
    let generation = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let old_files = generation_files(root, generation).map_err(TestCaseError::fail)?;
    let generations = writer.generation_leases();
    let roots = RetainedRootRegistryV1::default();
    let operations = match rotation {
        Some(_) => case.operations.as_slice(),
        None => &[],
    };
    let before_registration = directory_snapshot(root);
    let mut held = Vec::with_capacity(operations.len());
    let mut operation_roots = Vec::with_capacity(operations.len());
    let mut generation_lease_ids = Vec::with_capacity(operations.len());
    for operation in operations {
        let reference = objects[operation.root.index(objects.len())];
        let role =
            OPERATION_ROLES[(operation.role_slot + rotation.unwrap_or(0)) % OPERATION_ROLES.len()];
        let root_lease = roots.register(reference, role).map_err(|error| {
            TestCaseError::fail(format!("the root registry refused a root: {error}"))
        })?;
        let generation_lease = generations.acquire(generation).map_err(|error| {
            TestCaseError::fail(format!("the generation registry refused a pin: {error}"))
        })?;
        generation_lease_ids.push(generation_lease.lease_id());
        operation_roots.push((reference, operation.release));
        held.push(HeldOperation {
            release: operation.release,
            root_lease,
            generation_lease,
        });
    }
    let registry_refusal = |error: RetentionRegistryError| {
        TestCaseError::fail(format!("the root registry refused a snapshot: {error}"))
    };
    let registered = roots
        .snapshot_with_manifest(&manifest)
        .map_err(registry_refusal)?;
    let registration_current = roots
        .is_snapshot_current(&registered)
        .map_err(registry_refusal)?;
    let registration_wrote_nothing = directory_snapshot(root) == before_registration;
    let mut releases_accepted = release_operations(&mut held, OperationRelease::BeforePublication);
    let released = roots
        .snapshot_with_manifest(&manifest)
        .map_err(registry_refusal)?;
    let registration_current_after_release = roots
        .is_snapshot_current(&registered)
        .map_err(registry_refusal)?;
    let writer = publish_checkpoint(writer).map_err(TestCaseError::fail)?;
    let old_after_publication = generation_presence(&old_files);
    releases_accepted &= release_operations(&mut held, OperationRelease::AfterPublication);
    drop(writer);
    let writer = admit_checkpoint(root).map_err(TestCaseError::fail)?;
    let old_after_first_restart = generation_presence(&old_files);
    releases_accepted &= release_operations(&mut held, OperationRelease::AfterRestart);
    drop(writer);
    let writer = admit_checkpoint(root).map_err(TestCaseError::fail)?;
    let old_after_second_restart = generation_presence(&old_files);
    let final_slot = writer.catalog();
    drop(writer);
    let mut leased_roots = registered
        .roots()
        .iter()
        .map(|row| (row.root(), row.lease_id()))
        .collect::<Vec<_>>();
    leased_roots.sort_unstable();
    Ok(ProtectionRun {
        record: ProtectionRecord {
            registered_seeds: registered.object_refs().collect(),
            released_seeds: released.object_refs().collect(),
            revisions: (registered.revision(), released.revision()),
            leased_roots,
            generation_lease_ids,
            registration_current,
            registration_current_after_release,
            registration_wrote_nothing,
            releases_accepted,
            old_after_publication,
            old_after_first_restart,
            old_after_second_restart,
            final_slot,
            final_directory: directory_snapshot(root),
        },
        rows: registered.roots().to_vec(),
        manifest_roots,
        operation_roots,
    })
}

fn check_role_quotient(
    case: &QuotientCase,
    coverage: &RefCell<QuotientCoverage>,
) -> Result<(), TestCaseError> {
    let control = run_protection(case, None)?;
    let rotations = [
        run_protection(case, Some(0))?,
        run_protection(case, Some(1))?,
        run_protection(case, Some(2))?,
    ];
    let [first, second, third] = &rotations;
    prop_assert_eq!(&second.record, &first.record, "a role rotation changed the protection");
    prop_assert_eq!(&third.record, &first.record, "a role rotation changed the protection");
    prop_assert_ne!(&first.rows, &second.rows, "the registry did not record the roles");
    prop_assert_ne!(&first.rows, &third.rows, "the registry did not record the roles");
    prop_assert_ne!(&second.rows, &third.rows, "the registry did not record the roles");

    let record = &first.record;
    let operations = &first.operation_roots;
    let count = |point: OperationRelease| {
        operations
            .iter()
            .filter(|(_, release)| *release == point)
            .count()
    };
    let released_before = count(OperationRelease::BeforePublication);
    let held_after_restart = count(OperationRelease::AfterRestart);
    let presence = |held: bool| match held {
        true => Presence::Present,
        false => Presence::Absent,
    };
    prop_assert_eq!(
        &record.registered_seeds,
        &expected_seeds(&first.manifest_roots, operations.iter().map(|(root, _)| *root))
    );
    prop_assert_eq!(
        &record.released_seeds,
        &expected_seeds(
            &first.manifest_roots,
            operations
                .iter()
                .filter(|(_, release)| *release != OperationRelease::BeforePublication)
                .map(|(root, _)| *root),
        )
    );
    let registrations = u64::try_from(operations.len()).expect("operation counts fit u64");
    let releases = u64::try_from(released_before).expect("release counts fit u64");
    prop_assert_eq!(record.revisions, (registrations, registrations + releases));
    prop_assert!(record.registration_current);
    prop_assert_eq!(record.registration_current_after_release, released_before == 0);
    prop_assert!(record.registration_wrote_nothing);
    prop_assert!(record.releases_accepted);
    prop_assert_eq!(record.old_after_publication, presence(released_before < operations.len()));
    prop_assert_eq!(record.old_after_first_restart, presence(held_after_restart > 0));
    prop_assert_eq!(record.old_after_second_restart, Presence::Absent);
    prop_assert_eq!(&record.final_directory, &control.record.final_directory);
    prop_assert_eq!(record.final_slot, control.record.final_slot);
    prop_assert_eq!(
        &control.record.registered_seeds,
        &expected_seeds(&control.manifest_roots, std::iter::empty())
    );
    prop_assert_eq!(control.record.old_after_publication, Presence::Absent);

    let mut coverage = coverage.borrow_mut();
    let distinct_roots = operations
        .iter()
        .map(|(root, _)| *root)
        .collect::<BTreeSet<_>>();
    coverage.shared_roots += usize::from(distinct_roots.len() < operations.len());
    coverage.manifest_root_operations += operations
        .iter()
        .filter(|(root, _)| first.manifest_roots.contains(root))
        .count();
    for (slot, point) in OPERATION_RELEASES.iter().enumerate() {
        coverage.releases[slot] += count(*point);
    }
    coverage.deferred += usize::from(record.old_after_publication == Presence::Present);
    Ok(())
}

#[test]
fn reader_export_and_import_roots_share_one_protection_quotient() {
    let coverage = RefCell::new(QuotientCoverage::default());
    let config = ProptestConfig {
        cases: 16,
        failure_persistence: None,
        max_shrink_iters: 32,
        ..ProptestConfig::default()
    };
    let mut runner =
        TestRunner::new_with_rng(config, TestRng::deterministic_rng(RngAlgorithm::ChaCha));
    runner
        .run(&quotient_case(), |case| check_role_quotient(&case, &coverage))
        .expect("reader, export and import roots share one protection quotient");
    let coverage = coverage.into_inner();
    assert!(coverage.shared_roots > 0, "no case registered one root twice");
    assert!(
        coverage.manifest_root_operations > 0,
        "no operation registered a root that the retention manifest already seeds",
    );
    assert!(
        coverage.releases.iter().all(|count| *count > 0),
        "a release point was never exercised: {:?}",
        coverage.releases,
    );
    assert!(coverage.deferred > 0, "no operation held its generation across publication");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewTarget {
    Current,
    Retained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DurableView {
    PublishedCore(ViewTarget),
    RecoveredAuthority,
    RecoveredAuthorityAt(ViewTarget),
}

const DURABLE_VIEWS: [DurableView; 5] = [
    DurableView::PublishedCore(ViewTarget::Current),
    DurableView::PublishedCore(ViewTarget::Retained),
    DurableView::RecoveredAuthority,
    DurableView::RecoveredAuthorityAt(ViewTarget::Current),
    DurableView::RecoveredAuthorityAt(ViewTarget::Retained),
];

#[derive(Debug, PartialEq, Eq)]
struct ViewPinRecord {
    successor_is_fresh: bool,
    old_generation_while_alive: Presence,
    leases_while_alive: usize,
    exact_lease_while_alive: Result<bool, GenerationLeaseError>,
    retired_while_alive: Result<bool, GenerationLeaseError>,
    leases_after_drop: usize,
    retired_after_drop: Result<bool, GenerationLeaseError>,
}

fn hold_durable_view(
    view: DurableView,
    root: &Path,
    writer: GcWriter,
    retained: DurableStateRootV1,
    current: DurableStateRootV1,
) -> Result<(DurableViewReadHandleV1<TestSemanticProfile>, GcWriter), String> {
    let state = |target: ViewTarget| match target {
        ViewTarget::Current => current,
        ViewTarget::Retained => retained,
    };
    let recover = || {
        recover_read_authority::<TestSemanticProfile>(
            root,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("the read authority does not recover: {error:?}"))
    };
    match view {
        DurableView::PublishedCore(target) => {
            let (_, handle) = DurablePublishedCoreV1::detached_for_test(
                writer.shared_core(),
                writer.generation_leases(),
            )
            .read_handle(Some(state(target)))
            .map_err(|error| format!("the published core refused the view: {error:?}"))?;
            Ok((handle, writer))
        }
        DurableView::RecoveredAuthority => {
            drop(writer);
            let authority = recover()?;
            let handle = authority
                .read_handle()
                .map_err(|error| format!("the authority refused the view: {error:?}"))?;
            drop(authority);
            Ok((handle, admit_checkpoint(root)?))
        }
        DurableView::RecoveredAuthorityAt(target) => {
            drop(writer);
            let authority = recover()?;
            let resolved = authority
                .resolve(state(target))
                .map_err(|error| format!("the authority does not resolve the state: {error:?}"))?;
            let handle = authority
                .read_handle_at(&resolved)
                .map_err(|error| format!("the authority refused the view: {error:?}"))?;
            drop(authority);
            Ok((handle, admit_checkpoint(root)?))
        }
    }
}

fn pin_durable_view(view: DurableView) -> Result<ViewPinRecord, String> {
    let scratch = trial_scratch("gc-view-pin-");
    let root = scratch.path();
    install_checkpoint_image(root, &checkpoint_image_from_source(checkpoint_source_fixture()));
    let first = admit_checkpoint(root)?;
    let retained = DurableStateRootV1::of_manifest(first.catalog().state().manifest());
    let writer = publish_data_successor(first, "gc-view-pin");
    let current = DurableStateRootV1::of_manifest(writer.catalog().state().manifest());
    let old = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let old_files = generation_files(root, old)?;
    let (handle, writer) = hold_durable_view(view, root, writer, retained, current)?;
    let writer = publish_checkpoint(writer)?;
    let successor = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let registry = writer.generation_leases();
    drop(writer);
    let old_generation_while_alive = generation_presence(&old_files);
    let leases_while_alive = registry.active_lease_count_for_test();
    let exact_lease_while_alive = registry.has_active(old);
    let retired_while_alive = registry.retire_pack_range_if_unleased(old);
    drop(handle);
    Ok(ViewPinRecord {
        successor_is_fresh: successor.first_live_pack() > old.active_pack(),
        old_generation_while_alive,
        leases_while_alive,
        exact_lease_while_alive,
        retired_while_alive,
        leases_after_drop: registry.active_lease_count_for_test(),
        retired_after_drop: registry.retire_pack_range_if_unleased(old),
    })
}

#[test]
fn durable_views_on_an_old_generation_defer_reclamation_until_dropped() {
    let observed = DURABLE_VIEWS.map(|view| (view, pin_durable_view(view)));
    let expected = DURABLE_VIEWS.map(|view| {
        let protected = ViewPinRecord {
            successor_is_fresh: true,
            old_generation_while_alive: Presence::Present,
            leases_while_alive: 1,
            exact_lease_while_alive: Ok(true),
            retired_while_alive: Ok(false),
            leases_after_drop: 0,
            retired_after_drop: Ok(true),
        };
        (view, Ok::<ViewPinRecord, String>(protected))
    });
    assert_eq!(
        observed, expected,
        "GP-04: every durable view must keep its exact old generation present and unretired while \
         it lives, and give it up when it drops"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeRequest {
    Replay,
    Reset,
    Rollback,
    Clear,
    Spawn,
}

const RUNTIME_VIEW_REQUESTS: [RuntimeRequest; 5] = [
    RuntimeRequest::Replay,
    RuntimeRequest::Reset,
    RuntimeRequest::Rollback,
    RuntimeRequest::Clear,
    RuntimeRequest::Spawn,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileWrite {
    Created,
    Removed,
    Appended,
    Rewritten,
}

#[derive(Debug, PartialEq, Eq)]
struct RequestWrites {
    request: RuntimeRequest,
    resolved_target: bool,
    leases_added: Option<usize>,
    writes: BTreeMap<String, FileWrite>,
}

#[derive(Debug, PartialEq, Eq)]
struct CrashRecord {
    successor_is_fresh: bool,
    intent_while_viewed: IntentRole,
    viewed_generation_while_viewed: Presence,
    leases_after_crash: usize,
    intent_after_crash: IntentRole,
    viewed_generation_after_crash: Presence,
    successor_selected_after_recovery: bool,
    intent_after_recovery: IntentRole,
    viewed_generation_after_recovery: Presence,
    leases_after_recovery: usize,
    leases_with_successor_view: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct ObligationRecord {
    checkpoint_writes: BTreeMap<String, FileWrite>,
    requests: Vec<RequestWrites>,
    accepted_view_obligations: Vec<ReclamationCandidateV1>,
    persisted_view_obligation: Option<ReclamationCandidateV1>,
    crash: Result<CrashRecord, String>,
}

fn durable_writes(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> BTreeMap<String, FileWrite> {
    let names = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();
    let mut writes = BTreeMap::new();
    for name in names {
        let write = match (before.get(name), after.get(name)) {
            (Some(old), Some(new)) if old == new => continue,
            (Some(old), Some(new)) if new.len() > old.len() && new.starts_with(old) => {
                FileWrite::Appended
            }
            (Some(_), Some(_)) => FileWrite::Rewritten,
            (None, Some(_)) => FileWrite::Created,
            (Some(_), None) => FileWrite::Removed,
            (None, None) => continue,
        };
        writes.insert(name.clone(), write);
    }
    writes
}

fn reclamation_intent_role(
    root: &Path,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
) -> IntentRole {
    match fs::read(root.join("reclaim.v1")) {
        Err(error) if error.kind() == ErrorKind::NotFound => IntentRole::Absent,
        Err(_) => IntentRole::Other,
        Ok(bytes) => match ReclamationCandidateV1::decode(&bytes) {
            Ok(candidate) if candidate.old() == old && candidate.successor() == successor => {
                IntentRole::Exact
            }
            Ok(_) | Err(_) => IntentRole::Other,
        },
    }
}

fn crash_with_live_views(
    root: &Path,
    registry: GenerationLeaseRegistryV1,
    viewed: GenerationLeaseKeyV1,
    process: impl Sized,
) -> Result<CrashRecord, String> {
    let viewed_files = generation_files(root, viewed)?;
    let writer = DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .recover_and_admit_v1::<Profile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("admission with live views was refused: {error:?}"))?
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("the generation checkpoint was refused: {error:?}"))?;
    let successor = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let intent_while_viewed = reclamation_intent_role(root, viewed, successor);
    let viewed_generation_while_viewed = generation_presence(&viewed_files);
    drop((writer, process));
    let leases_after_crash = registry.active_lease_count_for_test();
    let intent_after_crash = reclamation_intent_role(root, viewed, successor);
    let viewed_generation_after_crash = generation_presence(&viewed_files);
    drop(registry);
    let authority = recover_read_authority::<Profile>(
        root,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .map_err(|error| format!("recovery after the crash was refused: {error:?}"))?;
    let registry = authority.generation_leases();
    let leases_after_recovery = registry.active_lease_count_for_test();
    let handle = authority
        .read_handle()
        .map_err(|error| format!("the recovered authority refused a view: {error:?}"))?;
    let leases_with_successor_view = registry.active_lease_count_for_test();
    drop(handle);
    Ok(CrashRecord {
        successor_is_fresh: successor.first_live_pack() > viewed.active_pack(),
        intent_while_viewed,
        viewed_generation_while_viewed,
        leases_after_crash,
        intent_after_crash,
        viewed_generation_after_crash,
        successor_selected_after_recovery: GenerationLeaseKeyV1::from_catalog(authority.catalog()) ==
            successor,
        intent_after_recovery: reclamation_intent_role(root, viewed, successor),
        viewed_generation_after_recovery: generation_presence(&viewed_files),
        leases_after_recovery,
        leases_with_successor_view,
    })
}

fn run_obligation_trial() -> Result<ObligationRecord, String> {
    let (scratch, hot, gate, mut owner) = owner_fixture();
    let root = scratch.path();
    let registry = owner.generation_leases();
    let first = owner.generation_key();
    let pre_state = Blake2b256Hash::from(owner.current_root());
    populate(&hot);
    let before = directory_snapshot(root);
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .map_err(|error| format!("the owner's checkpoint was refused: {error:?}"))?;
    let checkpoint_writes = durable_writes(&before, &directory_snapshot(root));
    let viewed = owner.generation_key();
    let repository = DurableHistoryRepositoryV1::new(owner.published_core())
        .map_err(|error| format!("the durable repository does not open: {error:?}"))?;
    let mut requests = Vec::with_capacity(RUNTIME_VIEW_REQUESTS.len());
    let mut views = Vec::with_capacity(RUNTIME_VIEW_REQUESTS.len());
    for request in RUNTIME_VIEW_REQUESTS {
        let target = match request {
            RuntimeRequest::Replay | RuntimeRequest::Rollback => pre_state.clone(),
            RuntimeRequest::Reset | RuntimeRequest::Spawn => repository.root(),
            RuntimeRequest::Clear => repository.empty_root(),
        };
        let before = directory_snapshot(root);
        let leases = registry.active_lease_count_for_test();
        let view = repository
            .reset(&target)
            .map_err(|error| format!("the {request:?} view was refused: {error:?}"))?;
        requests.push(RequestWrites {
            request,
            resolved_target: view.root() == target,
            leases_added: registry.active_lease_count_for_test().checked_sub(leases),
            writes: durable_writes(&before, &directory_snapshot(root)),
        });
        views.push(view);
    }
    owner
        .close()
        .map_err(|error| format!("the owner does not close: {error:?}"))?;
    let mut lease = DurableDirectoryV1::open(root)
        .and_then(DurableDirectoryV1::try_acquire_writer)
        .map_err(|error| format!("the closed owner left the writer lock taken: {error:?}"))?;
    let generations = [first, viewed];
    let mut accepted_view_obligations = Vec::with_capacity(generations.len() * generations.len());
    for old in generations {
        for successor in generations {
            if let Ok(candidate) = ReclamationCandidateV1::new(old, successor) {
                accepted_view_obligations.push(candidate);
            }
        }
    }
    if let Some(&candidate) = accepted_view_obligations.first() {
        lease
            .write_reclamation_candidate(candidate)
            .map_err(|error| format!("the reclamation intent was not written: {error:?}"))?;
    }
    let persisted_view_obligation = lease
        .read_reclamation_candidate()
        .map_err(|error| format!("the reclamation intent cannot be read: {error:?}"))?;
    drop(lease);
    let crash =
        crash_with_live_views(root, registry, viewed, (owner, hot, gate, repository, views));
    Ok(ObligationRecord {
        checkpoint_writes,
        requests,
        accepted_view_obligations,
        persisted_view_obligation,
        crash,
    })
}

#[test]
fn only_a_checkpoint_writes_and_no_replay_or_rollback_obligation_survives_a_crash() {
    let publication = BTreeMap::from([
        ("catalog.v1".to_owned(), FileWrite::Rewritten),
        (pack_file_name(PackId::GENESIS), FileWrite::Appended),
        (index_file_name(IndexGenerationV1::FIRST), FileWrite::Appended),
        ("preimage.v1".to_owned(), FileWrite::Created),
    ]);
    let requests = Vec::from(RUNTIME_VIEW_REQUESTS.map(|request| RequestWrites {
        request,
        resolved_target: true,
        leases_added: Some(1),
        writes: BTreeMap::new(),
    }));
    let expected = ObligationRecord {
        checkpoint_writes: publication,
        requests,
        accepted_view_obligations: Vec::new(),
        persisted_view_obligation: None,
        crash: Ok(CrashRecord {
            successor_is_fresh: true,
            intent_while_viewed: IntentRole::Exact,
            viewed_generation_while_viewed: Presence::Present,
            leases_after_crash: 0,
            intent_after_crash: IntentRole::Exact,
            viewed_generation_after_crash: Presence::Present,
            successor_selected_after_recovery: true,
            intent_after_recovery: IntentRole::Absent,
            viewed_generation_after_recovery: Presence::Absent,
            leases_after_recovery: 1,
            leases_with_successor_view: 2,
        }),
    };
    assert_eq!(
        run_obligation_trial(),
        Ok(expected),
        "GC-09 and GC-11: only the checkpoint may write durable bytes, every replay, reset, \
         rollback, clear and spawn view must write none and hold one process lease, the \
         reclamation intent must accept and persist no obligation over a generation the views \
         read, and a crash must leave no lease, so that recovery reclaims the viewed generation \
         before the first new lease"
    );
}

fn successor_generation(old: GenerationLeaseKeyV1) -> GenerationLeaseKeyV1 {
    GenerationLeaseKeyV1::new(
        old.epoch() + 1,
        old.active_pack() + 1,
        old.active_pack() + 1,
        old.index_generation() + 1,
    )
}

fn generation_role(
    key: GenerationLeaseKeyV1,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
) -> CatalogRole {
    match (key == old, key == successor) {
        (true, _) => CatalogRole::Old,
        (false, true) => CatalogRole::New,
        (false, false) => CatalogRole::Other,
    }
}

fn selected_generation_role(
    root: &Path,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
) -> CatalogRole {
    match selected_slot(root) {
        Ok(slot) => generation_role(GenerationLeaseKeyV1::from_catalog(slot), old, successor),
        Err(_) => CatalogRole::Other,
    }
}

fn request_reclamation(root: &Path) -> Result<ProbeOutcome, String> {
    let mut lease = DurableDirectoryV1::open(root)
        .and_then(DurableDirectoryV1::try_acquire_writer)
        .map_err(|error| {
            format!("the reclamation request cannot take the writer lock: {error:?}")
        })?;
    let candidate = lease
        .read_reclamation_candidate()
        .map_err(|error| format!("the reclamation request cannot read the intent: {error:?}"))?;
    match candidate {
        None => Ok(ProbeOutcome::NoIntent),
        Some(candidate) => {
            match lease.reclaim_reclamation_candidate(candidate, selected_slot(root)?) {
                Ok(reclaimed) if reclaimed.deferred_for_reader => Ok(ProbeOutcome::Deferred),
                Ok(_) => Ok(ProbeOutcome::Reclaimed),
                Err(DurableIoError::CatalogAuthorityChanged { .. }) => Ok(ProbeOutcome::Refused),
                Err(error) => Err(format!("the reclamation request failed: {error:?}")),
            }
        }
    }
}

fn admit_with_retention_writer(
    root: &Path,
    retention_writer: RetentionFormat,
) -> Result<GcWriter, String> {
    DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .with_retention_writer_for_test(retention_writer)
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("admission was refused: {error:?}"))
}

enum CompactionOutcome {
    Published(GcWriter),
    RootNotCarried(RetainedRootV1),
    Refused(String),
}

fn compact(writer: GcWriter) -> CompactionOutcome {
    match writer.compact_generation_v1(
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    ) {
        Ok(successor) => CompactionOutcome::Published(successor),
        Err(GenerationCheckpointPublicationErrorV1::RetainedRootNotCarried { root }) => {
            CompactionOutcome::RootNotCarried(root)
        }
        Err(error) => CompactionOutcome::Refused(format!("{error:?}")),
    }
}

fn on_trial_thread<T: Send + 'static>(
    name: &str,
    trial: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(trial)
        .expect("spawn a GC protection trial thread")
        .join()
        .unwrap_or_else(|panic| Err(panic_message(panic.as_ref())))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownRootChoice {
    CurrentState,
    HistoricalState,
    AppointedRetention,
    SupersededRetention,
}

#[derive(Clone, Copy, Debug)]
struct KnownStoreSpec {
    retained_union_objects: usize,
    earlier_publications: usize,
    retention_writer: RetentionFormat,
    data_successors: usize,
    root: KnownRootChoice,
}

const fn known_store(
    retained_union_objects: usize,
    earlier_publications: usize,
    retention_writer: RetentionFormat,
    data_successors: usize,
    root: KnownRootChoice,
) -> KnownStoreSpec {
    KnownStoreSpec {
        retained_union_objects,
        earlier_publications,
        retention_writer,
        data_successors,
        root,
    }
}

struct KnownStore {
    writer: GcWriter,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
    old_files: Vec<(PathBuf, Vec<u8>)>,
    known_root: ObjectRefV1,
    reader_root: ObjectRefV1,
    superseded_retention: Option<ObjectRefV1>,
}

fn open_known_store(spec: KnownStoreSpec, root: &Path) -> Result<KnownStore, String> {
    install_checkpoint_image(
        root,
        &checkpoint_image_from_source(fixture_with_retained_union_cardinality(
            spec.retained_union_objects,
        )),
    );
    let mut writer = admit_with_retention_writer(root, spec.retention_writer)?;
    let historical_state = RetentionManifestView::parse(writer.retention_manifest_bytes())
        .map_err(|error| format!("the installed retention manifest does not parse: {error:?}"))?
        .historical()
        .next()
        .and_then(Result::ok)
        .map(|retained| retained.state().manifest());
    for _ in 0..spec.earlier_publications {
        writer = publish_checkpoint(writer)?;
    }
    let mut superseded_retention = None;
    for successor in 0..spec.data_successors {
        superseded_retention = Some(writer.catalog().pack().retention_manifest());
        writer = publish_data_successor(writer, &format!("gc-known-root-{successor}"));
    }
    let known_root = match spec.root {
        KnownRootChoice::CurrentState => Some(writer.catalog().state().manifest()),
        KnownRootChoice::HistoricalState => historical_state,
        KnownRootChoice::AppointedRetention => Some(writer.catalog().pack().retention_manifest()),
        KnownRootChoice::SupersededRetention => superseded_retention,
    }
    .ok_or_else(|| format!("the store of {spec:?} holds no such root"))?;
    let old = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    Ok(KnownStore {
        old,
        successor: successor_generation(old),
        old_files: generation_files(root, old)?,
        known_root,
        reader_root: writer.catalog().state().manifest(),
        superseded_retention,
        writer,
    })
}

fn holds_known_closure(writer: &GcWriter, root: ObjectRefV1) -> bool {
    let store = writer.verified_state().object_store();
    let bytes = match writer.get_object_bounded(&root.digest(), root.encoded_len()) {
        Ok(Some(bytes)) if bytes.len() as u64 == root.encoded_len() => bytes,
        Ok(_) | Err(_) => return false,
    };
    let states: Vec<ObjectRefV1> =
        match (RetentionManifestView::parse(&bytes), RetentionAnchorV2::parse(&bytes)) {
            (Ok(manifest), _) => {
                match std::iter::once(Ok(manifest.current()))
                    .chain(manifest.historical())
                    .map(|retained| retained.map(|retained| retained.state().manifest()))
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(states) => states,
                    Err(_) => return false,
                }
            }
            (Err(_), Ok(anchor)) => match admit_retention_history_v2(store, anchor) {
                Ok(history) => std::iter::once(anchor.current())
                    .chain(history.entries().iter().copied())
                    .map(|retained| retained.state().manifest())
                    .collect(),
                Err(_) => return false,
            },
            (Err(_), Err(_)) => vec![root],
        };
    states
        .into_iter()
        .all(|manifest| verify_structural_closure(store, manifest).is_ok())
}

#[derive(Debug)]
struct KnownPublicationRefusal {
    names_the_lease: bool,
    wrote_nothing: bool,
    published_after_release: bool,
    root_absent_from_successor: bool,
}

#[derive(Debug)]
struct KnownPublicationRecord {
    published: bool,
    catalog: CatalogRole,
    successor_holds_closure: bool,
    refusal: Option<KnownPublicationRefusal>,
}

const KNOWN_PUBLICATION_SPECS: [KnownStoreSpec; 9] = [
    known_store(2, 0, RetentionFormat::V2, 0, KnownRootChoice::CurrentState),
    known_store(4, 1, RetentionFormat::V2, 0, KnownRootChoice::HistoricalState),
    known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::AppointedRetention),
    known_store(3, 1, RetentionFormat::V2, 1, KnownRootChoice::AppointedRetention),
    known_store(4, 0, RetentionFormat::V2, 2, KnownRootChoice::HistoricalState),
    known_store(3, 0, RetentionFormat::V1, 1, KnownRootChoice::AppointedRetention),
    known_store(3, 0, RetentionFormat::V2, 1, KnownRootChoice::SupersededRetention),
    known_store(4, 1, RetentionFormat::V2, 2, KnownRootChoice::SupersededRetention),
    known_store(3, 0, RetentionFormat::V1, 1, KnownRootChoice::SupersededRetention),
];

fn republish_after_release(
    root: &Path,
    retention_writer: RetentionFormat,
    known_root: ObjectRefV1,
) -> Result<(bool, bool), String> {
    Ok(match compact(admit_with_retention_writer(root, retention_writer)?) {
        CompactionOutcome::Published(published) => (
            true,
            matches!(
                published.get_object_bounded(&known_root.digest(), known_root.encoded_len()),
                Ok(None)
            ),
        ),
        CompactionOutcome::RootNotCarried(_) | CompactionOutcome::Refused(_) => (false, false),
    })
}

fn run_known_publication(spec: KnownStoreSpec) -> Result<KnownPublicationRecord, String> {
    let scratch = trial_scratch("gc-known-publication-");
    let root = scratch.path();
    let KnownStore {
        writer,
        old,
        successor,
        known_root,
        ..
    } = open_known_store(spec, root)?;
    let mut known = writer.register_known_root_v1(known_root).map_err(|error| {
        format!("the writer refused a Known lease on an object of its generation: {error}")
    })?;
    let before = directory_snapshot(root);
    let outcome = compact(writer);
    let catalog = selected_generation_role(root, old, successor);
    let (published, successor_holds_closure, refused) = match outcome {
        CompactionOutcome::Published(published) => {
            (true, holds_known_closure(&published, known_root), None)
        }
        CompactionOutcome::RootNotCarried(row) => (
            false,
            false,
            Some((
                (row.root(), row.lease_id()) == (known_root, known.lease_id()),
                directory_snapshot(root) == before,
            )),
        ),
        CompactionOutcome::Refused(reason) => {
            return Err(format!("the compaction was refused for another reason: {reason}"));
        }
    };
    known
        .release()
        .map_err(|error| format!("the Known lease was not released: {error}"))?;
    let refusal = refused
        .map(|(names_the_lease, wrote_nothing)| {
            republish_after_release(root, spec.retention_writer, known_root).map(
                |(published_after_release, root_absent_from_successor)| KnownPublicationRefusal {
                    names_the_lease,
                    wrote_nothing,
                    published_after_release,
                    root_absent_from_successor,
                },
            )
        })
        .transpose()?;
    Ok(KnownPublicationRecord {
        published,
        catalog,
        successor_holds_closure,
        refusal,
    })
}

#[test]
fn a_known_lease_active_at_publication_is_carried_by_the_successor_or_refuses_the_compaction() {
    let outcomes = KNOWN_PUBLICATION_SPECS.map(|spec| (spec, run_known_publication(spec)));
    for (spec, record) in outcomes
        .iter()
        .filter_map(|(spec, outcome)| outcome.as_ref().ok().map(|record| (spec, record)))
    {
        let known_at_publish = record.published || record.catalog == CatalogRole::New;
        assert!(
            !known_at_publish ||
                (record.catalog == CatalogRole::New && record.successor_holds_closure),
            "KnownPublicationClosureProtected is violated in trial {spec:?}: {record:?}",
        );
    }
    for (spec, outcome) in &outcomes {
        let record = match outcome {
            Ok(record) => record,
            Err(reason) => panic!("trial {spec:?} halted: {reason}"),
        };
        match (spec.root, &record.refusal) {
            (KnownRootChoice::SupersededRetention, Some(refusal)) => assert!(
                record.catalog == CatalogRole::Old &&
                    refusal.names_the_lease &&
                    refusal.wrote_nothing &&
                    refusal.published_after_release &&
                    refusal.root_absent_from_successor,
                "trial {spec:?} did not refuse an uncarried Known root cleanly: {record:?}",
            ),
            (KnownRootChoice::SupersededRetention, None) => {
                panic!("trial {spec:?} carried a superseded retention object: {record:?}")
            }
            (_, None) => assert!(
                record.published,
                "trial {spec:?} did not publish under a carried Known root: {record:?}",
            ),
            (_, Some(_)) => {
                panic!(
                    "trial {spec:?} refused a Known root that its checkpoint carries: {record:?}"
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownMoment {
    Registered,
    Hook(PublicationPoint),
    Published,
    Probed(ProbeOutcome),
    Readmitted,
    Released,
    RegisteredOnReclaimed,
    RegisteredOnSuccessor,
    RegisteredAbsentRoot,
}

#[derive(Clone, Copy, Debug)]
struct KnownGenerationObservation {
    moment: KnownMoment,
    known: Option<CatalogRole>,
    catalog: CatalogRole,
    old_generation: Presence,
    known_closure_in_successor: bool,
}

struct KnownGenerationRecorder {
    root: PathBuf,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
    old_files: Vec<(PathBuf, Vec<u8>)>,
    known: Option<CatalogRole>,
    observations: Vec<KnownGenerationObservation>,
}

impl KnownGenerationRecorder {
    fn hold(&mut self, generation: GenerationLeaseKeyV1) {
        self.known = Some(generation_role(generation, self.old, self.successor));
    }

    fn observe(&mut self, moment: KnownMoment, known_closure_in_successor: bool) {
        let observation = KnownGenerationObservation {
            moment,
            known: self.known,
            catalog: selected_generation_role(&self.root, self.old, self.successor),
            old_generation: generation_presence(&self.old_files),
            known_closure_in_successor,
        };
        self.observations.push(observation);
    }
}

struct KnownGenerationTrace {
    spec: KnownStoreSpec,
    observations: Vec<KnownGenerationObservation>,
    halt: Option<String>,
}

const KNOWN_GENERATION_SPECS: [KnownStoreSpec; 4] = [
    known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::AppointedRetention),
    known_store(2, 1, RetentionFormat::V2, 0, KnownRootChoice::CurrentState),
    known_store(4, 0, RetentionFormat::V2, 1, KnownRootChoice::AppointedRetention),
    known_store(3, 1, RetentionFormat::V1, 1, KnownRootChoice::HistoricalState),
];

fn arm_known_generation_chain(recorder: Rc<RefCell<KnownGenerationRecorder>>, index: usize) {
    let point = FORMAL_PUBLICATION_PHASE_ORDER[index];
    arm_generation_checkpoint_publication_hook(point, move || {
        recorder
            .borrow_mut()
            .observe(KnownMoment::Hook(point), false);
        if index + 1 < FORMAL_PUBLICATION_PHASE_ORDER.len() {
            arm_known_generation_chain(recorder, index + 1);
        }
    });
}

fn drive_known_generation(
    retention_writer: RetentionFormat,
    store: KnownStore,
    recorder: &Rc<RefCell<KnownGenerationRecorder>>,
) -> Result<(), String> {
    let KnownStore {
        writer,
        old,
        successor,
        known_root,
        superseded_retention,
        ..
    } = store;
    let root = recorder.borrow().root.clone();
    let generations = writer.generation_leases();
    let mut known = writer.register_known_root_v1(known_root).map_err(|error| {
        format!("the writer refused a Known lease on an object of its generation: {error}")
    })?;
    {
        let mut state = recorder.borrow_mut();
        state.hold(known.generation());
        state.observe(KnownMoment::Registered, false);
    }
    arm_known_generation_chain(Rc::clone(recorder), 0);
    let published = match compact(writer) {
        CompactionOutcome::Published(published) => published,
        CompactionOutcome::RootNotCarried(row) => {
            return Err(format!("the compaction refused the carried Known root {row:?}"));
        }
        CompactionOutcome::Refused(reason) => {
            return Err(format!("the compaction was refused: {reason}"));
        }
    };
    let carried = holds_known_closure(&published, known_root);
    recorder
        .borrow_mut()
        .observe(KnownMoment::Published, carried);
    drop(published);
    let deferred = request_reclamation(&root)?;
    recorder
        .borrow_mut()
        .observe(KnownMoment::Probed(deferred), false);
    drop(admit_with_retention_writer(&root, retention_writer)?);
    recorder
        .borrow_mut()
        .observe(KnownMoment::Readmitted, false);
    known
        .release()
        .map_err(|error| format!("the Known lease was not released: {error}"))?;
    {
        let mut state = recorder.borrow_mut();
        state.known = None;
        state.observe(KnownMoment::Released, false);
    }
    let reclaimed = request_reclamation(&root)?;
    recorder
        .borrow_mut()
        .observe(KnownMoment::Probed(reclaimed), false);
    match generations.register_known_root(known_root, old) {
        Ok(lease) => {
            let mut state = recorder.borrow_mut();
            state.hold(lease.generation());
            state.observe(KnownMoment::RegisteredOnReclaimed, false);
            state.known = None;
        }
        Err(KnownRootLeaseError::Generation(GenerationLeaseError::Retired(retired)))
            if retired == old =>
        {
            recorder
                .borrow_mut()
                .observe(KnownMoment::RegisteredOnReclaimed, false);
        }
        Err(error) => {
            return Err(format!("a Known lease on the reclaimed generation failed: {error}"));
        }
    }
    let writer = admit_with_retention_writer(&root, retention_writer)?;
    let fabricated =
        ObjectRefV1::new(ObjectDigest::from_array([0xA5; 32]), known_root.encoded_len())
            .map_err(|error| format!("the fabricated reference is refused: {error:?}"))?;
    let candidates = [
        Some((known_root, KnownMoment::RegisteredOnSuccessor)),
        Some((fabricated, KnownMoment::RegisteredAbsentRoot)),
        superseded_retention.map(|superseded| (superseded, KnownMoment::RegisteredAbsentRoot)),
    ];
    for (candidate, moment) in candidates.into_iter().flatten() {
        match writer.register_known_root_v1(candidate) {
            Ok(lease) => {
                let closed = holds_known_closure(&writer, candidate);
                let mut state = recorder.borrow_mut();
                state.hold(lease.generation());
                state.observe(moment, closed);
                state.known = None;
            }
            Err(KnownRootLeaseError::Absent {
                root: refused,
                generation,
            }) if (refused, generation) == (candidate, successor) => {
                recorder.borrow_mut().observe(moment, false);
            }
            Err(error) => {
                return Err(format!("a Known lease on the successor failed: {error}"));
            }
        }
    }
    Ok(())
}

fn run_known_generation(spec: KnownStoreSpec) -> KnownGenerationTrace {
    let scratch = trial_scratch("gc-known-generation-");
    let mut store = match open_known_store(spec, scratch.path()) {
        Ok(store) => store,
        Err(reason) => {
            return KnownGenerationTrace {
                spec,
                observations: Vec::new(),
                halt: Some(reason),
            };
        }
    };
    let recorder = Rc::new(RefCell::new(KnownGenerationRecorder {
        root: scratch.path().to_path_buf(),
        old: store.old,
        successor: store.successor,
        old_files: std::mem::take(&mut store.old_files),
        known: None,
        observations: Vec::with_capacity(FORMAL_PUBLICATION_PHASE_ORDER.len() + 12),
    }));
    let halt = drive_known_generation(spec.retention_writer, store, &recorder).err();
    let observations = std::mem::take(&mut recorder.borrow_mut().observations);
    KnownGenerationTrace {
        spec,
        observations,
        halt,
    }
}

fn known_generation_protected(observation: &KnownGenerationObservation) -> bool {
    match observation.known {
        None => true,
        Some(CatalogRole::Old) => observation.old_generation == Presence::Present,
        Some(CatalogRole::New) => observation.known_closure_in_successor,
        Some(CatalogRole::Other) => false,
    }
}

fn count_known_moments(
    trace: &KnownGenerationTrace,
    moment: KnownMoment,
    known: Option<CatalogRole>,
) -> usize {
    trace
        .observations
        .iter()
        .filter(|observation| (observation.moment, observation.known) == (moment, known))
        .count()
}

#[test]
fn a_known_lease_keeps_its_exact_generation_and_is_refused_for_roots_outside_it() {
    let traces = KNOWN_GENERATION_SPECS.map(|spec| {
        on_trial_thread("gc-known-generation-trial", move || Ok(run_known_generation(spec)))
            .unwrap_or_else(|reason| KnownGenerationTrace {
                spec,
                observations: Vec::new(),
                halt: Some(reason),
            })
    });
    for trace in &traces {
        if let Some(violation) = trace
            .observations
            .iter()
            .find(|observation| !known_generation_protected(observation))
        {
            panic!(
                "KnownExactGenerationProtected is violated in trial {:?}: {violation:?}",
                trace.spec
            );
        }
    }
    for trace in &traces {
        if let Some(reason) = &trace.halt {
            panic!("trial {:?} halted: {reason}", trace.spec);
        }
    }
    for trace in &traces {
        let counts = (
            count_known_moments(
                trace,
                KnownMoment::Probed(ProbeOutcome::Deferred),
                Some(CatalogRole::Old),
            ),
            count_known_moments(trace, KnownMoment::Probed(ProbeOutcome::Reclaimed), None),
            count_known_moments(trace, KnownMoment::RegisteredOnReclaimed, None),
            count_known_moments(trace, KnownMoment::RegisteredOnSuccessor, Some(CatalogRole::New)),
            count_known_moments(trace, KnownMoment::RegisteredAbsentRoot, None),
        );
        assert_eq!(
            counts,
            (1, 1, 1, 1, 1 + usize::from(trace.spec.data_successors > 0)),
            "trial {:?} did not defer the reclamation under its Known lease, reclaim after the \
             release, refuse the reclaimed generation, grant the carried root on the successor \
             and refuse the roots outside it",
            trace.spec
        );
        let hooks = trace
            .observations
            .iter()
            .filter(|observation| {
                matches!(observation.moment, KnownMoment::Hook(_)) &&
                    observation.known == Some(CatalogRole::Old)
            })
            .count();
        assert_eq!(
            hooks,
            FORMAL_PUBLICATION_PHASE_ORDER.len(),
            "trial {:?} did not hold its Known lease at every publication point",
            trace.spec
        );
        assert!(
            trace.observations.iter().any(|observation| {
                (observation.known, observation.catalog, observation.old_generation) ==
                    (Some(CatalogRole::Old), CatalogRole::New, Presence::Present)
            }),
            "trial {:?} never held its Known lease on the old generation under the successor \
             catalog",
            trace.spec
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlapToken {
    Known0,
    Reader0,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlapAction {
    AcquireOldPin(OverlapToken),
    AcquireNewPin(OverlapToken),
    ReleaseOldPin(OverlapToken),
    ReleaseNewPin(OverlapToken),
    Publish,
    Reclaim,
    Crash,
    Recover,
}

const PIN_OLD_KNOWN: OverlapAction = OverlapAction::AcquireOldPin(OverlapToken::Known0);
const PIN_OLD_READER: OverlapAction = OverlapAction::AcquireOldPin(OverlapToken::Reader0);
const PIN_NEW_KNOWN: OverlapAction = OverlapAction::AcquireNewPin(OverlapToken::Known0);
const PIN_NEW_READER: OverlapAction = OverlapAction::AcquireNewPin(OverlapToken::Reader0);
const DROP_OLD_KNOWN: OverlapAction = OverlapAction::ReleaseOldPin(OverlapToken::Known0);
const DROP_OLD_READER: OverlapAction = OverlapAction::ReleaseOldPin(OverlapToken::Reader0);
const DROP_NEW_KNOWN: OverlapAction = OverlapAction::ReleaseNewPin(OverlapToken::Known0);
const DROP_NEW_READER: OverlapAction = OverlapAction::ReleaseNewPin(OverlapToken::Reader0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OverlapModelState {
    crashed: bool,
    catalog: CatalogRole,
    old_generation: Presence,
    reclaim_intent: bool,
    known: Option<CatalogRole>,
    reader: Option<CatalogRole>,
    known_at_publish: bool,
    overlap_at_publish: bool,
    crashed_after_overlap: bool,
    known_after_reclaim: bool,
}

impl OverlapModelState {
    const INITIAL: Self = Self {
        crashed: false,
        catalog: CatalogRole::Old,
        old_generation: Presence::Present,
        reclaim_intent: false,
        known: None,
        reader: None,
        known_at_publish: false,
        overlap_at_publish: false,
        crashed_after_overlap: false,
        known_after_reclaim: false,
    };

    fn pin(self, token: OverlapToken) -> Option<CatalogRole> {
        match token {
            OverlapToken::Known0 => self.known,
            OverlapToken::Reader0 => self.reader,
        }
    }

    fn with_pin(self, token: OverlapToken, pin: Option<CatalogRole>) -> Self {
        match token {
            OverlapToken::Known0 => Self { known: pin, ..self },
            OverlapToken::Reader0 => Self {
                reader: pin,
                ..self
            },
        }
    }

    fn holds_old_pin(self) -> bool { [self.known, self.reader].contains(&Some(CatalogRole::Old)) }

    fn step(self, action: OverlapAction) -> Option<Self> {
        let old_present = self.old_generation == Presence::Present;
        match (action, self.crashed) {
            (OverlapAction::Recover, true) => Some(Self {
                crashed: false,
                ..self
            }),
            (OverlapAction::Recover, false) | (_, true) => None,
            (OverlapAction::Crash, false) => Some(Self {
                crashed: true,
                known: None,
                reader: None,
                crashed_after_overlap: self.crashed_after_overlap ||
                    (self.overlap_at_publish && old_present),
                ..self
            }),
            (OverlapAction::AcquireOldPin(token), false) => match (old_present, self.pin(token)) {
                (true, None) => Some(self.with_pin(token, Some(CatalogRole::Old))),
                (true, Some(_)) | (false, _) => None,
            },
            (OverlapAction::AcquireNewPin(token), false) => match (self.catalog, self.pin(token)) {
                (CatalogRole::New, None) => Some(Self {
                    known_after_reclaim: self.known_after_reclaim ||
                        (token == OverlapToken::Known0 &&
                            self.crashed_after_overlap &&
                            !old_present),
                    ..self.with_pin(token, Some(CatalogRole::New))
                }),
                (CatalogRole::New, Some(_)) | (CatalogRole::Old | CatalogRole::Other, _) => None,
            },
            (OverlapAction::ReleaseOldPin(token), false) => match self.pin(token) {
                Some(CatalogRole::Old) => Some(self.with_pin(token, None)),
                Some(CatalogRole::New | CatalogRole::Other) | None => None,
            },
            (OverlapAction::ReleaseNewPin(token), false) => match self.pin(token) {
                Some(CatalogRole::New) => Some(self.with_pin(token, None)),
                Some(CatalogRole::Old | CatalogRole::Other) | None => None,
            },
            (OverlapAction::Publish, false) => match self.catalog {
                CatalogRole::Old => Some(Self {
                    catalog: CatalogRole::New,
                    reclaim_intent: true,
                    known_at_publish: self.known.is_some(),
                    overlap_at_publish: self.known.is_some() && self.reader.is_some(),
                    ..self
                }),
                CatalogRole::New | CatalogRole::Other => None,
            },
            (OverlapAction::Reclaim, false) => {
                match (self.catalog, self.reclaim_intent, self.holds_old_pin()) {
                    (CatalogRole::New, true, false) => Some(Self {
                        old_generation: Presence::Absent,
                        reclaim_intent: false,
                        ..self
                    }),
                    _ => None,
                }
            }
        }
    }

    fn witness_history_sound(self) -> bool {
        (!self.overlap_at_publish || (self.known_at_publish && self.catalog == CatalogRole::New)) &&
            (!self.crashed_after_overlap || self.overlap_at_publish) &&
            (!self.known_after_reclaim ||
                (self.crashed_after_overlap &&
                    self.old_generation == Presence::Absent &&
                    self.catalog == CatalogRole::New))
    }
}

struct OverlapWorld {
    root: PathBuf,
    old: GenerationLeaseKeyV1,
    successor: GenerationLeaseKeyV1,
    old_files: Vec<(PathBuf, Vec<u8>)>,
    known_root: ObjectRefV1,
    reader_root: ObjectRefV1,
    writer: Option<GcWriter>,
    generations: Option<GenerationLeaseRegistryV1>,
    known: Option<KnownRootLeaseV1>,
    reader: Option<(RootLeaseV1, GenerationLeaseV1)>,
    known_at_publish: bool,
    overlap_at_publish: bool,
    crashed_after_overlap: bool,
    known_after_reclaim: bool,
}

impl OverlapWorld {
    fn start(spec: KnownStoreSpec, root: &Path) -> Result<Self, String> {
        let KnownStore {
            writer,
            old,
            successor,
            old_files,
            known_root,
            reader_root,
            ..
        } = open_known_store(spec, root)?;
        Ok(Self {
            root: root.to_path_buf(),
            old,
            successor,
            old_files,
            known_root,
            reader_root,
            generations: Some(writer.generation_leases()),
            writer: Some(writer),
            known: None,
            reader: None,
            known_at_publish: false,
            overlap_at_publish: false,
            crashed_after_overlap: false,
            known_after_reclaim: false,
        })
    }

    fn observed_pin(
        &self,
        generations: &GenerationLeaseRegistryV1,
        rows: &[RetainedRootV1],
        pin: Option<(GenerationLeaseKeyV1, u64)>,
    ) -> Result<Option<CatalogRole>, String> {
        match pin {
            None => Ok(None),
            Some((generation, lease_id)) => {
                let registered = rows.iter().any(|row| row.lease_id() == lease_id);
                match (generations.has_active(generation), registered) {
                    (Ok(true), true) => {
                        Ok(Some(generation_role(generation, self.old, self.successor)))
                    }
                    (leased, registered) => Err(format!(
                        "a lease on {generation:?} is not registered: generation lease \
                         {leased:?}, root row {registered}"
                    )),
                }
            }
        }
    }

    fn observe(&self) -> Result<OverlapModelState, String> {
        let catalog = selected_generation_role(&self.root, self.old, self.successor);
        let intent = reclamation_intent_role(&self.root, self.old, self.successor);
        let reclaim_intent = match (intent, catalog) {
            (IntentRole::Exact, CatalogRole::New) => true,
            (IntentRole::Absent, _) | (IntentRole::Exact, CatalogRole::Old) => false,
            (IntentRole::Exact | IntentRole::Other, _) => {
                return Err(format!(
                    "the reclamation intent is {intent:?} under catalog {catalog:?}"
                ));
            }
        };
        let known = self
            .known
            .as_ref()
            .map(|lease| (lease.generation(), lease.lease_id()));
        let reader = self
            .reader
            .as_ref()
            .map(|(row, pin)| (pin.key(), row.lease_id()));
        let held = usize::from(known.is_some()) + usize::from(reader.is_some());
        let (known, reader) = match (&self.generations, held) {
            (None, 0) => (None, None),
            (None, _) => return Err(format!("{held} leases outlived the crash")),
            (Some(generations), _) => {
                let snapshot = generations
                    .retained_roots()
                    .and_then(|roots| roots.snapshot())
                    .map_err(|error| format!("the root registry cannot be read: {error}"))?;
                let leases = generations.active_lease_count_for_test();
                let rows = snapshot.roots();
                if (leases, rows.len()) != (held, held) {
                    return Err(format!(
                        "the registries hold {leases} generation leases and {} root rows for \
                         {held} leases of the schedule",
                        rows.len()
                    ));
                }
                (
                    self.observed_pin(generations, rows, known)?,
                    self.observed_pin(generations, rows, reader)?,
                )
            }
        };
        Ok(OverlapModelState {
            crashed: self.generations.is_none(),
            catalog,
            old_generation: generation_presence(&self.old_files),
            reclaim_intent,
            known,
            reader,
            known_at_publish: self.known_at_publish,
            overlap_at_publish: self.overlap_at_publish,
            crashed_after_overlap: self.crashed_after_overlap,
            known_after_reclaim: self.known_after_reclaim,
        })
    }

    fn admitted(&mut self) -> Result<&GcWriter, String> {
        let generations = self
            .generations
            .clone()
            .ok_or_else(|| "the crashed store admits no writer".to_owned())?;
        let writer = match self.writer.take() {
            Some(writer) => writer,
            None => {
                let state = self.observe()?;
                match (state.reclaim_intent, state.holds_old_pin()) {
                    (true, false) => {
                        return Err(
                            "an admission here would take the model's Reclaim step".to_owned()
                        );
                    }
                    (false, _) | (true, true) => admit_checkpoint(&self.root)?,
                }
            }
        };
        if !generations.shares_registry(&writer.generation_leases()) {
            return Err("the admitted writer does not share the registry of the leases".to_owned());
        }
        Ok(&*self.writer.insert(writer))
    }

    fn acquire_old(
        &mut self,
        token: OverlapToken,
        generations: &GenerationLeaseRegistryV1,
    ) -> Result<bool, String> {
        let (old, known_root, reader_root) = (self.old, self.known_root, self.reader_root);
        match (token, self.known.is_some(), self.reader.is_some()) {
            (OverlapToken::Known0, true, _) | (OverlapToken::Reader0, _, true) => {
                Err(format!("the schedule leases {token:?} twice"))
            }
            (OverlapToken::Known0, false, _) => {
                let granted = match selected_generation_role(&self.root, old, self.successor) {
                    CatalogRole::Old => self.admitted()?.register_known_root_v1(known_root),
                    CatalogRole::New | CatalogRole::Other => {
                        generations.register_known_root(known_root, old)
                    }
                };
                match granted {
                    Ok(lease) => {
                        self.known = Some(lease);
                        Ok(true)
                    }
                    Err(KnownRootLeaseError::Generation(GenerationLeaseError::Retired(
                        retired,
                    ))) if retired == old => Ok(false),
                    Err(error) => {
                        Err(format!("a Known lease on the old generation failed: {error}"))
                    }
                }
            }
            (OverlapToken::Reader0, _, false) => match generations.acquire(old) {
                Ok(pin) => {
                    let row = generations
                        .retained_roots()
                        .and_then(|roots| roots.register(reader_root, RetainedRootKind::Reader))
                        .map_err(|error| format!("the reader root was not registered: {error}"))?;
                    self.reader = Some((row, pin));
                    Ok(true)
                }
                Err(GenerationLeaseError::Retired(retired)) if retired == old => Ok(false),
                Err(error) => Err(format!("a reader lease on the old generation failed: {error}")),
            },
        }
    }

    fn acquire_new(
        &mut self,
        token: OverlapToken,
        generations: &GenerationLeaseRegistryV1,
    ) -> Result<bool, String> {
        let (successor, known_root, reader_root) =
            (self.successor, self.known_root, self.reader_root);
        let catalog = selected_generation_role(&self.root, self.old, successor);
        match (catalog, token, self.known.is_some(), self.reader.is_some()) {
            (CatalogRole::Old | CatalogRole::Other, _, _, _) => Ok(false),
            (CatalogRole::New, OverlapToken::Known0, true, _) |
            (CatalogRole::New, OverlapToken::Reader0, _, true) => {
                Err(format!("the schedule leases {token:?} twice"))
            }
            (CatalogRole::New, OverlapToken::Known0, false, _) => {
                let writer = self.admitted()?;
                let lease = writer
                    .register_known_root_v1(known_root)
                    .map_err(|error| format!("a Known lease on the successor failed: {error}"))?;
                match (lease.generation() == successor, holds_known_closure(writer, known_root)) {
                    (true, true) => {}
                    (on_successor, closed) => {
                        return Err(format!(
                            "the Known lease on the successor is unsound: on the successor \
                             {on_successor}, closure held {closed}"
                        ));
                    }
                }
                self.known_after_reclaim |= self.crashed_after_overlap &&
                    generation_presence(&self.old_files) == Presence::Absent;
                self.known = Some(lease);
                Ok(true)
            }
            (CatalogRole::New, OverlapToken::Reader0, _, false) => {
                let pin = generations
                    .acquire(successor)
                    .map_err(|error| format!("a reader lease on the successor failed: {error}"))?;
                let row = generations
                    .retained_roots()
                    .and_then(|roots| roots.register(reader_root, RetainedRootKind::Reader))
                    .map_err(|error| format!("the reader root was not registered: {error}"))?;
                self.reader = Some((row, pin));
                Ok(true)
            }
        }
    }

    fn release(
        &mut self,
        token: OverlapToken,
        generation: GenerationLeaseKeyV1,
    ) -> Result<bool, String> {
        match token {
            OverlapToken::Known0 => match self.known.take() {
                Some(mut lease) if lease.generation() == generation => {
                    lease
                        .release()
                        .map_err(|error| format!("the Known lease was not released: {error}"))?;
                    Ok(true)
                }
                held => {
                    self.known = held;
                    Ok(false)
                }
            },
            OverlapToken::Reader0 => match self.reader.take() {
                Some((mut row, mut pin)) if pin.key() == generation => {
                    row.release()
                        .map_err(|error| format!("the reader root was not released: {error}"))?;
                    pin.release()
                        .map_err(|error| format!("the reader lease was not released: {error}"))?;
                    Ok(true)
                }
                held => {
                    self.reader = held;
                    Ok(false)
                }
            },
        }
    }

    fn take_old_writer(&mut self) -> Result<GcWriter, String> {
        let old = self.old;
        if !self.observe()?.holds_old_pin() {
            return Err("a publication without a lease on the old generation takes the model's \
                        Reclaim step as well"
                .to_owned());
        }
        self.admitted()?;
        match self.writer.take() {
            Some(writer) if GenerationLeaseKeyV1::from_catalog(writer.catalog()) == old => {
                Ok(writer)
            }
            Some(_) | None => Err("the schedule publishes a second successor".to_owned()),
        }
    }

    fn note_publication(&mut self) -> Result<(), String> {
        let state = self.observe()?;
        self.known_at_publish = state.known.is_some();
        self.overlap_at_publish = state.known.is_some() && state.reader.is_some();
        Ok(())
    }

    fn adopt_successor(&mut self, outcome: CompactionOutcome) -> Result<(), String> {
        match outcome {
            CompactionOutcome::Published(writer) => {
                self.writer = Some(writer);
                self.note_publication()
            }
            CompactionOutcome::RootNotCarried(row) => {
                Err(format!("the compaction refused the carried root {row:?}"))
            }
            CompactionOutcome::Refused(reason) => {
                Err(format!("the compaction was refused: {reason}"))
            }
        }
    }

    fn successor_presence(&self) -> Result<Presence, String> {
        let pack = PackId::new(self.successor.active_pack())
            .map_err(|error| format!("invalid successor pack: {error:?}"))?;
        let index = IndexGenerationV1::new(self.successor.index_generation())
            .map_err(|error| format!("invalid successor index generation: {error:?}"))?;
        let present = [
            self.root.join(pack_file_name(pack)).exists(),
            self.root.join(index_file_name(index)).exists(),
        ];
        Ok(match present {
            [true, true] => Presence::Present,
            [false, false] => Presence::Absent,
            [true, false] | [false, true] => Presence::Partial,
        })
    }

    fn observe_copy_point(
        &mut self,
        publication: bool,
    ) -> Result<(OverlapModelState, Presence), String> {
        if publication {
            self.note_publication()?;
        }
        Ok((self.observe()?, self.successor_presence()?))
    }

    fn successor_holds_closures(&mut self) -> Result<bool, String> {
        let (successor, known_root, reader_root) =
            (self.successor, self.known_root, self.reader_root);
        let writer = self.admitted()?;
        Ok(GenerationLeaseKeyV1::from_catalog(writer.catalog()) == successor &&
            holds_known_closure(writer, known_root) &&
            holds_known_closure(writer, reader_root))
    }

    fn apply(&mut self, action: OverlapAction) -> Result<bool, String> {
        match (action, self.generations.clone()) {
            (OverlapAction::Recover, None) => {
                let directory = DurableDirectoryV1::open(self.root.as_path()).map_err(|error| {
                    format!("the crashed store directory does not open: {error:?}")
                })?;
                self.generations = Some(directory.generation_leases());
                Ok(true)
            }
            (OverlapAction::Recover, Some(_)) | (_, None) => Ok(false),
            (OverlapAction::Crash, Some(_)) => {
                let state = self.observe()?;
                self.crashed_after_overlap |=
                    state.overlap_at_publish && state.old_generation == Presence::Present;
                self.writer = None;
                self.known = None;
                self.reader = None;
                self.generations = None;
                Ok(true)
            }
            (OverlapAction::AcquireOldPin(token), Some(generations)) => {
                self.acquire_old(token, &generations)
            }
            (OverlapAction::AcquireNewPin(token), Some(generations)) => {
                self.acquire_new(token, &generations)
            }
            (OverlapAction::ReleaseOldPin(token), Some(_)) => self.release(token, self.old),
            (OverlapAction::ReleaseNewPin(token), Some(_)) => self.release(token, self.successor),
            (OverlapAction::Publish, Some(_)) => {
                let writer = self.take_old_writer()?;
                self.adopt_successor(compact(writer))?;
                Ok(true)
            }
            (OverlapAction::Reclaim, Some(_)) => {
                self.writer = None;
                Ok(request_reclamation(&self.root)? == ProbeOutcome::Reclaimed)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct OverlapSchedule {
    name: &'static str,
    store: KnownStoreSpec,
    actions: &'static [OverlapAction],
}

struct OverlapHistory {
    name: &'static str,
    steps: Vec<(OverlapAction, bool, OverlapModelState)>,
    halt: Option<String>,
}

const OVERLAP_SCHEDULES: [OverlapSchedule; 6] = [
    OverlapSchedule {
        name: "known-alone-keeps-the-old-generation",
        store: known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::AppointedRetention),
        actions: &[
            PIN_OLD_KNOWN,
            PIN_OLD_READER,
            OverlapAction::Publish,
            DROP_OLD_READER,
            OverlapAction::Reclaim,
            PIN_NEW_READER,
            OverlapAction::Crash,
            OverlapAction::Recover,
            OverlapAction::Reclaim,
            PIN_NEW_KNOWN,
            PIN_NEW_READER,
            DROP_NEW_KNOWN,
            DROP_NEW_READER,
        ],
    },
    OverlapSchedule {
        name: "reader-alone-at-publication",
        store: known_store(2, 1, RetentionFormat::V2, 0, KnownRootChoice::CurrentState),
        actions: &[
            PIN_OLD_READER,
            OverlapAction::Publish,
            OverlapAction::Crash,
            OverlapAction::Recover,
            OverlapAction::Reclaim,
            PIN_NEW_KNOWN,
            DROP_NEW_KNOWN,
        ],
    },
    OverlapSchedule {
        name: "reclaimed-before-the-crash",
        store: known_store(4, 0, RetentionFormat::V2, 1, KnownRootChoice::CurrentState),
        actions: &[
            PIN_OLD_KNOWN,
            PIN_OLD_READER,
            OverlapAction::Publish,
            DROP_OLD_KNOWN,
            DROP_OLD_READER,
            OverlapAction::Reclaim,
            PIN_OLD_KNOWN,
            PIN_OLD_READER,
            OverlapAction::Crash,
            OverlapAction::Recover,
            PIN_NEW_KNOWN,
        ],
    },
    OverlapSchedule {
        name: "known-alone-at-publication",
        store: known_store(3, 1, RetentionFormat::V2, 0, KnownRootChoice::HistoricalState),
        actions: &[
            PIN_OLD_KNOWN,
            OverlapAction::Publish,
            OverlapAction::Reclaim,
            OverlapAction::Crash,
            OverlapAction::Recover,
            OverlapAction::Reclaim,
            PIN_NEW_KNOWN,
        ],
    },
    OverlapSchedule {
        name: "crash-before-publication",
        store: known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::CurrentState),
        actions: &[
            PIN_OLD_KNOWN,
            PIN_OLD_READER,
            OverlapAction::Crash,
            OverlapAction::Recover,
            PIN_NEW_KNOWN,
            OverlapAction::Reclaim,
            PIN_OLD_KNOWN,
            OverlapAction::Publish,
            DROP_OLD_KNOWN,
            OverlapAction::Reclaim,
            PIN_NEW_KNOWN,
        ],
    },
    OverlapSchedule {
        name: "known-moves-to-the-successor-before-the-crash",
        store: known_store(4, 1, RetentionFormat::V2, 1, KnownRootChoice::AppointedRetention),
        actions: &[
            PIN_OLD_KNOWN,
            PIN_OLD_READER,
            OverlapAction::Publish,
            DROP_OLD_KNOWN,
            PIN_NEW_KNOWN,
            OverlapAction::Crash,
            OverlapAction::Recover,
            OverlapAction::Reclaim,
            PIN_NEW_KNOWN,
            PIN_OLD_READER,
        ],
    },
];

fn drive_overlap_schedule(
    schedule: OverlapSchedule,
    steps: &mut Vec<(OverlapAction, bool, OverlapModelState)>,
) -> Result<(), String> {
    let scratch = trial_scratch("gc-overlap-history-");
    let mut world = OverlapWorld::start(schedule.store, scratch.path())?;
    let mut model = OverlapModelState::INITIAL;
    let initial = world.observe()?;
    if initial != model {
        return Err(format!(
            "the admitted runtime state {initial:?} is not the model's initial state"
        ));
    }
    for (index, action) in schedule.actions.iter().copied().enumerate() {
        let enabled = model.step(action);
        let taken = world
            .apply(action)
            .map_err(|reason| format!("step {index} {action:?}: {reason}"))?;
        let observed = world
            .observe()
            .map_err(|reason| format!("step {index} after {action:?}: {reason}"))?;
        steps.push((action, taken, observed));
        model = match (enabled, taken) {
            (Some(next), true) => next,
            (None, false) => model,
            (Some(_), false) => {
                return Err(format!(
                    "step {index}: the runtime refused {action:?}, which the model enables in \
                     {model:?}"
                ));
            }
            (None, true) => {
                return Err(format!(
                    "step {index}: the runtime took {action:?}, which the model disables in \
                     {model:?}"
                ));
            }
        };
        if observed != model {
            return Err(format!(
                "step {index}: after {action:?} the runtime state {observed:?} is not the model \
                 state {model:?}"
            ));
        }
    }
    Ok(())
}

fn run_overlap_schedule(schedule: OverlapSchedule) -> OverlapHistory {
    let mut steps = Vec::with_capacity(schedule.actions.len());
    let halt = drive_overlap_schedule(schedule, &mut steps).err();
    OverlapHistory {
        name: schedule.name,
        steps,
        halt,
    }
}

#[test]
fn the_overlap_witness_history_of_the_runtime_is_a_history_of_the_model() {
    let histories = OVERLAP_SCHEDULES.map(|schedule| {
        on_trial_thread(schedule.name, move || Ok(run_overlap_schedule(schedule))).unwrap_or_else(
            |reason| OverlapHistory {
                name: schedule.name,
                steps: Vec::new(),
                halt: Some(reason),
            },
        )
    });
    for history in &histories {
        for (index, (action, _, state)) in history.steps.iter().enumerate() {
            assert!(
                state.witness_history_sound(),
                "OverlapWitnessHistorySound is violated in schedule {} after step {index} \
                 {action:?}: {state:?}",
                history.name
            );
        }
    }
    for history in &histories {
        if let Some(reason) = &history.halt {
            panic!("schedule {} left the model: {reason}", history.name);
        }
    }
    let steps = || histories.iter().flat_map(|history| history.steps.iter());
    let witnessed = [
        (
            steps().any(|(_, _, state)| state.overlap_at_publish),
            "a publication under a Known lease and a reader lease",
        ),
        (
            steps()
                .any(|(_, _, state)| state.catalog == CatalogRole::New && !state.known_at_publish),
            "a publication without a Known lease",
        ),
        (
            steps().any(|(_, _, state)| state.known_at_publish && !state.overlap_at_publish),
            "a publication under a Known lease alone",
        ),
        (
            steps().any(|(_, _, state)| state.crashed_after_overlap),
            "a crash after the overlap while the old generation is present",
        ),
        (
            steps().any(|(_, _, state)| {
                state.crashed && state.overlap_at_publish && !state.crashed_after_overlap
            }),
            "a crash after the overlap once the old generation is reclaimed",
        ),
        (
            steps().any(|(_, _, state)| state.known_after_reclaim),
            "a new Known lease after the crash, the recovery and the reclamation",
        ),
        (
            steps().any(|(_, _, state)| {
                state.known == Some(CatalogRole::New) &&
                    state.old_generation == Presence::Absent &&
                    !state.known_after_reclaim
            }),
            "a new Known lease after a reclamation that no crash after an overlap preceded",
        ),
    ];
    for (observed, description) in witnessed {
        assert!(observed, "no schedule observed {description}");
    }
    for refused in [OverlapAction::Reclaim, PIN_OLD_KNOWN, PIN_OLD_READER, PIN_NEW_KNOWN] {
        assert!(
            steps().any(|(action, taken, _)| (*action, *taken) == (refused, false)),
            "no schedule had the runtime refuse {refused:?}",
        );
    }
}

const OVERLAP_WITNESS_STORE: KnownStoreSpec =
    known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::AppointedRetention);

const OVERLAP_WITNESS_COPY_POINTS: [(PublicationPoint, &str); 3] = [
    (PublicationPoint::AfterPreflightBeforeCleanup, "StartCopy"),
    (PublicationPoint::AfterCommitSync, "CopyOne"),
    (PublicationPoint::AfterCatalogFileSyncBeforeDirectorySync, "Publish"),
];

type OverlapCopyObservation = (&'static str, Result<(OverlapModelState, Presence), String>);

fn arm_overlap_copy_observation(
    world: Rc<RefCell<OverlapWorld>>,
    observed: Rc<RefCell<Vec<OverlapCopyObservation>>>,
    index: usize,
) {
    let (point, action) = OVERLAP_WITNESS_COPY_POINTS[index];
    let last = index + 1 == OVERLAP_WITNESS_COPY_POINTS.len();
    arm_generation_checkpoint_publication_hook(point, move || {
        let observation = world.borrow_mut().observe_copy_point(last);
        observed.borrow_mut().push((action, observation));
        if !last {
            arm_overlap_copy_observation(world, observed, index + 1);
        }
    });
}

fn replay_overlap_step(
    world: &Rc<RefCell<OverlapWorld>>,
    model: &mut OverlapModelState,
    action: OverlapAction,
) -> Result<(), String> {
    let expected = model
        .step(action)
        .ok_or_else(|| format!("{action:?}: the model disables the step in {model:?}"))?;
    let mut state = world.borrow_mut();
    match state.apply(action) {
        Ok(true) => {}
        Ok(false) => return Err(format!("{action:?}: the runtime refused the step")),
        Err(reason) => return Err(format!("{action:?}: {reason}")),
    }
    let observed = state
        .observe()
        .map_err(|reason| format!("{action:?}: {reason}"))?;
    if observed != expected {
        return Err(format!(
            "{action:?}: the runtime state {observed:?} is not the model state {expected:?}"
        ));
    }
    *model = expected;
    Ok(())
}

fn replay_overlap_witness() -> Result<(), String> {
    let scratch = trial_scratch("gc-overlap-witness-");
    let world = Rc::new(RefCell::new(OverlapWorld::start(OVERLAP_WITNESS_STORE, scratch.path())?));
    let mut model = OverlapModelState::INITIAL;
    let initial = world
        .borrow()
        .observe()
        .map_err(|reason| format!("Init: {reason}"))?;
    if initial != model {
        return Err(format!(
            "Init: the runtime state {initial:?} is not the model state {model:?}"
        ));
    }
    replay_overlap_step(&world, &mut model, PIN_OLD_KNOWN)?;
    replay_overlap_step(&world, &mut model, PIN_OLD_READER)?;
    let published = model
        .step(OverlapAction::Publish)
        .ok_or_else(|| format!("Publish: the model disables the step in {model:?}"))?;
    let expected_copy =
        [(model, Presence::Absent), (model, Presence::Present), (published, Presence::Present)];
    let copy = Rc::new(RefCell::new(Vec::with_capacity(OVERLAP_WITNESS_COPY_POINTS.len())));
    arm_overlap_copy_observation(Rc::clone(&world), Rc::clone(&copy), 0);
    let writer = world
        .borrow_mut()
        .take_old_writer()
        .map_err(|reason| format!("StartCopy: {reason}"))?;
    let outcome = compact(writer);
    world
        .borrow_mut()
        .adopt_successor(outcome)
        .map_err(|reason| format!("Publish: {reason}"))?;
    let observed_copy = std::mem::take(&mut *copy.borrow_mut());
    if observed_copy.len() != expected_copy.len() {
        return Err(format!(
            "Publish: the compaction passed {} of its {} observed points",
            observed_copy.len(),
            expected_copy.len()
        ));
    }
    for ((action, observation), expected) in observed_copy.into_iter().zip(expected_copy) {
        match observation {
            Ok(observed) if observed == expected => {}
            Ok(observed) => {
                return Err(format!(
                    "{action}: the runtime state and the successor files {observed:?} are not the \
                     model's {expected:?}"
                ));
            }
            Err(reason) => return Err(format!("{action}: {reason}")),
        }
    }
    model = published;
    let observed = world
        .borrow()
        .observe()
        .map_err(|reason| format!("Publish: {reason}"))?;
    if observed != model {
        return Err(format!(
            "Publish: after the compaction returned the runtime state {observed:?} is not the \
             model state {model:?}"
        ));
    }
    let carried = world.borrow_mut().successor_holds_closures();
    match carried {
        Ok(true) => {}
        Ok(false) => {
            return Err(
                "Publish: the successor does not hold the closures of the leased roots".to_owned()
            );
        }
        Err(reason) => return Err(format!("Publish: {reason}")),
    }
    for action in [OverlapAction::Crash, OverlapAction::Recover, OverlapAction::Reclaim] {
        replay_overlap_step(&world, &mut model, action)?;
    }
    replay_overlap_step(&world, &mut model, PIN_NEW_KNOWN)?;
    let closed = world.borrow_mut().successor_holds_closures();
    match (model.known_after_reclaim, closed) {
        (true, Ok(true)) => Ok(()),
        (false, _) => Err(format!(
            "{PIN_NEW_KNOWN:?}: the model's witness latch knownAfterReclaim is not set"
        )),
        (true, Ok(false)) => Err(format!(
            "{PIN_NEW_KNOWN:?}: the successor does not hold the closure of the new Known lease"
        )),
        (true, Err(reason)) => Err(format!("{PIN_NEW_KNOWN:?}: {reason}")),
    }
}

#[test]
fn the_overlap_witness_replays_through_crash_recovery_reclamation_and_a_new_known_lease() {
    if let Err(reason) = on_trial_thread("gc-overlap-witness-replay", replay_overlap_witness) {
        panic!("the runtime does not replay the model's overlap witness: {reason}");
    }
}

#[derive(Clone, Copy, Debug)]
enum RaceExpectation {
    RefusesCompaction,
    CarriedBySuccessor,
    SealedOut,
    PinsOldGeneration,
}

#[derive(Debug)]
struct RaceRecord {
    granted: bool,
    leases_after_registration: usize,
    published: bool,
    catalog: CatalogRole,
    refusal_names_the_lease: bool,
    successor_holds_closure: bool,
    old_generation: Presence,
    registry_open_afterwards: bool,
    published_after_release: bool,
}

const RACE_PLAN: [(PublicationPoint, KnownRootChoice, RaceExpectation); 9] = [
    (
        PublicationPoint::AfterPreflightBeforeCleanup,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::RefusesCompaction,
    ),
    (
        PublicationPoint::AfterCommitSync,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::RefusesCompaction,
    ),
    (
        PublicationPoint::BeforeCatalogCompareAndSwap,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::RefusesCompaction,
    ),
    (
        PublicationPoint::AfterPreflightBeforeCleanup,
        KnownRootChoice::AppointedRetention,
        RaceExpectation::CarriedBySuccessor,
    ),
    (
        PublicationPoint::BeforeCatalogCompareAndSwap,
        KnownRootChoice::CurrentState,
        RaceExpectation::CarriedBySuccessor,
    ),
    (
        PublicationPoint::AfterCatalogCasVerifiedBeforeSlotWrite,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::SealedOut,
    ),
    (
        PublicationPoint::AfterCatalogSlotWrite,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::SealedOut,
    ),
    (
        PublicationPoint::AfterCatalogSlotWrite,
        KnownRootChoice::CurrentState,
        RaceExpectation::SealedOut,
    ),
    (
        PublicationPoint::AfterCatalogFileSyncBeforeDirectorySync,
        KnownRootChoice::SupersededRetention,
        RaceExpectation::PinsOldGeneration,
    ),
];

fn precedes_catalog_slot_write(point: PublicationPoint) -> bool {
    let position = |wanted: PublicationPoint| {
        FORMAL_PUBLICATION_PHASE_ORDER
            .iter()
            .position(|candidate| *candidate == wanted)
    };
    match (position(point), position(PublicationPoint::AfterCatalogSlotWrite)) {
        (Some(registered), Some(written)) => registered < written,
        (None, _) | (_, None) => false,
    }
}

fn run_race(point: PublicationPoint, choice: KnownRootChoice) -> Result<RaceRecord, String> {
    let scratch = trial_scratch("gc-known-race-");
    let root = scratch.path();
    let KnownStore {
        writer,
        old,
        successor,
        old_files,
        known_root,
        reader_root,
        ..
    } = open_known_store(known_store(3, 0, RetentionFormat::V2, 1, choice), root)?;
    let generations = writer.generation_leases();
    let registration = Rc::new(RefCell::new(None));
    arm_generation_checkpoint_publication_hook(point, {
        let (generations, registration) = (generations.clone(), Rc::clone(&registration));
        move || {
            let granted = generations.register_known_root(known_root, old);
            let leases = generations.active_lease_count_for_test();
            *registration.borrow_mut() = Some((granted, leases));
        }
    });
    let outcome = compact(writer);
    let (granted, leases_after_registration) = registration
        .borrow_mut()
        .take()
        .ok_or_else(|| format!("the compaction did not reach {point:?}"))?;
    let lease = match granted {
        Ok(lease) => Some(lease),
        Err(KnownRootLeaseError::Registry(RetentionRegistryError::Sealed)) => None,
        Err(error) => return Err(format!("the racing registration failed: {error}")),
    };
    let catalog = selected_generation_role(root, old, successor);
    let (published, refusal_names_the_lease, successor_holds_closure) = match (&outcome, &lease) {
        (CompactionOutcome::Published(published), _) => {
            (true, false, holds_known_closure(published, known_root))
        }
        (CompactionOutcome::RootNotCarried(row), Some(lease)) => {
            (false, (row.root(), row.lease_id()) == (known_root, lease.lease_id()), false)
        }
        (CompactionOutcome::RootNotCarried(row), None) => {
            return Err(format!(
                "the compaction refused {row:?} although the registration was sealed out"
            ));
        }
        (CompactionOutcome::Refused(reason), _) => {
            return Err(format!("the compaction was refused for another reason: {reason}"));
        }
    };
    drop(outcome);
    let old_generation = generation_presence(&old_files);
    let registry_open_afterwards = generations
        .retained_roots()
        .and_then(|roots| roots.register(reader_root, RetainedRootKind::Reader))
        .is_ok();
    let granted = lease.is_some();
    let published_after_release = match (published, lease) {
        (false, Some(mut lease)) => {
            lease
                .release()
                .map_err(|error| format!("the racing lease was not released: {error}"))?;
            matches!(compact(admit_checkpoint(root)?), CompactionOutcome::Published(_))
        }
        (true, _) | (false, None) => false,
    };
    Ok(RaceRecord {
        granted,
        leases_after_registration,
        published,
        catalog,
        refusal_names_the_lease,
        successor_holds_closure,
        old_generation,
        registry_open_afterwards,
        published_after_release,
    })
}

fn run_interrupted_swap() -> Result<(CatalogRole, bool), String> {
    let scratch = trial_scratch("gc-known-race-");
    let root = scratch.path();
    let KnownStore {
        writer,
        old,
        successor,
        known_root,
        ..
    } = open_known_store(
        known_store(3, 0, RetentionFormat::V2, 0, KnownRootChoice::CurrentState),
        root,
    )?;
    let generations = writer.generation_leases();
    arm_generation_checkpoint_publication_failpoint(
        PublicationPoint::AfterCatalogCasVerifiedBeforeSlotWrite,
    );
    match compact(writer) {
        CompactionOutcome::Refused(_) => {}
        CompactionOutcome::Published(_) => {
            return Err("the failpoint did not interrupt the catalog swap".to_owned());
        }
        CompactionOutcome::RootNotCarried(row) => {
            return Err(format!("the compaction refused {row:?} without a registration"));
        }
    }
    Ok((
        selected_generation_role(root, old, successor),
        generations.register_known_root(known_root, old).is_ok(),
    ))
}

#[test]
fn a_registration_racing_a_compaction_is_carried_refuses_it_or_is_sealed_out() {
    let races = RACE_PLAN.map(|(point, choice, expectation)| {
        let outcome = on_trial_thread("gc-known-race-trial", move || run_race(point, choice));
        (point, choice, expectation, outcome)
    });
    for (point, record) in races
        .iter()
        .filter_map(|(point, _, _, outcome)| outcome.as_ref().ok().map(|record| (point, record)))
    {
        assert!(
            !(record.granted && precedes_catalog_slot_write(*point)) ||
                record.catalog != CatalogRole::New ||
                record.successor_holds_closure,
            "a compaction published without the root registered at {point:?}: {record:?}",
        );
    }
    for (point, choice, expectation, outcome) in &races {
        let record = match outcome {
            Ok(record) => record,
            Err(reason) => panic!("the race at {point:?} with {choice:?} halted: {reason}"),
        };
        let as_expected = match expectation {
            RaceExpectation::RefusesCompaction => {
                record.granted &&
                    !record.published &&
                    record.catalog == CatalogRole::Old &&
                    record.refusal_names_the_lease &&
                    record.leases_after_registration == 1 &&
                    record.registry_open_afterwards &&
                    record.published_after_release
            }
            RaceExpectation::CarriedBySuccessor => {
                record.granted &&
                    record.published &&
                    record.catalog == CatalogRole::New &&
                    record.successor_holds_closure &&
                    record.old_generation == Presence::Present &&
                    record.registry_open_afterwards
            }
            RaceExpectation::SealedOut => {
                !record.granted &&
                    record.leases_after_registration == 0 &&
                    record.published &&
                    record.catalog == CatalogRole::New &&
                    record.old_generation == Presence::Absent &&
                    record.registry_open_afterwards
            }
            RaceExpectation::PinsOldGeneration => {
                record.granted &&
                    record.published &&
                    record.catalog == CatalogRole::New &&
                    !record.successor_holds_closure &&
                    record.old_generation == Presence::Present
            }
        };
        assert!(
            as_expected,
            "the race at {point:?} with {choice:?} did not end as {expectation:?}: {record:?}",
        );
    }
    let interrupted = on_trial_thread("gc-known-race-interrupted-swap", run_interrupted_swap);
    assert_eq!(
        interrupted,
        Ok((CatalogRole::Old, true)),
        "an interrupted catalog swap left the root registry sealed",
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalRequest {
    NoRequest,
    RuntimeViews,
    View(DurableView),
    ExactPin,
    KnownLease,
}

const WAL_REQUESTS: [WalRequest; 9] = [
    WalRequest::NoRequest,
    WalRequest::RuntimeViews,
    WalRequest::View(DurableView::PublishedCore(ViewTarget::Current)),
    WalRequest::View(DurableView::PublishedCore(ViewTarget::Retained)),
    WalRequest::View(DurableView::RecoveredAuthority),
    WalRequest::View(DurableView::RecoveredAuthorityAt(ViewTarget::Current)),
    WalRequest::View(DurableView::RecoveredAuthorityAt(ViewTarget::Retained)),
    WalRequest::ExactPin,
    WalRequest::KnownLease,
];

const WAL_KNOWN_STORE: KnownStoreSpec =
    known_store(3, 1, RetentionFormat::V2, 1, KnownRootChoice::AppointedRetention);

#[derive(Debug, PartialEq, Eq)]
struct WalRecovery {
    checkpoints: u64,
    transactions: u64,
    reads_one_pack_to_its_tail: bool,
    ends_at_the_catalog_commit: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct WalFrontierRecord {
    old_tail_past_the_head: bool,
    old_tail_reads_one_pack: bool,
    old_tail_is_the_catalog_commit: bool,
    successor_window_past_the_tail: Option<(u64, u64)>,
    successor_follows_the_tail: Result<(), String>,
    old_generation_while_held: Presence,
    successor_recovery: Result<WalRecovery, String>,
    reclamation_after_release: Result<ProbeOutcome, String>,
    old_generation_after_release: Presence,
    successor_after_release: Presence,
}

fn recover_wal(root: &Path, slot: CatalogSlotV1) -> Result<WalRecovery, String> {
    let writer = DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
        .map_err(|error| format!("crash recovery was refused: {error:?}"))?;
    let recovered = writer.recovered();
    Ok(WalRecovery {
        checkpoints: recovered.checkpoints(),
        transactions: recovered.transactions(),
        reads_one_pack_to_its_tail: recovered.scanned_bytes() == slot.durable_pack_tail(),
        ends_at_the_catalog_commit: recovered.terminal_address() == slot.commit_address(),
    })
}

fn follows_the_recovery_tail(
    root: &Path,
    old: CatalogSlotV1,
    successor: CatalogSlotV1,
) -> Result<(), String> {
    use crate::rspace::persistence::{PACK_HEADER_LEN_V1, PackHeaderV1, PackTerminalIdentityV1};

    let pack = PackId::new(successor.pack().first_live_pack())
        .map_err(|error| format!("invalid first live pack: {error:?}"))?;
    let bytes = fs::read(root.join(pack_file_name(pack)))
        .map_err(|error| format!("cannot read the first live pack: {error}"))?;
    let header_len = usize::try_from(PACK_HEADER_LEN_V1)
        .map_err(|error| format!("the pack header length does not fit memory: {error}"))?;
    let header = bytes
        .get(..header_len)
        .ok_or_else(|| format!("the first live pack holds {} bytes", bytes.len()))?;
    let tail = old.commit_address();
    PackHeaderV1::parse_exact(header)
        .and_then(|header| {
            header.verify_predecessor(PackTerminalIdentityV1::new(
                tail.pack_id(),
                old.commit_sequence(),
                old.commit_digest(),
            ))
        })
        .map_err(|error| format!("the successor does not follow the recovery tail: {error:?}"))
}

fn observe_wal_frontier(
    root: &Path,
    release: impl FnOnce() -> Result<(), String>,
    publish: impl FnOnce(&Path) -> Result<CatalogSlotV1, String>,
) -> Result<WalFrontierRecord, String> {
    let old = selected_slot(root)?;
    let old_files = generation_files(root, GenerationLeaseKeyV1::from_catalog(old))?;
    let old_tail = recover_wal(root, old)?;
    let successor = publish(root)?;
    let successor_files = generation_files(root, GenerationLeaseKeyV1::from_catalog(successor))?;
    let old_generation_while_held = generation_presence(&old_files);
    let tail_pack = old.commit_address().segment();
    let successor_window_past_the_tail = successor
        .pack()
        .first_live_pack()
        .checked_sub(tail_pack)
        .zip(successor.pack().active_pack().checked_sub(tail_pack));
    let successor_follows_the_tail = follows_the_recovery_tail(root, old, successor);
    let successor_recovery = recover_wal(root, successor);
    release()?;
    let reclamation_after_release = request_reclamation(root);
    Ok(WalFrontierRecord {
        old_tail_past_the_head: old_tail.transactions > 0,
        old_tail_reads_one_pack: old_tail.reads_one_pack_to_its_tail,
        old_tail_is_the_catalog_commit: old_tail.ends_at_the_catalog_commit,
        successor_window_past_the_tail,
        successor_follows_the_tail,
        old_generation_while_held,
        successor_recovery,
        reclamation_after_release,
        old_generation_after_release: generation_presence(&old_files),
        successor_after_release: generation_presence(&successor_files),
    })
}

fn wal_data_store(
    root: &Path,
) -> Result<(GcWriter, DurableStateRootV1, DurableStateRootV1), String> {
    install_checkpoint_image(root, &checkpoint_image_from_source(checkpoint_source_fixture()));
    let first = admit_checkpoint(root)?;
    let retained = DurableStateRootV1::of_manifest(first.catalog().state().manifest());
    let writer = publish_data_successor(first, "gc-wal-frontier");
    let current = DurableStateRootV1::of_manifest(writer.catalog().state().manifest());
    Ok((writer, retained, current))
}

fn publish_test_store(root: &Path) -> Result<CatalogSlotV1, String> {
    publish_checkpoint(admit_checkpoint(root)?).map(|writer| writer.catalog())
}

fn publish_known_store(root: &Path) -> Result<CatalogSlotV1, String> {
    publish_checkpoint(admit_with_retention_writer(root, WAL_KNOWN_STORE.retention_writer)?)
        .map(|writer| writer.catalog())
}

fn publish_owner_store(root: &Path) -> Result<CatalogSlotV1, String> {
    DurableDirectoryV1::open(root)
        .map_err(|error| format!("the store directory does not open: {error:?}"))?
        .recover_and_admit_v1::<Profile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| format!("admission was refused: {error:?}"))?
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map(|writer| writer.catalog())
        .map_err(|error| format!("publication was refused: {error:?}"))
}

fn observe_runtime_views() -> Result<WalFrontierRecord, String> {
    let (scratch, hot, gate, mut owner) = owner_fixture();
    let pre_state = Blake2b256Hash::from(owner.current_root());
    populate(&hot);
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .map_err(|error| format!("the owner's checkpoint was refused: {error:?}"))?;
    let repository = DurableHistoryRepositoryV1::new(owner.published_core())
        .map_err(|error| format!("the durable repository does not open: {error:?}"))?;
    let mut views = Vec::with_capacity(RUNTIME_VIEW_REQUESTS.len());
    for request in RUNTIME_VIEW_REQUESTS {
        let target = match request {
            RuntimeRequest::Replay | RuntimeRequest::Rollback => pre_state.clone(),
            RuntimeRequest::Reset | RuntimeRequest::Spawn => repository.root(),
            RuntimeRequest::Clear => repository.empty_root(),
        };
        views.push(
            repository
                .reset(&target)
                .map_err(|error| format!("the {request:?} view was refused: {error:?}"))?,
        );
    }
    owner
        .close()
        .map_err(|error| format!("the owner does not close: {error:?}"))?;
    observe_wal_frontier(
        scratch.path(),
        move || {
            drop((owner, hot, gate, repository, views));
            Ok(())
        },
        publish_owner_store,
    )
}

fn run_wal_frontier_trial(request: WalRequest) -> Result<WalFrontierRecord, String> {
    match request {
        WalRequest::NoRequest => {
            let scratch = trial_scratch("gc-wal-frontier-");
            drop(wal_data_store(scratch.path())?);
            observe_wal_frontier(scratch.path(), || Ok(()), publish_test_store)
        }
        WalRequest::RuntimeViews => observe_runtime_views(),
        WalRequest::View(view) => {
            let scratch = trial_scratch("gc-wal-frontier-");
            let root = scratch.path();
            let (writer, retained, current) = wal_data_store(root)?;
            let (handle, writer) = hold_durable_view(view, root, writer, retained, current)?;
            drop(writer);
            observe_wal_frontier(
                root,
                move || {
                    drop(handle);
                    Ok(())
                },
                publish_test_store,
            )
        }
        WalRequest::ExactPin => {
            let scratch = trial_scratch("gc-wal-frontier-");
            let root = scratch.path();
            let (writer, _, _) = wal_data_store(root)?;
            let pin = writer
                .generation_leases()
                .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
                .map_err(|error| format!("the exact pin was refused: {error:?}"))?;
            drop(writer);
            observe_wal_frontier(
                root,
                move || {
                    drop(pin);
                    Ok(())
                },
                publish_test_store,
            )
        }
        WalRequest::KnownLease => {
            let scratch = trial_scratch("gc-wal-frontier-");
            let root = scratch.path();
            let store = open_known_store(WAL_KNOWN_STORE, root)?;
            let mut known = store
                .writer
                .register_known_root_v1(store.known_root)
                .map_err(|error| {
                    format!(
                        "the writer refused a Known lease on an object of its generation: {error}"
                    )
                })?;
            drop(store);
            observe_wal_frontier(
                root,
                move || {
                    known
                        .release()
                        .map_err(|error| format!("the Known lease was not released: {error}"))
                },
                publish_known_store,
            )
        }
    }
}

#[test]
fn the_wal_frontier_is_the_recovery_tail_and_no_request_holds_a_wal_start() {
    let observed = WAL_REQUESTS.map(|request| {
        (request, on_trial_thread("gc-wal-frontier", move || run_wal_frontier_trial(request)))
    });
    let expected = WAL_REQUESTS.map(|request| {
        let (while_held, after_release) = match request {
            WalRequest::NoRequest => (Presence::Absent, ProbeOutcome::NoIntent),
            WalRequest::RuntimeViews |
            WalRequest::View(_) |
            WalRequest::ExactPin |
            WalRequest::KnownLease => (Presence::Present, ProbeOutcome::Reclaimed),
        };
        let record = WalFrontierRecord {
            old_tail_past_the_head: true,
            old_tail_reads_one_pack: true,
            old_tail_is_the_catalog_commit: true,
            successor_window_past_the_tail: Some((1, 1)),
            successor_follows_the_tail: Ok(()),
            old_generation_while_held: while_held,
            successor_recovery: Ok(WalRecovery {
                checkpoints: 1,
                transactions: 0,
                reads_one_pack_to_its_tail: true,
                ends_at_the_catalog_commit: true,
            }),
            reclamation_after_release: Ok(after_release),
            old_generation_after_release: Presence::Absent,
            successor_after_release: Presence::Present,
        };
        (request, Ok::<WalFrontierRecord, String>(record))
    });
    assert_eq!(
        observed, expected,
        "GC-12, GC-13 and GC-15: no request other than crash recovery may hold a WAL start, so \
         under the runtime views, each durable view, an exact pin and a Known lease, as under no \
         request, the checkpoint must open the successor's one-pack WAL window directly after the \
         recovery tail of the old generation and follow that tail, crash recovery of the \
         successor must read that pack alone from its checkpoint to its durable tail, and the old \
         generation must stay only as objects while the request holds it and then be reclaimed \
         whole"
    );
}

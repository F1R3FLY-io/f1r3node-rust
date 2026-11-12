use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, TestCaseError, TestRng, TestRunner};

use super::checkpoint_wal::fixture_with_retained_union_cardinality;
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
};
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::support::{TestSemanticProfile, trial_scratch};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoError,
    GenerationCheckpointPublicationHookPointV1 as PublicationPoint, ReclamationCandidateV1,
    SemanticallyAdmittedDurableWriterV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, index_file_name, pack_file_name,
};
use crate::rspace::persistence::retention::{
    RetainedRootKind, RetainedRootRegistryV1, RetainedRootV1, RetentionRegistryError, RootLeaseV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogSelectionV1, CatalogSlotV1, GenerationLeaseError,
    GenerationLeaseKeyV1, GenerationLeaseRegistryV1, GenerationLeaseV1, IndexGenerationV1,
    LogPackDecodeLimits, ObjectRefV1, PackId, RetentionManifestV1, RetentionManifestView,
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

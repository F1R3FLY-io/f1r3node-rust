use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Occurrence {
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal {
    Pending,
    Finalized,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    None,
    OccurrenceCarrier,
    FinalizedStateFloor,
}

#[derive(Clone, Copy)]
struct Lifecycle {
    floor_committed: bool,
    finality_marked: bool,
    frozen_floor_covers: bool,
    effect_in_committed_lfb: bool,
    occurrence: Occurrence,
    terminal: Terminal,
    effect_done: bool,
    pool_contains: bool,
    writes: usize,
    occurrence_anchor: Anchor,
    state_anchor: Anchor,
}

fn apply_effect(state: &Mutex<Lifecycle>) {
    let mut state = state.lock().unwrap();
    if !state.floor_committed || !state.effect_in_committed_lfb || state.effect_done {
        return;
    }
    state.terminal = match state.occurrence {
        Occurrence::Succeeded => Terminal::Finalized,
        Occurrence::Failed => Terminal::Failed,
    };
    state.pool_contains = false;
    state.writes += 1;
    state.occurrence_anchor = Anchor::OccurrenceCarrier;
    state.state_anchor = Anchor::FinalizedStateFloor;
    state.effect_done = true;
}

fn check(occurrence: Occurrence, expected: Terminal) {
    loom::model(move || {
        let state = Arc::new(Mutex::new(Lifecycle {
            floor_committed: false,
            finality_marked: false,
            frozen_floor_covers: false,
            effect_in_committed_lfb: false,
            occurrence,
            terminal: Terminal::Pending,
            effect_done: false,
            pool_contains: true,
            writes: 0,
            occurrence_anchor: Anchor::None,
            state_anchor: Anchor::None,
        }));
        let commit = {
            let state = state.clone();
            thread::spawn(move || {
                let mut state = state.lock().unwrap();
                state.floor_committed = true;
                state.effect_in_committed_lfb = true;
            })
        };
        let effect = {
            let state = state.clone();
            thread::spawn(move || apply_effect(&state))
        };

        commit.join().unwrap();
        effect.join().unwrap();
        apply_effect(&state);
        apply_effect(&state);
        let state = state.lock().unwrap();
        assert_eq!(state.terminal, expected);
        assert!(state.effect_done);
        assert!(!state.pool_contains);
        assert_eq!(state.writes, 1);
        assert_eq!(state.occurrence_anchor, Anchor::OccurrenceCarrier);
        assert_eq!(state.state_anchor, Anchor::FinalizedStateFloor);
    });
}

#[test]
fn committed_success_terminalizes_without_later_admission() {
    check(Occurrence::Succeeded, Terminal::Finalized);
}

#[test]
fn committed_failure_terminalizes_without_later_admission() {
    check(Occurrence::Failed, Terminal::Failed);
}

fn check_non_state_evidence(marker: bool, frozen_floor: bool) {
    loom::model(move || {
        let state = Arc::new(Mutex::new(Lifecycle {
            floor_committed: false,
            finality_marked: false,
            frozen_floor_covers: false,
            effect_in_committed_lfb: false,
            occurrence: Occurrence::Succeeded,
            terminal: Terminal::Pending,
            effect_done: false,
            pool_contains: true,
            writes: 0,
            occurrence_anchor: Anchor::None,
            state_anchor: Anchor::None,
        }));
        let observe = {
            let state = state.clone();
            thread::spawn(move || {
                let mut state = state.lock().unwrap();
                state.finality_marked = marker;
                state.frozen_floor_covers = frozen_floor;
                state.floor_committed = true;
            })
        };
        let effect = {
            let state = state.clone();
            thread::spawn(move || apply_effect(&state))
        };

        observe.join().unwrap();
        effect.join().unwrap();
        apply_effect(&state);
        {
            let state = state.lock().unwrap();
            assert_eq!(state.terminal, Terminal::Pending);
            assert!(state.pool_contains);
            assert_eq!(state.writes, 0);
            assert_eq!(state.finality_marked, marker);
            assert_eq!(state.frozen_floor_covers, frozen_floor);
        }

        {
            let mut state = state.lock().unwrap();
            state.effect_in_committed_lfb = true;
        }
        apply_effect(&state);
        let state = state.lock().unwrap();
        assert_eq!(state.terminal, Terminal::Finalized);
        assert!(!state.pool_contains);
        assert_eq!(state.writes, 1);
    });
}

#[test]
fn finality_marker_does_not_settle_an_absent_effect() { check_non_state_evidence(true, false); }

#[test]
fn frozen_floor_coverage_does_not_settle_an_absent_effect() {
    check_non_state_evidence(false, true);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Surviving,
    Rejected,
    OffFloor,
}

#[derive(Clone, Copy)]
struct TerminalDisplay {
    terminalized: bool,
    semantic_carrier: Option<Source>,
    archive_representative: Source,
}

fn terminalize_display(display: &Mutex<TerminalDisplay>) {
    let mut display = display.lock().unwrap();
    if display.terminalized {
        return;
    }
    display.semantic_carrier = Some(Source::Surviving);
    display.terminalized = true;
}

#[test]
fn off_floor_archive_updates_cannot_change_terminal_carrier() {
    loom::model(|| {
        let display = Arc::new(Mutex::new(TerminalDisplay {
            terminalized: false,
            semantic_carrier: None,
            archive_representative: Source::Rejected,
        }));
        let terminalize = {
            let display = display.clone();
            thread::spawn(move || terminalize_display(&display))
        };
        let archive = {
            let display = display.clone();
            thread::spawn(move || {
                display.lock().unwrap().archive_representative = Source::OffFloor;
            })
        };

        terminalize.join().unwrap();
        archive.join().unwrap();
        terminalize_display(&display);
        let display = display.lock().unwrap();
        assert!(display.terminalized);
        assert_eq!(display.semantic_carrier, Some(Source::Surviving));
        assert_eq!(display.archive_representative, Source::OffFloor);
    });
}

#[derive(Clone, Copy)]
struct RestoredLifecycle {
    floor_ready: bool,
    open: bool,
    writes: usize,
}

fn evaluate_restored_lifecycle(state: &Mutex<RestoredLifecycle>) {
    let mut state = state.lock().unwrap();
    if !state.floor_ready || !state.open {
        return;
    }
    state.open = false;
    state.writes += 1;
}

#[test]
fn restored_lifecycle_waits_for_floor_materialization() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(RestoredLifecycle {
            floor_ready: false,
            open: true,
            writes: 0,
        }));
        let materialize = {
            let state = state.clone();
            thread::spawn(move || state.lock().unwrap().floor_ready = true)
        };
        let evaluate = {
            let state = state.clone();
            thread::spawn(move || evaluate_restored_lifecycle(&state))
        };

        materialize.join().unwrap();
        evaluate.join().unwrap();
        evaluate_restored_lifecycle(&state);
        let state = state.lock().unwrap();
        assert!(state.floor_ready);
        assert!(!state.open);
        assert_eq!(state.writes, 1);
    });
}

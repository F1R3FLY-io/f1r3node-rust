use std::collections::BTreeSet;

use loom::sync::{Arc, RwLock};
use loom::thread;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Summary {
    canonical: u8,
    revision: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    archive: BTreeSet<u8>,
    open: Option<Summary>,
    terminal: Option<Summary>,
    active: Option<u8>,
}

#[derive(Clone)]
struct InsertPlan {
    height: u8,
    expected_open: Option<Summary>,
    expected_terminal: Option<Summary>,
}

#[derive(Clone)]
struct CompactionPlan {
    expected_open: Option<Summary>,
    expected_terminal: Option<Summary>,
}

fn prepare_insert(state: &RwLock<State>, height: u8) -> InsertPlan {
    let state = state.read().unwrap();
    InsertPlan {
        height,
        expected_open: state.open.clone(),
        expected_terminal: state.terminal.clone(),
    }
}

fn commit_insert(state: &RwLock<State>, plan: InsertPlan) -> Result<(), ()> {
    let mut state = state.write().unwrap();
    if state.open != plan.expected_open || state.terminal != plan.expected_terminal {
        return Err(());
    }
    let inserted = state.archive.insert(plan.height);
    let revision_delta = u8::from(inserted);
    if state.terminal.is_none() {
        if let Some(open) = state.open.as_mut() {
            open.canonical = open.canonical.max(plan.height);
            open.revision += revision_delta;
            state.active = Some(open.canonical);
        } else {
            panic!("exactly one occurrence summary must exist");
        }
    } else if state.open.is_none() {
        if let Some(terminal) = state.terminal.as_mut() {
            terminal.canonical = terminal.canonical.max(plan.height);
            terminal.revision += revision_delta;
            state.active = None;
        } else {
            panic!("exactly one occurrence summary must exist");
        }
    } else {
        panic!("exactly one occurrence summary must exist");
    }
    Ok(())
}

fn insert(state: &RwLock<State>, height: u8) {
    loop {
        let plan = prepare_insert(state, height);
        if commit_insert(state, plan).is_ok() {
            return;
        }
        thread::yield_now();
    }
}

fn prepare_compaction(state: &RwLock<State>) -> CompactionPlan {
    let state = state.read().unwrap();
    CompactionPlan {
        expected_open: state.open.clone(),
        expected_terminal: state.terminal.clone(),
    }
}

fn commit_compaction(state: &RwLock<State>, plan: CompactionPlan) -> Result<(), ()> {
    let mut state = state.write().unwrap();
    if state.open != plan.expected_open || state.terminal != plan.expected_terminal {
        return Err(());
    }
    if state.terminal.is_some() {
        return Ok(());
    }
    state.terminal = state.open.take();
    state.active = None;
    Ok(())
}

fn compact(state: &RwLock<State>) {
    loop {
        let plan = prepare_compaction(state);
        if commit_compaction(state, plan).is_ok() {
            return;
        }
        thread::yield_now();
    }
}

fn validate(state: &State) {
    assert_eq!(
        usize::from(state.open.is_some()) + usize::from(state.terminal.is_some()),
        1
    );
    let summary = state.open.as_ref().or(state.terminal.as_ref()).unwrap();
    assert_eq!(
        Some(summary.canonical),
        state.archive.iter().next_back().copied()
    );
    assert_eq!(summary.revision as usize, state.archive.len());
    if state.terminal.is_some() {
        assert_eq!(state.active, None);
    } else {
        assert_eq!(state.active, Some(summary.canonical));
    }
}

fn initial_state() -> Arc<RwLock<State>> {
    Arc::new(RwLock::new(State {
        archive: BTreeSet::from([0]),
        open: Some(Summary {
            canonical: 0,
            revision: 1,
        }),
        terminal: None,
        active: Some(0),
    }))
}

#[test]
fn stale_insert_plans_retry_without_losing_occurrences() {
    loom::model(|| {
        let state = initial_state();
        let first = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 2))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.read().unwrap();
        assert_eq!(state.archive, BTreeSet::from([0, 1, 2]));
        assert_eq!(
            state.open.as_ref().map(|summary| summary.canonical),
            Some(2)
        );
        validate(&state);
    });
}

#[test]
fn insert_compaction_races_preserve_archive_and_terminal_representative() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(3);
    model.check(|| {
        let state = initial_state();
        let insert_one = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1))
        };
        let insert_two = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 2))
        };
        let compact_once = {
            let state = state.clone();
            thread::spawn(move || compact(&state))
        };
        insert_one.join().unwrap();
        insert_two.join().unwrap();
        compact_once.join().unwrap();

        let state = state.read().unwrap();
        assert_eq!(state.archive, BTreeSet::from([0, 1, 2]));
        assert_eq!(
            state.terminal.as_ref().map(|summary| summary.canonical),
            Some(2)
        );
        validate(&state);
    });
}

#[test]
fn readers_observe_only_atomic_pre_or_post_commit_states() {
    loom::model(|| {
        let state = initial_state();
        let insert_once = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1))
        };
        let compact_once = {
            let state = state.clone();
            thread::spawn(move || compact(&state))
        };
        let reader = {
            let state = state.clone();
            thread::spawn(move || validate(&state.read().unwrap()))
        };

        insert_once.join().unwrap();
        compact_once.join().unwrap();
        reader.join().unwrap();

        let state = state.read().unwrap();
        assert_eq!(state.archive, BTreeSet::from([0, 1]));
        assert_eq!(
            state.terminal.as_ref().map(|summary| summary.canonical),
            Some(1)
        );
        validate(&state);
    });
}

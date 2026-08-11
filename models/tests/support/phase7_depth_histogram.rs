//! Phase 7 production-entry depth measurement support.
//!
//! This file is compiled into `models` only with the explicitly test-only
//! `phase7-depth-histograms` feature. Normal builds contain none of its counter,
//! traversal, environment, synchronization, or file-I/O code.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};
use crate::rust::rholang::par_children::par_child_pars;

const OUTPUT_ENV: &str = "PHASE7_DEPTH_HISTOGRAM_PATH";
const SUITE_ENV: &str = "PHASE7_DEPTH_HISTOGRAM_SUITE";
const SUBJECT_ENV: &str = "PHASE7_DEPTH_HISTOGRAM_SUBJECT";

static OUTPUT_PATH: OnceLock<PathBuf> = OnceLock::new();
static OUTPUT_LOCK: Mutex<()> = Mutex::new(());
static SUBJECT: OnceLock<String> = OnceLock::new();

fn selected(subject: &'static str) -> bool {
    SUBJECT
        .get_or_init(|| {
            env::var(SUBJECT_ENV).unwrap_or_else(|_| {
                panic!(
                    "{SUBJECT_ENV} is required when phase7-depth-histograms is enabled; \
                     measuring every hook in every corpus would mix populations"
                )
            })
        })
        .as_str()
        == subject
}

fn output_path() -> &'static Path {
    OUTPUT_PATH
        .get_or_init(|| {
            env::var_os(OUTPUT_ENV).map(PathBuf::from).unwrap_or_else(|| {
                panic!(
                    "{OUTPUT_ENV} is required when phase7-depth-histograms is enabled; a silent sink would make the measurement vacuous"
                )
            })
        })
        .as_path()
}

fn output() -> impl Write {
    let path = output_path();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot append Phase 7 depth observations to {}: {error}",
                path.to_string_lossy()
            )
        })
}

/// Maximum structural `Par` depth, with each root `Par` at depth one.
///
/// The shared child table is the same generated/checked ownership relation used
/// by stack-safe teardown. In particular, `PathMap<()>` contributes no owned
/// `Par`, while `PathMap<Par>` contributes its associated values without
/// flattening either trie.
fn max_par_depth<'a>(roots: impl IntoIterator<Item = &'a Par>) -> usize {
    let mut maximum = 0usize;
    let mut work: Vec<(&Par, usize)> = roots.into_iter().map(|root| (root, 1)).collect();
    let mut children: Vec<&Par> = Vec::new();
    while let Some((par, depth)) = work.pop() {
        maximum = maximum.max(depth);
        children.clear();
        par_child_pars(par, &mut children);
        work.extend(children.iter().copied().map(|child| (child, depth + 1)));
    }
    maximum
}

fn record(subject: &'static str, root_kind: &'static str, depth: usize) {
    let suite = env::var(SUITE_ENV).unwrap_or_else(|_| {
        panic!(
            "{SUITE_ENV} is required when phase7-depth-histograms is enabled; every observation needs a corpus provenance label"
        )
    });
    let line = format!("{suite}\t{subject}\t{root_kind}\t{depth}\n");
    let _output_guard = OUTPUT_LOCK
        .lock()
        .expect("Phase 7 histogram output lock poisoned");
    output()
        .write_all(line.as_bytes())
        .expect("write Phase 7 depth observation");
}

pub(crate) fn record_par(subject: &'static str, value: &Par) {
    if !selected(subject) {
        return;
    }
    record(subject, "Par", max_par_depth([value]));
}

pub(crate) fn record_list_par_with_random(subject: &'static str, value: &ListParWithRandom) {
    if !selected(subject) {
        return;
    }
    record(
        subject,
        "ListParWithRandom",
        max_par_depth(value.pars.iter()),
    );
}

pub(crate) fn record_bind_pattern(subject: &'static str, value: &BindPattern) {
    if !selected(subject) {
        return;
    }
    record(subject, "BindPattern", max_par_depth(value.patterns.iter()));
}

pub(crate) fn record_par_with_random(subject: &'static str, value: &ParWithRandom) {
    if !selected(subject) {
        return;
    }
    record(subject, "ParWithRandom", max_par_depth(value.body.iter()));
}

pub(crate) fn record_tagged_continuation(subject: &'static str, value: &TaggedContinuation) {
    if !selected(subject) {
        return;
    }
    let body = value.tagged_cont.as_ref().and_then(|tagged| match tagged {
        TaggedCont::ParBody(value) => value.body.as_ref(),
        TaggedCont::ScalaBodyRef(_) => None,
    });
    record(
        subject,
        "TaggedContinuation",
        max_par_depth(value.guard.iter().chain(body)),
    );
}

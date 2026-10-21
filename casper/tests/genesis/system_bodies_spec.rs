//! DR-101: the genesis contract bodies that run a process held in a variable.
//!
//! A system body runs for the deployment that fires it, and the residue that
//! it stores keeps the system seal. A process held in a variable keeps its
//! sender's seal instead (P1 rem:signed-subst), and the reducer runs it outside
//! system mode. This check walks every genesis term in process position and
//! lists each variable that runs as a process, so a new site gets reviewed.

use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::interpreter_util::mk_term;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::normalizer_env::normalizer_env_from_deploy;

use crate::util::genesis_builder::GenesisBuilder;

/// Every variable that `term` runs as a process, with its position.
fn variables_run_as_processes(term: &Par) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![("term".to_string(), term)];
    while let Some((path, par)) = pending.pop() {
        for expr in &par.exprs {
            if let Some(ExprInstance::EVarBody(variable)) = &expr.expr_instance {
                found.push(format!("{path}: {variable:?}"));
            }
            // Added by G3 (DR-135, S3b): a method in process position runs
            // its result, so it is an exit of Rule H too (H1).
            if let Some(ExprInstance::EMethodBody(method)) = &expr.expr_instance {
                found.push(format!("{path}: method {}", method.method_name));
            }
        }
        // Added by G3 (DR-135, S3b): a bundle in process position runs its
        // body.
        for (index, bundle) in par.bundles.iter().enumerate() {
            if let Some(body) = &bundle.body {
                pending.push((format!("{path}/bundle[{index}]"), body));
            }
        }
        for (index, receive) in par.receives.iter().enumerate() {
            if let Some(body) = &receive.body {
                pending.push((format!("{path}/for[{index}]"), body));
            }
        }
        for (index, new) in par.news.iter().enumerate() {
            if let Some(body) = &new.p {
                pending.push((format!("{path}/new[{index}]"), body));
            }
        }
        for (index, matched) in par.matches.iter().enumerate() {
            for (case, branch) in matched.cases.iter().enumerate() {
                if let Some(body) = &branch.source {
                    pending.push((format!("{path}/match[{index}].case[{case}]"), body));
                }
            }
        }
        for (index, conditional) in par.conditionals.iter().enumerate() {
            for (branch, body) in [
                ("then", &conditional.if_true),
                ("else", &conditional.if_false),
            ] {
                if let Some(body) = body {
                    pending.push((format!("{path}/if[{index}].{branch}"), body));
                }
            }
        }
        for (index, signed) in par.cost_signed_terms.iter().enumerate() {
            if let Some(body) = &signed.body {
                pending.push((format!("{path}/signed[{index}]"), body));
            }
        }
    }
    found
}

#[test]
fn only_either_runs_a_process_held_in_a_variable() {
    let genesis = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4)).2;
    let terms = Genesis::default_blessed_terms(
        &genesis.proof_of_stake,
        &genesis.vaults,
        genesis.supply,
        &genesis.shard_id,
        &genesis.native_token_name,
        &genesis.native_token_symbol,
        genesis.native_token_decimals,
    );
    let either = standard_deploys::either(&genesis.shard_id).data.term;
    let mut either_sites = 0;
    let mut other_sites = Vec::new();
    for (index, deploy) in terms.iter().enumerate() {
        let term = mk_term(&deploy.data.term, normalizer_env_from_deploy(deploy))
            .expect("a genesis term normalizes");
        let sites = variables_run_as_processes(&term);
        if deploy.data.term == either {
            either_sites += sites.len();
        } else {
            other_sites.extend(
                sites
                    .into_iter()
                    .map(|site| format!("genesis deploy {index}: {site}")),
            );
        }
    }
    // `Either.map2` and `Either.map2Clean` run the caller's function `f`. The
    // reducer runs such a process outside system mode, so it keeps its
    // caller's seal (a_process_that_a_system_contract_runs_keeps_its_callers_seal).
    // A new site needs the same review.
    assert_eq!(either_sites, 2);
    assert!(other_sites.is_empty(), "{other_sites:#?}");
}

#[test]
fn the_check_finds_a_received_process_that_runs() {
    let term = mk_term(
        "new ch in { for(@p <- ch) { p } }",
        std::collections::HashMap::new(),
    )
    .expect("the control term normalizes");
    assert_eq!(variables_run_as_processes(&term).len(), 1);
}

/// Added by G3 (DR-135, S3b M3): the binders of one scope of the walk.
#[derive(Clone)]
enum Binders {
    /// The binders of a receive, index 0 first: the datum group of each, or
    /// none for a reply on a channel that the contract made itself.
    Receive(Vec<Option<usize>>),
    /// The names of a `new`.
    New(usize),
    /// The binders of a match case, with the datum groups of its target.
    Case(usize, std::collections::BTreeSet<usize>),
}

impl Binders {
    fn len(&self) -> usize {
        match self {
            Binders::Receive(groups) => groups.len(),
            Binders::New(count) | Binders::Case(count, _) => *count,
        }
    }
}

/// Added by G3 (DR-135, S3b M3): the scope that binds `BoundVar(k)` in
/// `scopes` (the innermost scope last), and the index of the binder in it.
fn binder_of(k: usize, scopes: &[Binders]) -> Option<(usize, usize)> {
    let mut k = k;
    for (index, scope) in scopes.iter().enumerate().rev() {
        if k < scope.len() {
            return Some((index, k));
        }
        k -= scope.len();
    }
    None
}

/// Added by G3 (DR-135, S3b M3): the datum groups of the free variables of
/// `par`.
fn free_groups(par: &Par, scopes: &[Binders]) -> std::collections::BTreeSet<usize> {
    let mut groups = std::collections::BTreeSet::new();
    for (k, _) in par
        .locally_free
        .iter()
        .enumerate()
        .filter(|(_, bit)| **bit != 0)
    {
        let Some((index, binder)) = binder_of(k, scopes) else {
            continue;
        };
        match &scopes[index] {
            Binders::Receive(owners) => groups.extend(owners[binder]),
            Binders::Case(_, target) => groups.extend(target.iter().copied()),
            Binders::New(_) => {}
        }
    }
    groups
}

/// Added by G3 (DR-135, S3b M3): whether a receive on `source` reads a reply
/// on a channel that the enclosing contract made itself: one name of a `new`
/// inside the body of the innermost enclosing receive. Such a datum comes
/// from the contract's own call, so it brings no value of another datum.
fn reads_own_reply(source: Option<&Par>, scopes: &[Binders]) -> bool {
    let Some(source) = source else {
        return false;
    };
    let [expr] = source.exprs.as_slice() else {
        return false;
    };
    let Some(ExprInstance::EVarBody(models::rhoapi::EVar {
        v:
            Some(models::rhoapi::Var {
                var_instance: Some(models::rhoapi::var::VarInstance::BoundVar(k)),
            }),
    })) = &expr.expr_instance
    else {
        return false;
    };
    let Some((index, _)) = usize::try_from(*k).ok().and_then(|k| binder_of(k, scopes)) else {
        return false;
    };
    let receive = scopes
        .iter()
        .rposition(|scope| matches!(scope, Binders::Receive(_)));
    matches!(scopes[index], Binders::New(_)) && receive.is_some_and(|receive| index > receive)
}

/// Added by G3 (DR-135, S3b M3): every send in `term` whose data use the
/// values of two or more data that the contract does not make itself: the
/// data of its own entry, of channels that callers pass in, and of shared
/// state. Such a send can mix code of several holders, so Rule H records no
/// single holder for it (H5, H6), and DR-135 documents the payer of each
/// site. A reply on a channel that the contract made itself comes from its
/// own call and is not counted.
fn sends_that_mix_data(term: &Par) -> Vec<String> {
    let mut found = Vec::new();
    let mut next_group = 0_usize;
    let mut pending: Vec<(String, &Par, Vec<Binders>)> =
        vec![("term".to_string(), term, Vec::new())];
    while let Some((path, par, scopes)) = pending.pop() {
        for (index, send) in par.sends.iter().enumerate() {
            let groups: std::collections::BTreeSet<usize> = send
                .data
                .iter()
                .flat_map(|datum| free_groups(datum, &scopes))
                .collect();
            if groups.len() >= 2 {
                found.push(format!("{path}/send[{index}]"));
            }
        }
        for (index, receive) in par.receives.iter().enumerate() {
            let mut owners: Vec<Option<usize>> = Vec::new();
            for bind in &receive.binds {
                let group = match reads_own_reply(bind.source.as_ref(), &scopes) {
                    true => None,
                    false => {
                        next_group += 1;
                        Some(next_group)
                    }
                };
                let count = usize::try_from(bind.free_count).unwrap_or_default();
                owners.extend(std::iter::repeat_n(group, count));
            }
            owners.reverse();
            if let Some(body) = &receive.body {
                let mut inner = scopes.clone();
                inner.push(Binders::Receive(owners));
                pending.push((format!("{path}/for[{index}]"), body, inner));
            }
        }
        for (index, new) in par.news.iter().enumerate() {
            if let Some(body) = &new.p {
                let mut inner = scopes.clone();
                inner.push(Binders::New(
                    usize::try_from(new.bind_count).unwrap_or_default(),
                ));
                pending.push((format!("{path}/new[{index}]"), body, inner));
            }
        }
        for (index, matched) in par.matches.iter().enumerate() {
            let target = matched
                .target
                .as_ref()
                .map(|target| free_groups(target, &scopes))
                .unwrap_or_default();
            for (case, branch) in matched.cases.iter().enumerate() {
                if let Some(body) = &branch.source {
                    let mut inner = scopes.clone();
                    inner.push(Binders::Case(
                        usize::try_from(branch.free_count).unwrap_or_default(),
                        target.clone(),
                    ));
                    pending.push((format!("{path}/match[{index}].case[{case}]"), body, inner));
                }
            }
        }
        for (index, conditional) in par.conditionals.iter().enumerate() {
            for (branch, body) in [
                ("then", &conditional.if_true),
                ("else", &conditional.if_false),
            ] {
                if let Some(body) = body {
                    pending.push((format!("{path}/if[{index}].{branch}"), body, scopes.clone()));
                }
            }
        }
        for (index, bundle) in par.bundles.iter().enumerate() {
            if let Some(body) = &bundle.body {
                pending.push((format!("{path}/bundle[{index}]"), body, scopes.clone()));
            }
        }
        for (index, signed) in par.cost_signed_terms.iter().enumerate() {
            if let Some(body) = &signed.body {
                pending.push((format!("{path}/signed[{index}]"), body, scopes.clone()));
            }
        }
    }
    found.sort();
    found
}

/// Added by G3 (DR-135, S3b M3): every genesis send whose data use the
/// values of several data, so a new site gets reviewed. Rule H gives such a
/// datum one holder only when all of its code has that holder (H5), and a
/// system body that fires on several code holders has no caller (H6).
#[test]
fn genesis_sends_that_mix_the_values_of_several_data_are_listed() {
    let genesis = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(4)).2;
    let terms = Genesis::default_blessed_terms(
        &genesis.proof_of_stake,
        &genesis.vaults,
        genesis.supply,
        &genesis.shard_id,
        &genesis.native_token_name,
        &genesis.native_token_symbol,
        genesis.native_token_decimals,
    );
    // The order of `Genesis::default_blessed_terms_with_policy`: eleven
    // standard deploys, the vault deploys, and the PoS deploy last.
    const NAMES: [&str; 11] = [
        "Registry",
        "VersionedRegistry",
        "ListOps",
        "Either",
        "NonNegativeNumber",
        "MakeMint",
        "AuthKey",
        "SystemVault",
        "MultiSigSystemVault",
        "Stack",
        "TokenMetadata",
    ];
    let mut sites = Vec::new();
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (index, deploy) in terms.iter().enumerate() {
        let name = match NAMES.get(index) {
            Some(name) => *name,
            None if index + 1 == terms.len() => "PoS",
            None => "vaults",
        };
        let term = mk_term(&deploy.data.term, normalizer_env_from_deploy(deploy))
            .expect("a genesis term normalizes");
        for site in sends_that_mix_data(&term) {
            *counts.entry(name).or_default() += 1;
            sites.push(format!("{name}: {site}"));
        }
    }
    let expected: std::collections::BTreeMap<&str, usize> = MIXED_DATA_SITES.into_iter().collect();
    assert_eq!(counts, expected, "{sites:#?}");
}

/// Added by G3 (DR-135, S3b M3): the control: a join that forwards both of
/// its values is listed, and a body that forwards one datum is not.
#[test]
fn the_check_finds_a_send_that_mixes_two_data() {
    let joined = mk_term(
        "new x, y, out in { for(@a <- x & @b <- y) { out!(a, b) } }",
        std::collections::HashMap::new(),
    )
    .expect("the join normalizes");
    assert_eq!(sends_that_mix_data(&joined).len(), 1);
    let single = mk_term(
        "new x, out in { for(@a, @b <- x) { out!(a, b) } }",
        std::collections::HashMap::new(),
    )
    .expect("the receive normalizes");
    assert!(sends_that_mix_data(&single).is_empty());
    let own_reply = mk_term(
        "new c in { for(@x, ret <- c) { new r in { ret!(x) | for(@y <- r) { ret!(x, y) } } } }",
        std::collections::HashMap::new(),
    )
    .expect("the contract normalizes");
    assert!(
        sends_that_mix_data(&own_reply).is_empty(),
        "a reply on a channel that the contract made is its own datum"
    );
}

/// Added by G3 (DR-135, S3b M3): the reviewed number of sites of each
/// genesis contract in
/// `genesis_sends_that_mix_the_values_of_several_data_are_listed`. Most
/// sites combine a value that a caller-provided or shared channel brings
/// with the arguments of the contract's own entry: the "X <-" forms of
/// Either (among them "map2 <-", "map2Clean <-" and "productR <-"), the
/// node updates of the registry (among them the leaf merge), and the vault,
/// mint, stack and PoS state updates. A change in genesis that adds or
/// removes such a site changes these numbers, and the site then gets the
/// review that DR-135 records.
const MIXED_DATA_SITES: [(&str, usize); 8] = [
    ("Either", 13),
    ("MakeMint", 3),
    ("MultiSigSystemVault", 7),
    ("PoS", 6),
    ("Registry", 20),
    ("Stack", 1),
    ("SystemVault", 12),
    ("VersionedRegistry", 7),
];

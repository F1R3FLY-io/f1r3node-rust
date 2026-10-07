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

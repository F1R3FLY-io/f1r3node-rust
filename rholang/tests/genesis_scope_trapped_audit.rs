// Audit: for each top-level `new` in the embedded genesis Rholang
// contracts, run `is_scope_trapped` on every rand-allocated binder and
// print the verdict. Used to hunt the pattern H1 misclassifies in the
// casper multi-parent-block regression, per docs/rholang/
// garbage-collection-formalization-plan.md flip-the-default gate item 4.
//
// This is a diagnostic test — it does not assert anything; it just
// prints a structured report. Run with:
//   cargo test -p rholang --test genesis_scope_trapped_audit -- --nocapture

use std::fs;

use models::rhoapi::Par;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::garbage::is_scope_trapped;

const GENESIS_RHO_DIR: &str = "../casper/src/main/resources";

const GENESIS_FILES: &[&str] = &[
    "Registry.rho",
    "NonNegativeNumber.rho",
    "MakeMint.rho",
    "AuthKey.rho",
    "SystemVault.rho",
    "MultiSigSystemVault.rho",
    "Either.rho",
    "ListOps.rho",
];

#[derive(Debug, Default, Clone)]
struct NewStats {
    path: String,
    rand_count: usize,
    trapped: Vec<usize>, // de Bruijn indices in body
}

fn walk_news(par: &Par, path: &str, out: &mut Vec<NewStats>) {
    for (idx, n) in par.news.iter().enumerate() {
        let bind_count = n.bind_count as usize;
        let uri_count = n.uri.len();
        let rand_count = bind_count - uri_count;
        let body = n.p.as_ref().expect("new body missing");
        let path_here = format!("{}/news[{}]:bc={}:uri={}", path, idx, bind_count, uri_count);

        let mut stats = NewStats {
            path: path_here.clone(),
            rand_count,
            trapped: Vec::new(),
        };
        // Match eval_new: rand slot i -> body de Bruijn index bind_count-1-i,
        // for i in 0..rand_count. URN-injected binders occupy the low
        // indices [0, uri_count-1] and are excluded.
        for i in 0..rand_count {
            let body_db_index = bind_count - 1 - i;
            if is_scope_trapped(body_db_index, body) {
                stats.trapped.push(body_db_index);
            }
        }
        out.push(stats);

        // Recurse into every sub-field that can carry a nested `new`.
        walk_news(body, &path_here, out);
    }

    for (i, s) in par.sends.iter().enumerate() {
        if let Some(c) = &s.chan {
            walk_news(c, &format!("{}/sends[{}].chan", path, i), out);
        }
        for (j, d) in s.data.iter().enumerate() {
            walk_news(d, &format!("{}/sends[{}].data[{}]", path, i, j), out);
        }
    }
    for (i, r) in par.receives.iter().enumerate() {
        for (j, b) in r.binds.iter().enumerate() {
            if let Some(src) = &b.source {
                walk_news(
                    src,
                    &format!("{}/receives[{}].binds[{}].source", path, i, j),
                    out,
                );
            }
            for (k, p) in b.patterns.iter().enumerate() {
                walk_news(
                    p,
                    &format!("{}/receives[{}].binds[{}].patterns[{}]", path, i, j, k),
                    out,
                );
            }
        }
        if let Some(body) = &r.body {
            walk_news(body, &format!("{}/receives[{}].body", path, i), out);
        }
    }
    for (i, m) in par.matches.iter().enumerate() {
        if let Some(t) = &m.target {
            walk_news(t, &format!("{}/matches[{}].target", path, i), out);
        }
        for (j, c) in m.cases.iter().enumerate() {
            if let Some(src) = &c.source {
                walk_news(
                    src,
                    &format!("{}/matches[{}].cases[{}].source", path, i, j),
                    out,
                );
            }
            if let Some(pat) = &c.pattern {
                walk_news(
                    pat,
                    &format!("{}/matches[{}].cases[{}].pattern", path, i, j),
                    out,
                );
            }
        }
    }
    for (i, b) in par.bundles.iter().enumerate() {
        if let Some(body) = &b.body {
            walk_news(body, &format!("{}/bundles[{}].body", path, i), out);
        }
    }
    for (i, cond) in par.conditionals.iter().enumerate() {
        if let Some(c) = &cond.condition {
            walk_news(c, &format!("{}/conditionals[{}].condition", path, i), out);
        }
        if let Some(t) = &cond.if_true {
            walk_news(t, &format!("{}/conditionals[{}].if_true", path, i), out);
        }
        if let Some(f) = &cond.if_false {
            walk_news(f, &format!("{}/conditionals[{}].if_false", path, i), out);
        }
    }
}

#[test]
fn audit_genesis_scope_trapped_classifier() {
    for &fname in GENESIS_FILES {
        let path = format!("{}/{}", GENESIS_RHO_DIR, fname);
        let rho = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("SKIP {}: missing", path);
                continue;
            }
        };
        let par = match Compiler::source_to_adt(&rho) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("SKIP {}: compile failed: {:?}", path, e);
                continue;
            }
        };

        let mut stats = Vec::new();
        walk_news(&par, &format!("{}:root", fname), &mut stats);

        let total_rand: usize = stats.iter().map(|s| s.rand_count).sum();
        let total_trapped: usize = stats.iter().map(|s| s.trapped.len()).sum();

        println!("\n=== {} ===", fname);
        println!(
            "  news: {} | rand binders: {} | trapped by H1: {}",
            stats.len(),
            total_rand,
            total_trapped
        );
        for s in &stats {
            if !s.trapped.is_empty() {
                println!(
                    "  TRAPPED @ {} — body de Bruijn indices {:?} of {} rand slots",
                    s.path, s.trapped, s.rand_count
                );
            }
        }
    }
}

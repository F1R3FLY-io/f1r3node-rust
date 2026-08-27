//! Corpus tests for the Plausible Fiction object language.
//!
//! Companion to `corpus.rs`. `PFLam.module` is the `Theory PFLam()` of
//! `publications/plausible-fiction/plausible-fiction.tex` (revision of
//! 2026-08-26); `PricedPFLam.module` is section 5 of `priced-pf.tex`.
//!
//! These are the acceptance tests for the prototype's object language. If
//! they pass, Embers can elaborate a PF theory client-side and the papers'
//! listings are executable rather than illustrative.

use mettail_elab::diag::DiagKind;
use mettail_elab::resolve::{FileResolver, Resolver};
use mettail_elab::{elaborate, Presentation};
use std::path::PathBuf;

fn modules() -> FileResolver {
    FileResolver {
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/modules"),
    }
}

fn bad() -> FileResolver {
    FileResolver {
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/modules/bad"),
    }
}

fn ok(file: &str, r: &dyn Resolver) -> Presentation {
    match elaborate(file, r) {
        Ok(p) => p,
        Err(d) => panic!("{file}: expected success, got {d}"),
    }
}

// ------------------------------------------------------------------ positive

#[test]
fn pflam_join_is_a_pushout_over_core() {
    let p = ok("PFLam.module", &modules());

    // Records, Variants and Holes all extend the SAME Core instance, so the
    // join identifies the carrier rather than duplicating it. Seven
    // categories: Term and Lvl from Core, Tag/VariantAlt/CaseBranch from
    // Variants, HoleId/MetaId from Holes.
    assert_eq!(p.types.len(), 7, "one Term carrier, not three");
    for c in [
        "Term",
        "Lvl",
        "Tag",
        "VariantAlt",
        "CaseBranch",
        "HoleId",
        "MetaId",
    ] {
        assert!(p.has_cat(c), "missing category `{c}`");
    }

    // Exactly one application former survived the three-way join.
    assert_eq!(p.terms.len(), 16, "4 core + 4 record + 5 variant + 3 hole");
    for l in [
        "Univ", "Pi", "Lam", "App", "Sig", "Pair", "Fst", "Snd", "Variant", "Alt", "Inj", "Case",
        "CaseBr", "Hole", "Obs", "Meta",
    ] {
        assert!(p.has_label(l), "missing `{l}`");
    }
}

#[test]
fn pflam_has_no_native_escapes() {
    // grounded, holes and plug are host operations at the bridge, not term
    // formers. If any of them reappears as a constructor, the theory has
    // stopped being escape-free and the paper's section 4.3 is wrong.
    let p = ok("PFLam.module", &modules());
    for l in ["Grounded", "HolesOf", "Plug", "SelectArm", "Var"] {
        assert!(!p.has_label(l), "`{l}` should not be a PFLam constructor");
    }
}

#[test]
fn pflam_carries_the_hard_forms() {
    let p = ok("PFLam.module", &modules());
    let r = p.render();

    // G2: the alternatives of a variant and the arms of a case are
    // collections, rendered with a separator projection.
    assert!(r.contains("alts:HashBag(VariantAlt)"));
    assert!(r.contains(r#"alts.*sep("|")"#));
    assert!(r.contains("brs:HashBag(CaseBranch)"));

    // G4: every binder is an abstraction, and substitution is two-argument.
    assert!(r.contains("^x.body:[Term -> Term]"));
    assert!(r.contains("(subst ^x.b c)"));

    // G3: variant beta selects the matching arm by remainder pattern with a
    // repeated label variable -- structurally rho's COMM rule.
    assert!(r.contains("...rest"));
    assert!(r.contains("(CaseBr l ^x.body)"));

    // The eta side condition, which the withdrawn `language!` block could
    // only write as a comment.
    assert!(r.contains("if x # f then"));

    // G6: the motive is spelled, not merely passed.
    assert!(r.contains(r#""return""#));
}

#[test]
fn priced_pflam_extends_across_a_module_boundary() {
    let p = ok("PricedPFLam.module", &modules());

    // Everything PFLam had, plus Price and Obsv. The parameter carries the
    // base theory in; nothing is restated.
    assert_eq!(p.types.len(), 9);
    assert!(p.has_cat("Price"));
    assert!(p.has_cat("Obsv"));
    assert!(p.has_cat("HoleId"), "inherited through the parameter");

    for l in ["Priced", "Scale", "Give", "ObsOne"] {
        assert!(p.has_label(l), "missing `{l}`");
    }
    // The base constructors came along.
    for l in ["Hole", "Obs", "Case", "Lam"] {
        assert!(p.has_label(l), "`{l}` should arrive with the parameter");
    }

    // val is a host operation over the term, not a constructor: it returns a
    // Price, not a Term.
    assert!(!p.has_label("Val"));
}

#[test]
fn priced_import_graph_is_recorded() {
    let prog =
        mettail_elab::resolve::Program::load("PricedPFLam.module", &modules()).expect("loads");
    let lock = prog.lockfile();
    assert_eq!(lock.len(), 2, "entry plus PFLam.module");
    assert!(lock.iter().any(|(u, _)| u.ends_with("PFLam.module")));
}

// ------------------------------------------------------------------ negative

#[test]
fn rejects_case_motive_not_referenced() {
    // The dependent eliminator as the withdrawn block wrote it: the motive is
    // passed but never spelled. Invisible under the positional BNFC form; G6
    // makes it a diagnostic.
    match elaborate("CaseMotiveUnused.module", &bad()) {
        Ok(_) => panic!("expected ArgumentUse, but it elaborated"),
        Err(d) => assert_eq!(d.kind, DiagKind::ArgumentUse, "got {}", d.msg),
    }
}

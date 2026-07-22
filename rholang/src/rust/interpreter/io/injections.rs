//! `NormalizerEnv` bundles for the File I/O native primitives.
//!
//! The `rho:io:fs:native:1.0.0/*` URNs are filtered out of the
//! runtime's shared `urn_map` (see
//! `rho_runtime::is_internal_urn`), so user code can't bind them
//! via `new x(URN)`. The genesis-time FS-agent deploy still
//! needs a way to obtain the fixed-channel `Par` for each
//! primitive so its Rholang can dispatch to the handlers. The
//! per-deploy `NormalizerEnv` (a `HashMap<String, Par>` accepted
//! by `Compiler::source_to_adt_with_normalizer_env` /
//! `RhoRuntime::evaluate_with_env_and_phlo`) is that channel.
//!
//! During compilation the env is copied into every `new` node's
//! `injections` `BTreeMap`. At `eval_new` time (`reduce.rs`),
//! URNs that miss `urn_map` fall through to `new.injections` --
//! meaning the fileio native URNs, which are absent from the
//! former, are resolved from the latter. See
//! `implementation-plan.md` §"Bootstrap path for the FS agent"
//! for the full design.
//!
//! Injection values are the bare `GPrivate` `Par`s
//! `FixedChannels::native_*()` returns. They are NOT
//! bundle-wrapped: `eval_new`'s injection path
//! (`reduce.rs:1297-1325`) accepts only `GUnforgeable` or
//! `Expression` Pars and errors on `Bundle`. Bare unforgeable is
//! the right authority level for the FS-agent, which owns the
//! channel and never leaks the name; user code has no path to
//! obtain the private name because the URN isn't in `urn_map`.
//!
//! # Visibility discipline
//!
//! `fileio_native_urns()` is `pub(crate)` on purpose: callers
//! outside `rholang` must not obtain the raw bundle, since
//! merging it into any user-reachable env would grant
//! unauthenticated arbitrary-host-FS authority. The one
//! legitimate consumer (the FS-agent's genesis deploy) goes
//! through [`compile_fileio_genesis_source`], which merges the
//! bundle in-crate and returns only the compiled `Par`. A
//! downstream crate accidentally reaching for
//! `injections::fileio_native_urns()` will then be a compile
//! error, not a code-review invariant.

use std::collections::HashMap;

use models::rhoapi::Par;

use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::system_processes::FixedChannels;

/// Return the `NormalizerEnv` bundle for the File I/O native
/// primitives: the twenty-two `rho:io:fs:native:1.0.0/*` URNs
/// registered in `std_system_processes()`, each mapped to its
/// fixed-channel `Par` (a `GUnforgeable::GPrivate` under the
/// hood).
///
/// Kept `pub(crate)` -- external callers must go through
/// [`compile_fileio_genesis_source`] so the raw bundle never
/// crosses the crate boundary. See the module docstring's
/// "Visibility discipline" section.
pub(crate) fn fileio_native_urns() -> HashMap<String, Par> {
    let mut m = HashMap::with_capacity(22);
    m.insert(
        "rho:io:fs:native:1.0.0/open".to_string(),
        FixedChannels::native_open(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/close".to_string(),
        FixedChannels::native_close(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/read".to_string(),
        FixedChannels::native_read(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/write".to_string(),
        FixedChannels::native_write(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/seek".to_string(),
        FixedChannels::native_seek(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/tell".to_string(),
        FixedChannels::native_tell(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/size".to_string(),
        FixedChannels::native_size(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/truncate".to_string(),
        FixedChannels::native_truncate(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/flush".to_string(),
        FixedChannels::native_flush(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/stat".to_string(),
        FixedChannels::native_stat(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/entries".to_string(),
        FixedChannels::native_entries(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/exists".to_string(),
        FixedChannels::native_exists(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/rename".to_string(),
        FixedChannels::native_rename(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/copyFile".to_string(),
        FixedChannels::native_copy_file(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/removeFile".to_string(),
        FixedChannels::native_remove_file(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/removeDir".to_string(),
        FixedChannels::native_remove_dir(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/chmod".to_string(),
        FixedChannels::native_chmod(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/quarantine".to_string(),
        FixedChannels::native_quarantine(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/chown".to_string(),
        FixedChannels::native_chown(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/readLine".to_string(),
        FixedChannels::native_read_line(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/readAllLines".to_string(),
        FixedChannels::native_read_all_lines(),
    );
    m.insert(
        "rho:io:fs:native:1.0.0/appendLines".to_string(),
        FixedChannels::native_append_lines(),
    );
    m
}

/// Compile the FS-agent's genesis Rholang source with the
/// fileio-native URN bundle spliced into the normalizer env.
/// The raw bundle is never returned; callers only see the
/// compiled `Par`, which carries the fixed-channel `Par`s inside
/// each `New` node's `injections` map where `eval_new` can
/// resolve them but nothing else can name them.
///
/// `extra_env` lets the caller add its own bindings (e.g. the
/// per-deploy `deployId`/`deployerId` pair from
/// `normalizer_env_from_deploy`). Collisions with the fileio
/// namespace are rejected -- the fileio URNs are a fixed
/// vocabulary, so a caller attempting to shadow one is a
/// programming error, not a legitimate override.
pub fn compile_fileio_genesis_source(
    source: &str,
    extra_env: HashMap<String, Par>,
) -> Result<Par, InterpreterError> {
    let fileio_env = fileio_native_urns();
    for k in extra_env.keys() {
        if fileio_env.contains_key(k) {
            return Err(InterpreterError::BugFoundError(format!(
                "compile_fileio_genesis_source: extra_env shadows a fileio native URN ({})",
                k
            )));
        }
    }
    let mut env = fileio_env;
    env.extend(extra_env);
    Compiler::source_to_adt_with_normalizer_env(source, env)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;

    use super::*;
    use crate::rust::interpreter::compiler::compiler::Compiler;

    /// The bundle keys are the URN strings we register in
    /// `std_system_processes()`. Any drift (e.g. renaming an
    /// endpoint) needs a matching update here or the bundle
    /// misses the URN and eval_new errors with "No value set for
    /// {urn}" at runtime.
    #[test]
    fn bundle_covers_every_fileio_native_urn() {
        let bundle = fileio_native_urns();
        let expected: &[&str] = &[
            "rho:io:fs:native:1.0.0/open",
            "rho:io:fs:native:1.0.0/close",
            "rho:io:fs:native:1.0.0/read",
            "rho:io:fs:native:1.0.0/write",
            "rho:io:fs:native:1.0.0/seek",
            "rho:io:fs:native:1.0.0/tell",
            "rho:io:fs:native:1.0.0/size",
            "rho:io:fs:native:1.0.0/truncate",
            "rho:io:fs:native:1.0.0/flush",
            "rho:io:fs:native:1.0.0/stat",
            "rho:io:fs:native:1.0.0/entries",
            "rho:io:fs:native:1.0.0/exists",
            "rho:io:fs:native:1.0.0/rename",
            "rho:io:fs:native:1.0.0/copyFile",
            "rho:io:fs:native:1.0.0/removeFile",
            "rho:io:fs:native:1.0.0/removeDir",
            "rho:io:fs:native:1.0.0/chmod",
            "rho:io:fs:native:1.0.0/quarantine",
            "rho:io:fs:native:1.0.0/chown",
            "rho:io:fs:native:1.0.0/readLine",
            "rho:io:fs:native:1.0.0/readAllLines",
            "rho:io:fs:native:1.0.0/appendLines",
        ];
        for urn in expected {
            assert!(bundle.contains_key(*urn), "bundle missing {urn}");
        }
        assert_eq!(
            bundle.len(),
            expected.len(),
            "bundle has {} entries, expected {}",
            bundle.len(),
            expected.len()
        );
    }

    /// Each value is a Par carrying a single `GPrivate`
    /// unforgeable, i.e. what `byte_name(N)` produces. That
    /// shape is what `eval_new`'s injection path
    /// (`reduce.rs:1297-1325`) recognizes via
    /// `RhoUnforgeable::unapply`; any other shape (Bundle, list,
    /// literal) would land in `Err("invalid injection")` at
    /// runtime.
    #[test]
    fn every_value_is_a_bare_gprivate_unforgeable() {
        let bundle = fileio_native_urns();
        for (urn, par) in &bundle {
            assert_eq!(
                par.exprs.len(),
                0,
                "{urn}: injection Par should have no exprs"
            );
            assert_eq!(
                par.unforgeables.len(),
                1,
                "{urn}: injection Par should have exactly one unforgeable"
            );
            let unf = &par.unforgeables[0];
            assert!(
                matches!(unf.unf_instance, Some(UnfInstance::GPrivateBody(_))),
                "{urn}: injection unforgeable should be GPrivate, got {:?}",
                unf.unf_instance
            );
        }
    }

    /// End-to-end plumbing check: compile a fragment that binds
    /// one fileio URN with the bundle in the normalizer env, and
    /// assert the resulting `New` node's `injections` BTreeMap
    /// carries the URN mapped to the same fixed-channel Par we
    /// injected. This is the exact hop the FS-agent's genesis
    /// deploy will rely on at boot.
    #[test]
    fn injections_land_in_the_new_node_after_compilation() {
        use models::rhoapi::New;

        let bundle = fileio_native_urns();
        let expected_par = bundle
            .get("rho:io:fs:native:1.0.0/open")
            .cloned()
            .expect("bundle has open");

        let src = "new nOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }";
        let par = Compiler::source_to_adt_with_normalizer_env(src, bundle)
            .expect("compile with fileio injections");

        // The compiled Par should have one `New` (or contain it
        // nested; for this shape it's at the top level).
        assert_eq!(
            par.news.len(),
            1,
            "expected exactly one New at the top level"
        );
        let new_node: &New = &par.news[0];
        let injected = new_node
            .injections
            .get("rho:io:fs:native:1.0.0/open")
            .expect("injections should carry the fileio URN");
        assert_eq!(
            injected, &expected_par,
            "injected Par should match the bundle's fixed-channel Par"
        );
    }

    /// Negative twin of the above: compile the same fragment
    /// with an empty env; the URN still ends up in the New's
    /// `uri` list (compilation doesn't validate URN existence),
    /// but the `injections` map has no entry for it. At runtime
    /// this would produce `"No value set for {urn}"` from
    /// `eval_new` -- documented here so a future caller who
    /// forgets to pass the bundle understands the failure mode.
    #[test]
    fn empty_env_omits_the_url_from_injections() {
        let src = "new nOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }";
        let par = Compiler::source_to_adt_with_normalizer_env(src, HashMap::new())
            .expect("compile with empty env");
        assert_eq!(par.news.len(), 1);
        let new_node = &par.news[0];
        assert!(new_node
            .uri
            .contains(&"rho:io:fs:native:1.0.0/open".to_string()));
        assert!(
            !new_node
                .injections
                .contains_key("rho:io:fs:native:1.0.0/open"),
            "empty env should not populate injections"
        );
        // Silence unused-import warning under this test-arm-only path.
        let _ = ExprInstance::GBool(true);
    }

    /// Wrapper happy-path: `compile_fileio_genesis_source` with
    /// no extra env produces a Par whose top-level `New` carries
    /// the fileio URN in `injections`.
    #[test]
    fn wrapper_compiles_fileio_source_and_injects_bundle() {
        let src = "new nOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }";
        let par = compile_fileio_genesis_source(src, HashMap::new())
            .expect("wrapper should compile with default fileio env");
        assert_eq!(par.news.len(), 1);
        let expected = FixedChannels::native_open();
        let injected = par.news[0]
            .injections
            .get("rho:io:fs:native:1.0.0/open")
            .expect("wrapper must inject fileio URN");
        assert_eq!(injected, &expected);
    }

    /// Wrapper rejects any `extra_env` key that shadows a fileio
    /// URN. Prevents a caller from swapping the FS-agent's channel
    /// out from under itself (either accidentally or maliciously).
    #[test]
    fn wrapper_rejects_extra_env_shadowing_a_fileio_urn() {
        let src = "new nOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }";
        let mut extra = HashMap::new();
        extra.insert("rho:io:fs:native:1.0.0/open".to_string(), Par::default());
        let err =
            compile_fileio_genesis_source(src, extra).expect_err("collision should be rejected");
        assert!(
            format!("{:?}", err).contains("shadows a fileio native URN"),
            "unexpected error: {:?}",
            err
        );
    }

    /// Wrapper merges caller-supplied env alongside the fileio
    /// bundle: a non-colliding extra binding lands in the compiled
    /// New's injections just like the fileio URNs.
    #[test]
    fn wrapper_preserves_non_colliding_extra_env() {
        use models::rhoapi::g_unforgeable::UnfInstance;
        use models::rhoapi::{GPrivate, GUnforgeable};

        let extra_par = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![0xEE; 64] })),
        }]);
        let mut extra = HashMap::new();
        extra.insert("rho:test:injection:custom".to_string(), extra_par.clone());

        let src = r#"new nOpen(`rho:io:fs:native:1.0.0/open`),
                          custom(`rho:test:injection:custom`) in { Nil }"#;
        let par = compile_fileio_genesis_source(src, extra)
            .expect("wrapper should compile with mixed env");
        assert_eq!(par.news.len(), 1);
        let injections = &par.news[0].injections;
        assert!(injections.contains_key("rho:io:fs:native:1.0.0/open"));
        assert_eq!(
            injections.get("rho:test:injection:custom"),
            Some(&extra_par)
        );
    }
}

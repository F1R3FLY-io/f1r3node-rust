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

use std::collections::HashMap;

use models::rhoapi::Par;

use crate::rust::interpreter::system_processes::FixedChannels;

/// Return the `NormalizerEnv` bundle for the File I/O native
/// primitives: the seventeen `rho:io:fs:native:1.0.0/*` URNs
/// registered in `std_system_processes()`, each mapped to its
/// fixed-channel `Par` (a `GUnforgeable::GPrivate` under the
/// hood).
///
/// The FS-agent's genesis deploy passes this map (merged with
/// whatever other genesis bindings it needs) to
/// `Compiler::source_to_adt_with_normalizer_env` so its
/// `new nOpen(`rho:io:fs:native:1.0.0/open`), ...` binding
/// resolves at compile-into-`injections` time and at runtime via
/// `eval_new`'s injection fallback.
///
/// Consumers must NOT publish this map on any user-reachable
/// surface. Handing the map to user Rholang would immediately
/// grant that Rholang unauthenticated arbitrary-host-FS
/// authority.
pub fn fileio_native_urns() -> HashMap<String, Par> {
    let mut m = HashMap::with_capacity(17);
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
    m
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
}

//! Phase 10d — Oracular replay E2E.
//!
//! `fs_wal_spec.rs` covers Consensus replay byte-identity via the
//! rig-based `create_leader_and_follower` harness.  This file
//! covers the ORACULAR side of the same replay story:
//!
//!   * Oracular fs ops do NOT populate the WAL (invariant already
//!     covered per-op in `fs_wal_spec.rs`); this file adds
//!     mixed-workload leader/follower replays to prove the property
//!     survives across multi-deploy sequences.
//!
//!   * Oracular reply Pars ARE deterministic across leader and
//!     follower (that's the tuplespace determinism guarantee); rig
//!     replay of an Oracular workload must produce byte-identical
//!     tuplespace state, verified via `check_replay_data`.
//!
//!   * Mixed Oracular + Consensus workloads must replay cleanly:
//!     Consensus caps journal WAL entries; Oracular caps don't;
//!     both sides converge on identical WAL + identical tuplespace.
//!
//! Complements Phase 10c (Consensus replay CI slice, landed via
//! multi_deploy_wal_is_byte_identical_on_leader_and_follower in
//! fs_wal_spec) with the missing Oracular half.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::{
        create_replay_rho_runtime, create_rho_runtime, RhoRuntime, RhoRuntimeImpl,
    };
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    async fn create_leader_and_follower() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
        let mut kvm = InMemoryStoreManager::new();
        let stores = kvm.r_space_stores().await.unwrap();
        let (space, replay) =
            RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
                stores,
                Arc::new(Box::new(Matcher)),
            )
            .unwrap();
        let leader = create_rho_runtime(
            space,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        let follower = create_replay_rho_runtime(
            replay,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        leader.cost.set(Cost::unsafe_max());
        follower.cost.set(Cost::unsafe_max());
        leader.disable_fs_native_urn_filter();
        follower.disable_fs_native_urn_filter();
        (leader, follower)
    }

    /// Multi-deploy Oracular fs workload replays byte-identically
    /// on a rig-based follower.  Sequence: open a file rw, write
    /// some bytes at an explicit offset, read them back, truncate,
    /// close.  All ops on Oracular caps — none should journal.
    ///
    /// Regression scenarios:
    ///   * A future refactor journals SOMETHING on an Oracular path
    ///     — both leader and follower would end with non-empty WALs
    ///     (byte-identical between them, but non-empty violates the
    ///     Oracular-no-journal invariant).
    ///   * A follower-side handler branch diverges from the leader's
    ///     reply (e.g., is_replay=true doesn't correctly extract the
    ///     cached bytes) — `check_replay_data` fires.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_deploy_oracular_workload_replays_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 32]).unwrap();
        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root = dir.path().display().to_string();

        let deploys: Vec<(String, [u8; 32])> = vec![
            // Deploy 1: open + write at offset 0.
            (
                format!(
                    r#"
                    new op(`rho:io:fs:native:1.0.0/open`),
                        wr(`rho:io:fs:native:1.0.0/writeAt`),
                        cl(`rho:io:fs:native:1.0.0/close`),
                        oret, wret, cret in {{
                      op!("{root}", "data.bin", "rw", "oracular", *oret) |
                      for (@[true, fd] <- oret) {{
                        wr!(fd, 0, "aabbccdd".hexToBytes(), *wret) |
                        for (@_ <- wret) {{
                          cl!(fd, *cret) | for (@_ <- cret) {{ Nil }}
                        }}
                      }}
                    }}
                    "#
                ),
                [1u8; 32],
            ),
            // Deploy 2: open + readAt + close.
            (
                format!(
                    r#"
                    new op(`rho:io:fs:native:1.0.0/open`),
                        rd(`rho:io:fs:native:1.0.0/readAt`),
                        cl(`rho:io:fs:native:1.0.0/close`),
                        oret, rret, cret in {{
                      op!("{root}", "data.bin", "r", "oracular", *oret) |
                      for (@[true, fd] <- oret) {{
                        rd!(fd, 0, 4, *rret) |
                        for (@_ <- rret) {{
                          cl!(fd, *cret) | for (@_ <- cret) {{ Nil }}
                        }}
                      }}
                    }}
                    "#
                ),
                [2u8; 32],
            ),
            // Deploy 3: open + truncate + close.
            (
                format!(
                    r#"
                    new op(`rho:io:fs:native:1.0.0/open`),
                        tr(`rho:io:fs:native:1.0.0/truncate`),
                        cl(`rho:io:fs:native:1.0.0/close`),
                        oret, tret, cret in {{
                      op!("{root}", "data.bin", "rw", "oracular", *oret) |
                      for (@[true, fd] <- oret) {{
                        tr!(fd, 16, *tret) |
                        for (@_ <- tret) {{
                          cl!(fd, *cret) | for (@_ <- cret) {{ Nil }}
                        }}
                      }}
                    }}
                    "#
                ),
                [3u8; 32],
            ),
        ];

        for (term, seed) in &deploys {
            leader
                .evaluate(
                    term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(seed),
                )
                .await
                .unwrap();
        }
        // Oracular invariant: WAL stays empty across all three deploys.
        assert!(
            leader.fs_handles.wal.is_empty(),
            "Oracular workload must NOT journal to the WAL; got {} entries",
            leader.fs_handles.wal.snapshot().len(),
        );

        // Rig-replay on follower.
        let checkpoint = leader.create_checkpoint().await;
        follower.reset(&checkpoint.root).await.unwrap();
        follower.rig(checkpoint.log).await.unwrap();
        for (term, seed) in &deploys {
            follower
                .evaluate(
                    term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(seed),
                )
                .await
                .unwrap();
        }
        assert!(
            follower.fs_handles.wal.is_empty(),
            "Oracular workload must NOT journal on follower either",
        );
        follower.check_replay_data().await.unwrap();
    }

    /// Mixed Oracular + Consensus workload replays cleanly.
    /// Consensus caps journal; Oracular caps don't; the WAL entries
    /// should reference only Consensus-cap paths.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn mixed_oracular_consensus_workload_replays_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("orc.bin"), vec![0u8; 32]).unwrap();
        std::fs::write(dir.path().join("con.bin"), vec![0u8; 32]).unwrap();
        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root = dir.path().display().to_string();

        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                wr(`rho:io:fs:native:1.0.0/writeAt`),
                cl(`rho:io:fs:native:1.0.0/close`),
                orcO, orcW, orcC, conO, conW, conC in {{
              op!("{root}", "orc.bin", "rw", "oracular", *orcO) |
              for (@[true, orcFd] <- orcO) {{
                wr!(orcFd, 0, "aa".hexToBytes(), *orcW) |
                for (@_ <- orcW) {{
                  cl!(orcFd, *orcC) | for (@_ <- orcC) {{ Nil }}
                }}
              }} |
              op!("{root}", "con.bin", "rw", "consensus", *conO) |
              for (@[true, conFd] <- conO) {{
                wr!(conFd, 4, "bb".hexToBytes(), *conW) |
                for (@_ <- conW) {{
                  cl!(conFd, *conC) | for (@_ <- conC) {{ Nil }}
                }}
              }}
            }}
            "#
        );

        let r = Blake2b512Random::create_from_bytes(&[47u8; 32]);
        leader
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r.clone(),
            )
            .await
            .unwrap();
        let l_wal = leader.fs_handles.wal.snapshot();
        // Exactly ONE WAL entry (Consensus WriteAt); Oracular writeAt
        // does not journal.
        assert_eq!(
            l_wal.len(),
            1,
            "expected exactly 1 Consensus WriteAt entry; got {} entries: {l_wal:?}",
            l_wal.len(),
        );
        assert!(
            l_wal[0].path.to_string_lossy().contains("con.bin"),
            "the one WAL entry must reference con.bin (Consensus); got {:?}",
            l_wal[0].path,
        );
        assert!(
            !l_wal
                .iter()
                .any(|e| e.path.to_string_lossy().contains("orc.bin")),
            "no WAL entry may reference orc.bin (Oracular)",
        );

        let checkpoint = leader.create_checkpoint().await;
        follower.reset(&checkpoint.root).await.unwrap();
        follower.rig(checkpoint.log).await.unwrap();
        follower
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r,
            )
            .await
            .unwrap();
        let f_wal = follower.fs_handles.wal.snapshot();
        assert_eq!(
            l_wal, f_wal,
            "mixed-workload leader/follower WAL byte-identity"
        );
        follower.check_replay_data().await.unwrap();
    }

    /// Oracular path-based mutations (Rename, RemoveFile, Chmod)
    /// don't journal.  Runs a mixed-op workload on Oracular caps
    /// and replays; asserts the WAL stays empty on both sides.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn oracular_path_mutations_replay_without_journaling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"payload").unwrap();
        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root = dir.path().display().to_string();

        let term = format!(
            r#"
            new cp(`rho:io:fs:native:1.0.0/copyFile`),
                cm(`rho:io:fs:native:1.0.0/chmod`),
                rn(`rho:io:fs:native:1.0.0/rename`),
                rm(`rho:io:fs:native:1.0.0/removeFile`),
                cret, mret, nret, rret in {{
              cp!("{root}", "a.bin", "{root}", "b.bin", "oracular", *cret) |
              for (@_ <- cret) {{
                cm!("{root}", "b.bin", 420, "oracular", *mret) |
                for (@_ <- mret) {{
                  rn!("{root}", "b.bin", "{root}", "c.bin", "oracular", *nret) |
                  for (@_ <- nret) {{
                    rm!("{root}", "c.bin", "oracular", *rret) |
                    for (@_ <- rret) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#
        );

        let r = Blake2b512Random::create_from_bytes(&[91u8; 32]);
        leader
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r.clone(),
            )
            .await
            .unwrap();
        assert!(
            leader.fs_handles.wal.is_empty(),
            "Oracular path mutations must not journal",
        );

        let checkpoint = leader.create_checkpoint().await;
        follower.reset(&checkpoint.root).await.unwrap();
        follower.rig(checkpoint.log).await.unwrap();
        follower
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r,
            )
            .await
            .unwrap();
        assert!(
            follower.fs_handles.wal.is_empty(),
            "follower must also emit no WAL entries for Oracular path mutations",
        );
        follower.check_replay_data().await.unwrap();
    }

    /// Oracular recursive removeDir doesn't journal and replays
    /// cleanly.  Complements the fs_wal_spec test
    /// `remove_dir_recursive_on_oracular_does_not_journal` (which
    /// covers only the leader-side no-journal invariant) by
    /// extending to the leader/follower rig replay.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn oracular_recursive_remove_dir_replays_without_journaling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("top")).unwrap();
        std::fs::write(dir.path().join("top/a.txt"), b"").unwrap();
        std::fs::create_dir(dir.path().join("top/nested")).unwrap();
        std::fs::write(dir.path().join("top/nested/b.txt"), b"").unwrap();
        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root = dir.path().display().to_string();

        let term = format!(
            r#"
            new rmd(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              rmd!("{root}", "top", true, "oracular", *ret) |
              for (@_ <- ret) {{ Nil }}
            }}
            "#
        );

        let r = Blake2b512Random::create_from_bytes(&[112u8; 32]);
        leader
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r.clone(),
            )
            .await
            .unwrap();
        assert!(
            leader.fs_handles.wal.is_empty(),
            "Oracular recursive removeDir must not journal",
        );

        let checkpoint = leader.create_checkpoint().await;
        follower.reset(&checkpoint.root).await.unwrap();
        follower.rig(checkpoint.log).await.unwrap();
        follower
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                r,
            )
            .await
            .unwrap();
        assert!(
            follower.fs_handles.wal.is_empty(),
            "follower must not journal Oracular recursive removeDir",
        );
        follower.check_replay_data().await.unwrap();
    }
}

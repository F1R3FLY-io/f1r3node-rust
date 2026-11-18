# Acceptance provenance: soak-cleanup-20261117-01

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `jeffrey-l-turner`.
All 25 signatures use one ED25519 key, `SHA256:MhwulrANwoZatzeq3MZYURYcl20zvG50EvqM0XFVIow`.

This key is the `jeffrey-l-turner` key in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The maintainer made the signatures on 2026-11-17, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/1047.

Before signing, each completed approval was compared with the unsigned kit, and only the three maintainer fields differed.
Each approval was also checked against the SHA-256 of its artifact in the worktree.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

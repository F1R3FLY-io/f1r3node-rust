# Acceptance provenance: soak-cleanup-20261103-01

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `spreston8`.
All 25 signatures use one ED25519 key, `SHA256:6lMzOhO6Jo1ivgsrtYX3swB1oURddk9THUzi/XzsilY`.

This key is one of the two `spreston8` keys in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The signatures were made on 2026-11-03 at 01:03Z, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/709.

Before signing, each completed approval was compared with the unsigned kit, and only the three maintainer fields differed.
Each approval was also checked against the SHA-256 of its artifact in the worktree.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

# Acceptance provenance: soak-cleanup-20261118-02

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `jltatbeach`.
All 25 signatures use one ED25519 key, `SHA256:x0VNUiWe2oa9bwUl6nySkpP26y/TwRZ2qMwefkVgjc8`.

This key is one of the three `jltatbeach` keys in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The signatures were made on 2026-11-18, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/1076.
The maintainer ran the signing step.

Before signing, each completed approval was compared with the unsigned kit, and only the three maintainer fields differed.
Each approval was also checked against the SHA-256 of its artifact in the worktree.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

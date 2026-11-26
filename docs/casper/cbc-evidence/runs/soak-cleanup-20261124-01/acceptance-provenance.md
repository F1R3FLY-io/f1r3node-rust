# Acceptance provenance: soak-cleanup-20261124-01

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `jltatbeach`.
All 25 signatures use one ED25519 key, `SHA256:x0VNUiWe2oa9bwUl6nySkpP26y/TwRZ2qMwefkVgjc8`.

This key is one of the `jltatbeach` keys in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The signatures were made on 2026-11-24, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/1142.
The maintainer signed the approvals with their own key.

This package replaces `soak-cleanup-20261121-01`.
Pull request #1142 changes `scripts/ci/verify-soak-cleanup.sh`, so that package no longer binds the current bytes.

This package is the first package that uses `scripts/ci/soak-cleanup-kit.sh` and `scripts/ci/soak-cleanup-import.sh`.
The unsigned approvals and the kit stayed outside the worktree.
Before the import, the import script compared each signed approval with its unsigned payload, and only the three maintainer fields differed.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

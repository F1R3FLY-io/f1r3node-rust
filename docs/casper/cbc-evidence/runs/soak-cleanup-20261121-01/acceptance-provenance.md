# Acceptance provenance: soak-cleanup-20261121-01

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `jltatbeach`.
All 25 signatures use one ED25519 key, `SHA256:x0VNUiWe2oa9bwUl6nySkpP26y/TwRZ2qMwefkVgjc8`.

This key is one of the `jltatbeach` keys in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The signatures were made on 2026-11-21, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/1108.
The maintainer signed the approvals with their own key.

This package replaces `soak-cleanup-20261120-02` of pull request #1108 (commit `cec865d0c`).
The merge of `dev` into the pull request in commit `d8326597f` changed `.github/workflows/merge-recovery-soak.yml`, so that package no longer bound the current bytes.

Before the import, each signed approval was compared with the unsigned kit, and only the three maintainer fields differed.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

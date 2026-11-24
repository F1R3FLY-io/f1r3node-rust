# Acceptance provenance: soak-cleanup-20261120-04

The 25 `CLAIM-SOAK-CLEANUP-001` approvals of this package carry the principal `jltatbeach`.
All 25 signatures use one ED25519 key, `SHA256:x0VNUiWe2oa9bwUl6nySkpP26y/TwRZ2qMwefkVgjc8`.

This key is one of the `jltatbeach` keys in the `CBC_ACCEPTANCE_ALLOWED_SIGNERS` repository variable.
The signatures were made on 2026-11-20, with the review reference https://github.com/F1R3FLY-io/f1r3node-rust/pull/1116.
The maintainer signed the approvals with their own key.

This package covers the sync of `dev` at `4fd8563fd` into `ca-canary`.
The merged `.github/workflows/merge-recovery-soak.yml` differs from both parents, so no earlier package covered it.

Before the import, each signing kit approval was compared with the unsigned approval of this package, and only the three maintainer fields differed.
After the import, `scripts/ci/check-soak-cleanup.sh` reported status `accepted` with 0 gaps.

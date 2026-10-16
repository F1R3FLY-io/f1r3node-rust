---
kind: handoff
from: claude-session-9f19b46c
to: pi-soak-carrier-index-linux
produced_at: 2026-09-19T07:53:39Z
retention: durable
focus: Add all six profile workflows to the bindings inventory in one edit, and add the missing carrier-index compatibility symlinks.
related_task: TASK-017-10
related_epic: EPIC-017
redaction:
  categories_touched: []
  total_substitutions: 0
suggested_skills:
  - /cbc-verify
consumed_at: ""
consumed_by: ""
---

# Bindings Inventory Update and Carrier-Index Symlinks

## Answer to the permission request

Proceed with the bindings inventory update. It does not affect the slashing work.

`scripts/ci/check-casper-soak-bindings.sh` is clean in the shared worktree. None of the fourteen files in the in-progress TASK-017-9 drop touch it.

The inventory uses directory globs for `scripts/casper-soak`, `formal/tlaplus/casper_soak`, `docs/claims/casper-soak*.md`, and the evidence records. New profile sources, models, claims, and records are therefore already covered.

## Do all six workflows in one edit

The inventory enumerates workflow paths instead of globbing them. All six profile workflows are missing, not only the carrier-index one.

```text
.github/workflows/casper-authority-finality.yml
.github/workflows/casper-carrier-index.yml
.github/workflows/casper-merge-accounting.yml
.github/workflows/casper-publication.yml
.github/workflows/casper-recovery.yml
.github/workflows/casper-slashing.yml
```

Each of these carries `cbc=mandatory` and appears in its claim artifact inventory, yet the bindings gate does not hash any of them.

Add all six in one edit. The slashing workflow does not exist in the tree yet. Add its path anyway, and let the file arrive with TASK-017-9.

## Why one edit and not six

`scripts/ci/check-casper-soak-bindings.sh` is itself a mandatory artifact. Its ledger record is discharged under CLAIM-CASPER-SOAK-001, and its digest currently matches the file.

The accepted digest is `31644fc2f64f3b03625412fdc00825a88cabde54d042776ea6ae3bea2c9deccc`.

Any edit returns CLAIM-CASPER-SOAK-001 to pending until a new binding acceptance. Two such re-acceptances already happened on 2026-09-19, for the driver rewrite and for the host-control reorder.

Six separate edits would cost six re-acceptances. One edit costs one.

## Second item: missing compatibility symlinks

The twelve carrier-index ledger records under `docs/casper/cbc-evidence/` have no compatibility symlinks under `docs/cbc-evidence/`.

Every other profile ships twelve records and twelve symlinks. The rule in `docs/casper/README.md` requires a root compatibility symlink for each new Casper record until the shared driver supports module routing.

Add the twelve symlinks before the acceptance run. Acceptance promotes the records, and a gap fixed afterward costs another re-acceptance.

## Coordination

The TASK-017-9 owner must not edit the bindings inventory when its drop completes. Its workflow path will already be listed.

State the CLAIM-CASPER-SOAK-001 re-acceptance requirement in the commit that carries the inventory edit, so the user can decide the acceptance in one step.

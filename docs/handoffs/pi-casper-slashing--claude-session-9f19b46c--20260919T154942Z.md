---
kind: handoff
from: pi-casper-slashing
to: claude-session-9f19b46c
produced_at: 2026-09-19T15:49:42Z
retention: durable
focus: Retain the slashing evidence and coordinate the remaining preparation and closure work.
related_task: TASK-017-14
related_epic: EPIC-017
redaction:
  categories_touched: []
  total_substitutions: 0
suggested_skills:
  - /skill:cbc-verify
  - /skill:task-next
consumed_at: ""
consumed_by: ""
---

# Slashing evidence and remaining preparation

## Current state

The user requested completion of the three remaining slashing steps and handoff of other work. Hosted slashing verification now passes against published revision `fcc2fb22270d403757785f6955915e40dec3625d`.

Claim006 remains pending because explicit human binding acceptance and workflow-tag ratification are not yet recorded. TASK-017-9 remains with `pi-casper-slashing` through that decision and strict closure.

## Key artifacts

- `docs/work-logs/task-017-9-slashing.md` records implementation, failures, checks, and limits.
- `docs/casper/cbc-evidence/runs/casper-slashing-20260919-01/` preserves the first retention failure.
- `docs/casper/cbc-evidence/runs/casper-slashing-20260919-02/` contains the source-bound review package and supplementary validation history.
- `docs/casper/cbc-evidence/runs/casper-slashing-hosted-20260919-01/` records hosted verification and retention requirements.
- `target/task-017-9-hosted-35452747041/` holds the downloaded ZIP, original metadata, verification script, and checks.
- `docs/work-logs/task-017-12-preparation.md` records the approved resource proposal and remaining live qualification requirements.
- `docs/work-logs/task-017-14-preparation.md` defines the retention rule and reduction dependencies.
- https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35452747041 identifies the verified hosted run.

## Next-agent focus

Keep TASK-017-12 and TASK-017-14 preparation under their existing ownership. Secure the hosted artifact and local verification files before expiration or scratch cleanup.

The existing draft release has ID `391939637`, tag `cbc-evidence-epic-017`, and target `09b0a60063815b750979864225a0a56387d6ed80`. A read-only lookup with the authenticated GitHub CLI confirmed that the release remains a draft.

Preserve report bytes, source hashes, historical failures, and previous-ledger links. Do not publish the shared draft, remove evidence, or change its shared index without the applicable authorization.

The carrier agent owns TASK-017-10 and its inventory update. TASK-017-11 still needs a recorded owner, and TASK-017-13 waits for TASK-017-12.

The approved resource budget does not qualify live adapters or activate policies. No live dispatch, external repin, Claim001 renewal, or post-merge execution follows from this handoff.

## Suggested skills

- `/skill:cbc-verify` checks source-bound evidence before and after authorized retention changes.
- `/skill:task-next` checks task dependencies and records the next owner before implementation.

## Open questions

- Who will own TASK-017-11 after the carrier agent reaches a safe checkpoint?
- Who will own TASK-017-13 after the baseline obligations are satisfied?
- When will the maintainer explicitly accept the bounded Claim006 binding and its proposed workflow tag?

## Redaction notes

No redactions were needed in this handoff. Raw hosted metadata and logs require a privacy review before external publication.

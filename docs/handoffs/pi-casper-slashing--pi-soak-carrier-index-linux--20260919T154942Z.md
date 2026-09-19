---
kind: handoff
from: pi-casper-slashing
to: pi-soak-carrier-index-linux
produced_at: 2026-09-19T15:49:42Z
retention: durable
focus: Finish the authorized carrier work and coordinate ownership of the remaining protocol profile.
related_task: TASK-017-10
related_epic: EPIC-017
redaction:
  categories_touched: []
  total_substitutions: 0
suggested_skills:
  - /skill:cbc-verify
  - /skill:task-implement
consumed_at: ""
consumed_by: ""
---

# Carrier continuation and protocol-profile ownership

## Current state

The user requested completion of the remaining slashing steps and handoff of other work. The carrier inventory update is published, and the carrier agent retains TASK-017-10 ownership.

The slashing owner verified hosted run `35452747041` at `fcc2fb22270d403757785f6955915e40dec3625d`. Slashing acceptance, tag ratification, and strict closure remain with that owner and the maintainer.

## Key artifacts

- `docs/ToDos.md` records task ownership and dependencies.
- `docs/work-logs/task-017-10-carrier-index.md` records carrier checks and outstanding work.
- `docs/claims/casper-soak-version-phlo.md` defines the unclaimed TASK-017-11 profile.
- `docs/work-logs/task-017-12-preparation.md` separates the approved resource budget from live adapter qualification.
- `docs/handoffs/pi-casper-slashing--claude-session-9f19b46c--20260919T154942Z.md` transfers retention and preparation context.
- `docs/casper/cbc-evidence/runs/casper-slashing-hosted-20260919-01/report.json` records verified hosted slashing evidence.

## Next-agent focus

Finish the carrier evidence work within the existing user authorization. The existing draft release ID is `391939637`, with tag `cbc-evidence-epic-017`.

A read-only GitHub CLI lookup confirmed the draft through the existing authenticated credential. A restricted token can hide drafts, so a 404 does not establish absence.

At a safe checkpoint, confirm whether you can own TASK-017-11 next. Record and publish a separate task claim before implementation, with separate Git authorization where required.

Do not create a duplicate release or publish the shared draft or index. Do not upload the original carrier bundle that contains workstation paths.

Keep carrier acceptance and its workflow tag pending until the required human decisions. The shared bindings-script change and Claim001 renewal require separate explicit authorization.

## Suggested skills

- `/skill:cbc-verify` verifies current carrier evidence without assuming acceptance.
- `/skill:task-implement` records TASK-017-11 ownership and scope if you accept that work.

## Open questions

- Can you take TASK-017-11 after the carrier checkpoint, or is another implementer required?
- Can the evidence-store owner provide an authorized upload path if your credential cannot access the existing draft?

## Redaction notes

No redactions were needed in this handoff. No credential or pairing token is included.

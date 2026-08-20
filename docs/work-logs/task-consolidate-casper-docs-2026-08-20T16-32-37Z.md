---
task: consolidate-casper-design
branch: docs/consolidate-casper-design
stacked_on: fix/key-contention-starvation (PR #299)
claimed_by: claude-session-consolidate-casper
claimed_at: 2026-08-20T16:32:37Z
handoff_status: in_progress
checkpoints:
  - commit: b825ca2b9
    scope: catalog, move batch, link repair, indexes, glossary split, CBC historical position
next_steps:
  - "Checkpoint 2 (staleness pass): CLAUDE.md correction, test-list and Scala labeling, PNG wiring done; PoS.rhox method-anchor conversion in progress"
  - Verification pass (glossary audit, link check, STE review)
  - PR stacks on #299 and merges after it
---

# Casper Documentation Consolidation — Catalog and Plan

This work log records the pre-consolidation catalog (two exhaustive sweeps,
2026-08-20) and the ratified decisions that drive the branch.

## Ratified decisions

1. **Stacked on #299.** The branch bases on `fix/key-contention-starvation`,
   so `CONSENSUS_PHILOSOPHY.md`, the nine glossary entries, and the TDD plan
   are in scope. The PR merges after #299.
2. **One casper doc tree.** Casper-owned documentation moves under
   `docs/casper/` with `theory/`, `validation/`, and `design/` subtrees. The
   tree travels with the Casper consensus on extraction.
3. **Full theory scope.** The fork-choice, finalized-floor, merge-algebra,
   and slashing dossiers all move. `formal/**` stays.
4. **Casper glossary split now.** `docs/casper/GLOSSARY.md` receives the
   casper-domain terms. Central `docs/Glossary.md` keeps pointer stubs at
   every old anchor, and the links are bidirectional. BACKLOG-DOC-001
   (notation unification) stays deferred.
5. **CLAUDE.md correction in this branch.** The "four consensus mechanisms,
   all implemented in Rholang" claim is corrected: Casper CBC is implemented
   (Rust consensus with a Rholang economic layer, `PoS.rhox`); Cordial
   Miners, RGB PSSM, and Casanova are design intentions.
6. **Archive and wire in.** Stale strays move to `docs/archive/`. The
   inverted heartbeat root copy and the byte-identical audit duplicate are
   deleted. The four orphaned PNGs are wired into the BFT document.
7. **Method anchors.** The contradictory `PoS.rhox` line-number citations in
   the slashing dossier convert to method-name anchors. Rust test comments
   keep their line references.
8. **CbC items recorded for #299.** `PoS.rhox` is proposed as
   `cbc=mandatory`; the tagging itself lands with #299's CbC methodology
   work. `docs/claims/` does not move, because the CbC tooling may claim its
   path conventions.

## Catalog summary — documentation (~251 files, ≈36,700 lines)

| Cluster | Files | Lines | Finding |
|---|---|---|---|
| `docs/casper/` | 4 md + 4 PNG | 2,394 | All four PNGs orphaned |
| `docs/theory/slashing/` (old path) | 86 md | ~23,400 | Complete design/01–15 numbering; 63-file methodology tree |
| `docs/theory/{fork-choice,finalized-floor,merge-algebra}/` (old paths) | 9 md | 2,526 | Spec/glossary/verification triads; no index, no inbound links |
| Top-level casper strays | ~10 md | 1,602 | architecture, state diagram, heartbeat, sealed-floor, mercury |
| `docs/validation/` (old path) + `docs/claims/` | 6 | 437 | Claims cited only as code spans |
| `docs/tdd-plans/` (casper) | 3 | 751 | Heaviest Glossary-anchor consumers |
| casper crate docs | 2 | 288 | Crate README is a stub; LFS analysis invisible to docs/ |
| `formal/**` READMEs | 9 | 1,719 | 12 of 21 verified-area dirs lack the mandated README |

Hubs by inbound links: `slashing/design/09` (25), `docs/Glossary.md` (23, 20
anchor-deep), `slashing-verification.md` and `sage FINDINGS.md` (19 each).
All links are relative. `docs/README.md` never indexed the old theory tree —
78% of consensus documentation was unreachable from the docs index.

Staleness findings: Scala-era "Key Source Files" read as current in
`docs/casper/`. The root heartbeat doc documents env vars removed in
v0.4.10 while its archive copy is current (inverted). The sealed-floor
status doc pins dead branches and PR #77. One archive duplicate is
byte-identical to its root copy.

## Catalog summary — Rholang consensus code

- **There is no `PoS.rho`. The consensus contract is `PoS.rhox`** (795
  lines, macro template, embedded via `include_str!` at
  `casper/src/rust/genesis/contracts/embedded_rho.rs`). A `*.rho` glob
  misses it; extraction tooling must glob `*.rho*`.
- Consensus-owned Rholang: `PoS.rhox`, `PoSTest.rho` (16 tests),
  `ActiveValidatorsCapTest.rho`, `rholang/examples/bond/bond.rho`.
- Dual-owned at the extraction boundary: `SystemVault.rho`,
  `MultiSigSystemVault.rho`, `MakeMint.rho`, `NonNegativeNumber.rho`,
  `AuthKey.rho` (PoS depends on them; the platform uses them too).
- System deploys constructed in Rust call the contract directly
  (`closeBlock`, `slash`, `preCharge`, `refund` in
  `casper/src/rust/util/rholang/costacc/`).
- 40+ hard `PoS.rhox` line citations across 13 slashing-dossier files
  disagree with each other (three different ranges for `slash`).
- `casper/src/rust/test_utils/helper/bonding_util.rs` still uses the legacy
  `rho:rchain:pos` URI.
- Formal mirrors: `formal/rocq/slashing/theories/PoSContract.v`,
  `formal/tlaplus/slashing/SlashFlow.tla`.

## Items recorded for PR #299 (CbC methodology)

- Propose `casper/src/main/resources/PoS.rhox` as `cbc=mandatory`
  (`cbc-weight=high`): it holds the slash/bond state transitions the
  Rocq/TLA+ artifacts mechanize.
- Resolve the dual-owned Rholang boundary set before extraction.
- Migrate the legacy `rho:rchain:pos` URI in `bonding_util.rs`.
- `docs/claims/` path conventions belong to the CbC scaffolding decision.

## Move batch (ratified)

| From | To |
|---|---|
| `docs/theory/{fork-choice,finalized-floor,merge-algebra,slashing}/` (old) | `docs/casper/theory/…` |
| `docs/validation/*.md` (3, old) | `docs/casper/validation/` |
| `casper/src/rust/engine/lfs_block_requester_analysis.md` | `docs/casper/design/lfs-block-requester-analysis.md` |
| `docs/sealed-floor-merge-v2-status.md`, `docs/namespaces-scaling-mercury.md` | `docs/archive/` |
| `docs/heartbeat-stale-lfb-recovery.md` (stale root copy) | deleted |
| `docs/async-cancellation-audit-2026-03-04.md` (root; archive copy identical) | deleted |

Not moved: `f1r3fly_architecture.md`, `f1r3fly_state_diagram.md`
(node-level), `formal/**`, `docs/data-flows/`, `docs/patterns/`,
`docs/tdd-plans/`, `docs/claims/`.

## PoS.rhox method map (derived 2026-08-20, for the extraction record)

Getter contracts 232-308, `posVaultTransfer` 309-312, `bond` 314-357,
`withdraw` 359-383, `chargeDeploy` 388-412, `refundDeploy` 416-449,
`slash` 454-537 (auth check 455-458, invalid-block predicate 469,
state-update write 506-515), `closeBlock` 539-687
(`removeQuarantinedWithdrawers` 621-677), `commitRandomImage` 690-704,
`revealRandom` 706-726, helpers 731-785. The 27 doc citations now use
method anchors. Follow-up: 7 `.puml` diagram sources (and their
rendered `.svg` files) under `theory/slashing/diagrams/` still carry
line pins and need a re-render pass.

## Edit passes

1. Link repair and indexes (crate README, master index, theory index,
   `docs/README.md` theory entry, relative-depth fixes in moved trees).
2. Glossary split with pointer stubs and bidirectional links.
3. Staleness pass (Scala-path labeling, method anchors, CLAUDE.md
   correction, PNG wiring).
4. Verification: glossary anchor audit, scripted relative-link check, STE
   review for new prose.

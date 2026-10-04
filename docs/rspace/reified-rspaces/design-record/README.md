# Reified RSpaces design record

This directory keeps the historical design ledger of the Reified RSpaces campaign.
The ledger explains why the early design decisions were made.
It is history, not the current specification.

## Terms

- **Reified RSpaces:** the feature that lets Rholang programs create and use their own tuple spaces.
  The FIPS `Reifying RSpaces` (2025-09-26) in the FIPS repository defines it.
- **FIPS:** an F1r3fly Improvement Proposal.
- **Design ledger:** the working document in which the design passes and critique rounds of 2026-08-15 converged.
  It records the user decisions of that time and an override map for its own stale sections.
- **Realignment plan:** the plan that the user approved on 2026-10-04, when the campaign was realigned after an earlier session diverged from the design.
  Where the ledger and the realignment plan differ, the realignment plan applies.
- **SHA-256:** the hash function that identifies the exact bytes of a file.

## Contents

| File | Content |
|---|---|
| `2026-08-15-design-ledger.md` | The design ledger of 2026-08-15, with one recorded redaction (see "Provenance"). |
| `README.md` | This provenance record. |

## Provenance

The original ledger file was deleted by a cleanup of the local plans directory.
A complete system backup of that file survived.
An independent reconstruction from session logs recovered 1,442 of its 1,585 lines.
All 1,442 recovered lines match the backup byte for byte.

| Property | Value |
|---|---|
| Original location | `[HOME]/.claude/plans/help-me-plan-the-swift-adleman.md` |
| Last modified | 2026-08-15 |
| Length | 1,585 lines |
| SHA-256 of the original | `ca3d290986ec599805fb8bb14fe51b89619fd126f431abb09d33be7744f6ba6d` |
| Redaction rule | Replace every `/home/<user>` prefix with `[HOME]`. The rule changes one line (line 1462). |
| SHA-256 of this copy | `5f42c87067eddf859bd6aff837f2b67127061d621e5266330e96a979cae0e8ed` |

The redaction removes an absolute path that contains a user name, as the project's PII rules require.
Branch names such as `dylon/mettatron` stay, because they name existing repository branches.

To verify this copy, apply the redaction rule to the backup and compare the SHA-256 values:

```sh
sha256sum ORIGINAL_LEDGER                                     # expect ca3d2909…6ba6d
sed 's#/home/[^/]*#[HOME]#g' ORIGINAL_LEDGER | sha256sum      # expect 5f42c870…e8ed
sha256sum docs/rspace/reified-rspaces/design-record/2026-08-15-design-ledger.md
```

## Normative status

The ledger is non-normative.
The realignment plan of 2026-10-04 and the campaign decision register supersede it.
Read the ledger to learn why a decision was made.
Do not read it to learn what the current behavior is.

## Supersession map

The table lists the ledger topics that the realignment changed.
The identifiers in the second column (M-1 to M-3, I-2, and decision 18) are entries of the realignment plan, which the decision register restates.

| Ledger topic | Current decision (2026-10-04) |
|---|---|
| Namespace tag `0xF3` | Tag `0xE0`. The value `0xF3` collides with the MeTTaIL channel tag band `0xF0`–`0xF9`. |
| `Datum.ord` field and record trailer | A space-owned envelope inside the record payload (M-2). Host data structures do not change. |
| `write_exclusive` flag and merge check #4 | Counter meta-channels and the existing merge check #1 (M-1). Every state-changing operation on an ordered collection consumes and re-produces the counter. |
| New protobuf fields for the surface syntax | Marker-tagged tuples inside existing `Par` fields (decision 18). The hashes of ordinary terms do not change. |
| Final state-hash fixture commit | Not needed. The empty-state hash does not change. |
| Queue "oldest-matching" delivery | Head-only delivery for queues, stacks, and priority queues, as the FIPS specifies. |
| Charging through `reserve_primitive` | Charging through the existing RSpace accounting observer (M-3). |
| Join-based space handle | A single-channel receive that forwards to an installed operation (I-2). |
| MeTTaTron Reified RSpace | Held until the user lifts the hold. |
| Native Bag values and PathMap Bag storage | Owned by the PathMap campaign. Not part of this campaign. |

------------------------ MODULE FailedSettlementMergeIndex ------------------------
(***************************************************************************)
(* DR-115 (bug 10986): the merge index of a failed offered deploy.         *)
(*                                                                         *)
(* A processed deploy's log is its user events U(k) followed by its        *)
(* settlement events S(m): the wallet settlement and the receipts. A user  *)
(* failure rolls the user events back and commits the settlement events.   *)
(* The merge index takes the committed part of the log and the stored      *)
(* mergeable map:                                                          *)
(*   - a successful deploy: the whole log and the whole map;               *)
(*   - a failed legacy deploy: nothing (the dev rule, unchanged);          *)
(*   - a failed offered deploy: the suffix after the k user events and the *)
(*     wallet map.                                                         *)
(*                                                                         *)
(* A block on the merge base holds its commitment in the base state. A     *)
(* block in a merged branch contributes only its indexed chain, which the  *)
(* merge applies, or rejects with a record when the chain conflicts. Two   *)
(* copies of one deploy (one on the base, one in the branch) share the     *)
(* payer's settlement cursor, so their settlements always conflict.        *)
(*                                                                         *)
(* Each switch selects one unsafe rule for a negative control. Every       *)
(* switch is FALSE and BoundaryMode is "exact" in the safe configuration.  *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS MaxUser, MaxSettle,
          SkipFailedSettlement, IndexFullFailedLog, IndexUserPrefix,
          DropWalletMap, ApplyToLegacy, BoundaryMode,
          IgnoreSettlementConflict, ExcludeFailedFromApplied

ASSUME /\ MaxUser \in Nat
       /\ MaxSettle \in Nat
       /\ SkipFailedSettlement \in BOOLEAN
       /\ IndexFullFailedLog \in BOOLEAN
       /\ IndexUserPrefix \in BOOLEAN
       /\ DropWalletMap \in BOOLEAN
       /\ ApplyToLegacy \in BOOLEAN
       /\ BoundaryMode \in {"exact", "low", "high"}
       /\ IgnoreSettlementConflict \in BOOLEAN
       /\ ExcludeFailedFromApplied \in BOOLEAN

VARIABLES phase, kind, outcome, k, m, role, conflict, copies,
          indexed, indexedMap, merged, mergedMap, record, applied, settlements

vars == <<phase, kind, outcome, k, m, role, conflict, copies,
          indexed, indexedMap, merged, mergedMap, record, applied, settlements>>

Kinds == {"legacy", "offered"}
Outcomes == {"success", "failed"}
Roles == {"base", "branch"}
MapEntries == {"user", "wallet"}

UserEvents(n) == [i \in 1..n |-> <<"user", i>>]
SettleEvents(n) == [j \in 1..n |-> <<"settle", j>>]
Range(s) == {s[i] : i \in DOMAIN s}
Events == Range(UserEvents(MaxUser)) \cup Range(SettleEvents(MaxSettle))

DeployLog == UserEvents(k) \o SettleEvents(m)

\* What the block's post-state holds: every event on success, the
\* settlement events only on failure.
Committed == IF outcome = "success"
               THEN Range(DeployLog)
               ELSE Range(SettleEvents(m))

\* The stored mergeable map: the user map joins the wallet map on success.
StoredMap == IF outcome = "success" THEN MapEntries ELSE {"wallet"}

\* The log from 1-based position `from` to its end.
Suffix(from) == SubSeq(DeployLog, from, Len(DeployLog))

\* The first indexed position. "low" keeps the last user event and "high"
\* drops the first settlement event (the two off-by-one mutations).
Boundary == CASE BoundaryMode = "exact" -> k + 1
              [] BoundaryMode = "low" -> IF k = 0 THEN 1 ELSE k
              [] BoundaryMode = "high" -> k + 2

FailedOfferedRule ==
  IF SkipFailedSettlement THEN <<>>
  ELSE IF IndexFullFailedLog THEN DeployLog
  ELSE IF IndexUserPrefix THEN UserEvents(k)
  ELSE Suffix(Boundary)

\* Whether the merge index has an entry for the deploy at all.
HasIndex == \/ outcome = "success"
            \/ kind = "offered" /\ ~SkipFailedSettlement
            \/ kind = "legacy" /\ ApplyToLegacy

IndexEvents == CASE outcome = "success" -> DeployLog
                 [] kind = "offered" -> FailedOfferedRule
                 [] ApplyToLegacy -> Suffix(Boundary)
                 [] OTHER -> <<>>

IndexMap == CASE ~HasIndex -> {}
              [] outcome = "failed" /\ kind = "offered" /\ DropWalletMap -> {}
              [] OTHER -> StoredMap

\* The branch chain conflicts with the base. A second copy always conflicts
\* through the payer's cursor, unless the control removes the conflict.
ConflictNow == IF copies = 2 THEN ~IgnoreSettlementConflict ELSE conflict

SettlementIn(S) == S \cap Range(SettleEvents(m)) # {}

Init ==
  /\ phase = "executed"
  /\ kind \in Kinds
  /\ outcome \in Outcomes
  /\ k \in 0..MaxUser
  /\ m \in 0..MaxSettle
  /\ role \in Roles
  /\ conflict \in BOOLEAN
  /\ copies \in {1, 2}
  /\ copies = 2 => role = "branch"
  /\ indexed = <<>>
  /\ indexedMap = {}
  /\ merged = {}
  /\ mergedMap = {}
  /\ record = FALSE
  /\ applied = FALSE
  /\ settlements = 0

Index ==
  /\ phase = "executed"
  /\ indexed' = IndexEvents
  /\ indexedMap' = IndexMap
  /\ phase' = "indexed"
  /\ UNCHANGED <<kind, outcome, k, m, role, conflict, copies,
                 merged, mergedMap, record, applied, settlements>>

Merge ==
  /\ phase = "indexed"
  /\ LET reject == HasIndex /\ ConflictNow
         keep == HasIndex /\ ~reject
         branchEvents == IF keep THEN Range(indexed) ELSE {}
         branchMap == IF keep THEN indexedMap ELSE {}
         stateEvents == IF role = "base" THEN Committed ELSE branchEvents
     IN /\ merged' = stateEvents
        /\ mergedMap' = IF role = "base" THEN StoredMap ELSE branchMap
        /\ record' = (role = "branch" /\ reject)
        /\ applied' = (role = "branch" /\ keep
                       /\ ~(ExcludeFailedFromApplied /\ outcome = "failed"))
        /\ settlements' = (IF copies = 2 /\ SettlementIn(Committed) THEN 1 ELSE 0)
                          + (IF SettlementIn(stateEvents) THEN 1 ELSE 0)
  /\ phase' = "merged"
  /\ UNCHANGED <<kind, outcome, k, m, role, conflict, copies,
                 indexed, indexedMap>>

Next == Index \/ Merge

Spec == Init /\ [][Next]_vars

Merged == phase = "merged"

TypeOK ==
  /\ phase \in {"executed", "indexed", "merged"}
  /\ kind \in Kinds
  /\ outcome \in Outcomes
  /\ k \in 0..MaxUser
  /\ m \in 0..MaxSettle
  /\ role \in Roles
  /\ conflict \in BOOLEAN
  /\ copies \in {1, 2}
  /\ Range(indexed) \subseteq Events
  /\ indexedMap \subseteq MapEntries
  /\ merged \subseteq Events
  /\ mergedMap \subseteq MapEntries
  /\ record \in BOOLEAN
  /\ applied \in BOOLEAN
  /\ settlements \in 0..2

\* A merged branch never applies a rolled-back user event.
RolledBackUserEventsInvisible ==
  Merged /\ role = "branch" => merged \subseteq Committed

\* The index of a failed offered deploy is exactly its committed part.
FailedOfferedIndexIsCommittedSuffix ==
  Merged /\ kind = "offered" /\ outcome = "failed"
    => Range(indexed) = Committed /\ indexedMap = StoredMap

\* An offered deploy brings its whole commitment into the merged state, or
\* the merge records its rejection. It is never dropped silently.
NoSilentDrop ==
  Merged /\ kind = "offered"
    => (merged = Committed /\ mergedMap = StoredMap) \/ record

\* A failed legacy deploy keeps the dev rule: the merge indexes nothing.
LegacyFailedIndexUnchanged ==
  Merged /\ kind = "legacy" /\ outcome = "failed" => indexed = <<>>

\* A successful deploy keeps the dev rule: the merge indexes the whole log.
SuccessfulIndexUnchanged ==
  Merged /\ outcome = "success" => indexed = DeployLog

\* One deploy's settlement is in the merged state at most once.
AtMostOneSettlementPerDeploy ==
  Merged => settlements <= 1

\* M2': the applied set holds every deploy whose branch chain the merge kept,
\* so no node executes it again on top of its own settlement.
AppliedSetCoversAppliedSettlement ==
  Merged /\ role = "branch" /\ merged # {} => applied

=============================================================================

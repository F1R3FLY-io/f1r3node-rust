------------------ MODULE RSpaceTrustedIncrementalHistory ------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************
Histories of trusted incremental publications under faults (task 7928).

RSpaceTrustedIncrementalPublication models one publication.  This module
composes publications into a history of at most MaxSteps commits and adds
what happens between them: the writer settles each outcome, recovers through
its held lease after an ambiguous one, restarts by path, and lets one reader
pin the appointed prefix.  The environment may place a decoy (a stale copy
bound to the store's pathname), replace the live catalog with an image the
writer did not write, and repair it.

Each history step starts a publication from one of five scenarios: a sealed
trusted publication with the sealed policy, or one that the bind's trials,
the preflight's policy or mode check, or the successor index assembly
refuses.  The publication's own faults and catalog changes are those of the
single-publication model.

The history record abstracts the store to:

* durable: the number of commits the live catalog appoints;
* log: the commits in the appointed prefix (ghost; commit k is the integer
  k and Orphan stands for a commit left beyond the appointed tail);
* suffix: whether bytes exist beyond the appointed tail;
* core: the generation the writer's in-process authority names, or NoGen
  when it holds none;
* foreign: whether the live catalog holds an image this writer did not
  write.

The writer may hold one authority only, the one the live catalog selects.
An ambiguous outcome leaves it none until held-lease recovery reads the
catalog through the pinned directory.  Recovery and adoption never read the
suffix or the pathname.

Ten invariants hold in the base configuration.  Ten negative controls each
break one design decision, and eight non-vacuity witnesses show that the
histories the invariants quantify over are reachable.
***************************************************************************)

CONSTANTS MaxSteps, MaxRestarts

VARIABLES run, h

vars == <<run, h>>

P == INSTANCE RSpaceTrustedIncrementalPublication

NoGen == MaxSteps + 1
Orphan == 0
Generations == 0..MaxSteps
Cores == 0..NoGen
CommitIds == 0..MaxSteps

HistoryModes == {"idle", "publishing", "recovery", "damaged"}

AmbiguousTags == {"indeterminate", "published-error"}
RecoveredTags ==
    {"recovered-predecessor-after-ambiguity",
     "recovered-successor-after-ambiguity", "recovered"}
SelectedTags == RecoveredTags \cup {"restarted", "restarted-mid"}
Tags ==
    {"init", "begun", "ready", "refused", "dnp", "recovery-refused",
     "restart-refused", "replaced", "repaired"}
    \cup AmbiguousTags \cup SelectedTags

IdleRun == P!InitialRun("trusted", "trusted", "exact", "sealed", "ok", "tight")

Scenarios ==
    {IdleRun,
     P!InitialRun("trusted", "trusted", "minus-one", "sealed", "ok", "tight"),
     P!InitialRun("trusted", "trusted", "exact", "changed", "ok", "tight"),
     P!InitialRun("trusted", "cold", "exact", "sealed", "ok", "tight"),
     P!InitialRun("trusted", "trusted", "exact", "sealed", "conflict", "tight")}

InitialHistory ==
    [durable |-> 0, log |-> <<>>, suffix |-> FALSE, core |-> 0,
     mode |-> "idle", pre |-> 0, pinned |-> NoGen, pinnedLog |-> <<>>,
     restarts |-> 0, decoy |-> FALSE, foreign |-> FALSE,
     atBegin |-> [log |-> <<>>, suffix |-> FALSE, durable |-> 0],
     last |-> "init"]

Init ==
    /\ run = IdleRun
    /\ h = InitialHistory

(***************************************************************************
A step begins only from an idle writer whose authority names the live
catalog's generation.
***************************************************************************)
Begin ==
    /\ h.mode = "idle"
    /\ h.core = h.durable
    /\ h.durable < MaxSteps
    /\ ~h.foreign
    /\ \E scenario \in Scenarios : run' = scenario
    /\ h' = [h EXCEPT !.mode = "publishing", !.pre = h.durable,
                      !.atBegin = [log |-> h.log, suffix |-> h.suffix,
                                   durable |-> h.durable],
                      !.last = "begun"]

Publish ==
    /\ h.mode = "publishing"
    /\ run.phase \notin P!Terminal
    /\ P!Next
    /\ UNCHANGED h

(***************************************************************************
Settling a terminal publication.  Ready adopts the successor.  A definite
non-publication keeps the predecessor authority; a run that appended leaves
a suffix, which the next publication truncates.  An indeterminate catalog
write appoints the successor only if the new image persisted, and an error
after the catalog file is durable appoints it; both leave the writer without
authority until recovery.
***************************************************************************)
Settle ==
    /\ h.mode = "publishing"
    /\ run.phase \in P!Terminal
    /\ run' = IdleRun
    /\ CASE run.outcome = "ready" ->
              h' = [h EXCEPT !.durable = h.pre + 1,
                             !.log = Append(h.log, h.pre + 1),
                             !.suffix = FALSE, !.core = h.pre + 1,
                             !.mode = "idle", !.foreign = FALSE,
                             !.last = "ready"]
         [] run.outcome = "definitely-not-published" ->
              h' = [h EXCEPT !.suffix = h.suffix \/ run.mutated,
                             !.core = h.pre, !.mode = "idle",
                             !.foreign = run.catalog = "other",
                             !.last = IF run.refusal # "none"
                                      THEN "refused" ELSE "dnp"]
         [] run.outcome = "indeterminate" /\ run.catalog = "new" ->
              h' = [h EXCEPT !.durable = h.pre + 1,
                             !.log = Append(h.log, h.pre + 1),
                             !.suffix = FALSE, !.core = NoGen,
                             !.mode = "recovery", !.last = "indeterminate"]
         [] run.outcome = "indeterminate" /\ run.catalog # "new" ->
              h' = [h EXCEPT !.suffix = TRUE, !.core = NoGen,
                             !.mode = "recovery", !.last = "indeterminate"]
         [] run.outcome = "published-error" ->
              h' = [h EXCEPT !.durable = h.pre + 1,
                             !.log = Append(h.log, h.pre + 1),
                             !.suffix = FALSE, !.core = NoGen,
                             !.mode = IF run.catalog = "other"
                                      THEN "damaged" ELSE "recovery",
                             !.foreign = run.catalog = "other",
                             !.last = "published-error"]

RecoveredTag ==
    IF h.last \in AmbiguousTags
    THEN IF h.durable = h.pre + 1
         THEN "recovered-successor-after-ambiguity"
         ELSE "recovered-predecessor-after-ambiguity"
    ELSE "recovered"

(* Held-lease recovery reads the catalog through the pinned directory handle
   and adopts the generation it selects. *)
Recover ==
    /\ h.mode = "recovery"
    /\ ~h.foreign
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.core = h.durable, !.mode = "idle",
                      !.last = RecoveredTag]

RecoverRefused ==
    /\ h.mode = "recovery"
    /\ h.foreign
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.mode = "damaged", !.last = "recovery-refused"]

ReplaceCatalog ==
    /\ h.mode \in {"idle", "recovery"}
    /\ ~h.foreign
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.foreign = TRUE, !.last = "replaced"]

(* The environment restores the image this writer last wrote. *)
Repair ==
    /\ h.foreign
    /\ h.mode \in {"idle", "recovery", "damaged"}
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.foreign = FALSE,
                      !.mode = IF h.mode = "damaged" THEN "recovery"
                               ELSE h.mode,
                      !.last = "repaired"]

(***************************************************************************
A restart drops the writer and its readers, reopens the store by path (the
designed boundary, so never under a decoy) and cold-admits the live catalog.
A publication abandoned at the slot write leaves the old, a torn or the new
image.
***************************************************************************)
RestartImages ==
    CASE h.mode = "publishing" /\ run.phase = "SlotWriting" ->
           {"old", "torn", "new"}
      [] h.mode = "publishing" /\ run.phase # "SlotWriting" -> {run.catalog}
      [] h.mode # "publishing" /\ h.foreign -> {"other"}
      [] h.mode # "publishing" /\ ~h.foreign -> {"old"}

RestartFrom(image) ==
    LET publishing == h.mode = "publishing"
        written == IF publishing /\
                      (image = "new" \/
                       (image = "other" /\ run.catalogFileDurable))
                   THEN h.pre + 1 ELSE h.durable
        appended == publishing /\ written = h.pre + 1
    IN h' = [h EXCEPT
               !.durable = written,
               !.log = IF appended THEN Append(h.log, h.pre + 1) ELSE h.log,
               !.suffix = IF appended THEN FALSE
                          ELSE h.suffix \/ (publishing /\ run.mutated),
               !.foreign = image = "other",
               !.core = IF image = "other" THEN NoGen ELSE written,
               !.mode = IF image = "other" THEN "damaged" ELSE "idle",
               !.pinned = NoGen, !.pinnedLog = <<>>,
               !.restarts = h.restarts + 1,
               !.last = CASE image = "other" -> "restart-refused"
                          [] image # "other" /\ publishing -> "restarted-mid"
                          [] image # "other" /\ ~publishing -> "restarted"]

Restart ==
    /\ h.restarts < MaxRestarts
    /\ ~h.decoy
    /\ run' = IdleRun
    /\ \E image \in RestartImages : RestartFrom(image)

PinReader ==
    /\ h.mode = "idle"
    /\ h.pinned = NoGen
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.pinned = h.core, !.pinnedLog = h.log]

UnpinReader ==
    /\ h.mode \in {"idle", "recovery", "damaged"}
    /\ h.pinned # NoGen
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.pinned = NoGen, !.pinnedLog = <<>>]

PlaceDecoy ==
    /\ h.mode \in {"idle", "recovery"}
    /\ ~h.decoy
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.decoy = TRUE]

RemoveDecoy ==
    /\ h.mode \in {"idle", "recovery"}
    /\ h.decoy
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.decoy = FALSE]

Next ==
    \/ Begin \/ Publish \/ Settle
    \/ Recover \/ RecoverRefused \/ ReplaceCatalog \/ Repair \/ Restart
    \/ PinReader \/ UnpinReader \/ PlaceDecoy \/ RemoveDecoy

Spec == Init /\ [][Next]_vars

(***************************************************************************
Invariants of the base configuration.
***************************************************************************)
HistoryTypeInvariant ==
    /\ P!TrustedPublicationTypeInvariant
    /\ h \in [durable : Generations, log : Seq(CommitIds), suffix : BOOLEAN,
              core : Cores, mode : HistoryModes, pre : Generations,
              pinned : Cores, pinnedLog : Seq(CommitIds),
              restarts : 0..MaxRestarts, decoy : BOOLEAN, foreign : BOOLEAN,
              atBegin : [log : Seq(CommitIds), suffix : BOOLEAN,
                         durable : Generations],
              last : Tags]

(* An idle writer's authority names exactly the generation the live catalog
   appoints. *)
HistorySoleAuthority == h.mode = "idle" => h.core = h.durable

(* Recovery and restart adopt the catalog's selection: the predecessor or the
   successor of the last attempt, never a suffix commit or a decoy's
   generation. *)
HistoryAuthorityIsCatalogSelected ==
    h.last \in SelectedTags =>
      /\ h.core = h.durable
      /\ h.durable \in {h.pre, h.pre + 1}

HistoryRecoveryRequiredAfterAmbiguity ==
    h.last \in AmbiguousTags =>
      /\ h.mode \in {"recovery", "damaged"}
      /\ h.core = NoGen

HistoryAppointedPrefixIsCommitted == h.log = [i \in 1..h.durable |-> i]

HistoryCommitsAtMostOnce ==
    \A i, j \in 1..Len(h.log) : i # j => h.log[i] # h.log[j]

HistoryPinnedPrefixStable ==
    h.pinned # NoGen =>
      /\ h.pinned <= Len(h.log)
      /\ SubSeq(h.log, 1, h.pinned) = h.pinnedLog

HistoryRefusalPreservesFiles ==
    h.last = "refused" =>
      [log |-> h.log, suffix |-> h.suffix, durable |-> h.durable] = h.atBegin

RunOutcomeBoundaryIsExact == P!TrustedOutcomeBoundaryIsExact

RunAdoptedSuccessorIsExactCatalog == P!AdoptedSuccessorIsExactCatalog

(***************************************************************************
Non-vacuity witnesses.  Each must be violated by the faithful specification.
***************************************************************************)
NeverRecoveredSuccessorAfterAmbiguity ==
    h.last # "recovered-successor-after-ambiguity"

NeverRecoveredPredecessorAfterAmbiguity ==
    h.last # "recovered-predecessor-after-ambiguity"

NeverRetriedAfterSuffix == ~(h.last = "ready" /\ h.atBegin.suffix)

NeverReaderAcrossFault ==
    ~(h.pinned # NoGen /\ h.last \in {"dnp"} \cup AmbiguousTags)

NeverRestartedMidPublication == h.last # "restarted-mid"

NeverRecoveredUnderDecoy == ~(h.decoy /\ h.last \in RecoveredTags)

NeverRefusedWithSuffix == ~(h.last = "refused" /\ h.suffix)

NeverRepairedForeignCatalog == h.last # "repaired"

(***************************************************************************
Negative control: an indeterminate catalog write whose image stayed old or
torn adopts the prepared successor.
***************************************************************************)
AdoptPreparedOnIndeterminate ==
    /\ h.mode = "publishing"
    /\ run.phase \in P!Terminal
    /\ run.outcome = "indeterminate"
    /\ run.catalog \in {"old", "torn"}
    /\ run' = IdleRun
    /\ h' = [h EXCEPT !.suffix = TRUE, !.core = h.pre + 1, !.mode = "idle",
                      !.last = "indeterminate"]

AdoptPreparedOnIndeterminateMutantSpec ==
    Init /\ [][Next \/ AdoptPreparedOnIndeterminate]_vars

(***************************************************************************
Negative control: recovery replays the synced commit left beyond the
appointed tail.
***************************************************************************)
RecoverReplayingSuffix ==
    /\ h.mode = "recovery"
    /\ ~h.foreign
    /\ h.suffix
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.core = h.durable + 1, !.mode = "idle",
                      !.last = RecoveredTag]

SuffixReplayMutantSpec == Init /\ [][Next \/ RecoverReplayingSuffix]_vars

(***************************************************************************
Negative control: recovery reopens the store by path and, under a decoy,
adopts the stale copy's generation.
***************************************************************************)
RecoverThroughPathname ==
    /\ h.mode = "recovery"
    /\ ~h.foreign
    /\ h.decoy
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.core = h.pre, !.mode = "idle", !.last = RecoveredTag]

PathnameRecoveryMutantSpec == Init /\ [][Next \/ RecoverThroughPathname]_vars

(***************************************************************************
Negative control: adoption re-reads the catalog by path, where a decoy holds
the expected new image while the live catalog holds another.
***************************************************************************)
AdoptThroughPathname ==
    /\ h.mode = "publishing"
    /\ h.decoy
    /\ run.phase = "Resolved"
    /\ run.callMode = "trusted"
    /\ run.catalog = "other"
    /\ run' = [run EXCEPT !.phase = "Ready", !.adopted = "new",
                          !.outcome = "ready"]
    /\ UNCHANGED h

PathnameRereadMutantSpec == Init /\ [][Next \/ AdoptThroughPathname]_vars

(***************************************************************************
Negative control: an ambiguous outcome hands the predecessor authority back
to the writer.
***************************************************************************)
SettleKeepingStaleCore ==
    /\ h.mode = "publishing"
    /\ run.phase \in P!Terminal
    /\ run.outcome \in AmbiguousTags
    /\ run' = IdleRun
    /\ h' = [h EXCEPT !.core = h.pre, !.mode = "idle", !.last = run.outcome]

StaleWriterRestorationMutantSpec ==
    Init /\ [][Next \/ SettleKeepingStaleCore]_vars

(***************************************************************************
Negative control: a publication that began over a suffix appends after it
instead of truncating it.
***************************************************************************)
SettleAfterOrphan ==
    /\ h.mode = "publishing"
    /\ run.phase \in P!Terminal
    /\ run.outcome = "ready"
    /\ h.atBegin.suffix
    /\ run' = IdleRun
    /\ h' = [h EXCEPT !.durable = h.pre + 1,
                      !.log = Append(Append(h.log, Orphan), h.pre + 1),
                      !.suffix = FALSE, !.core = h.pre + 1, !.mode = "idle",
                      !.last = "ready"]

AppendWithoutTruncationMutantSpec ==
    Init /\ [][Next \/ SettleAfterOrphan]_vars

(***************************************************************************
Negative control: after an ambiguous outcome that appointed the successor,
the writer retries from its stale authority without the base check and
commits the same transaction again.
***************************************************************************)
RetryFromStaleCore ==
    /\ h.mode = "recovery"
    /\ h.last \in AmbiguousTags
    /\ h.durable = h.pre + 1
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.log = Append(h.log, h.pre + 1), !.core = h.durable,
                      !.mode = "idle", !.last = "ready"]

StaleCoreRetryMutantSpec == Init /\ [][Next \/ RetryFromStaleCore]_vars

(***************************************************************************
Negative control: truncating a suffix also cuts the last appointed commit.
***************************************************************************)
TruncateCommittedEntry ==
    /\ h.mode = "idle"
    /\ h.suffix
    /\ Len(h.log) > 0
    /\ UNCHANGED run
    /\ h' = [h EXCEPT !.log = SubSeq(h.log, 1, Len(h.log) - 1),
                      !.suffix = FALSE]

TruncateBelowAppointedMutantSpec ==
    Init /\ [][Next \/ TruncateCommittedEntry]_vars

(***************************************************************************
Negative control: the suffix is truncated before the preflight, so a refused
publication changes the files.
***************************************************************************)
SettleRefusalAfterTruncation ==
    /\ h.mode = "publishing"
    /\ run.phase \in P!Terminal
    /\ run.refusal # "none"
    /\ h.suffix
    /\ run' = IdleRun
    /\ h' = [h EXCEPT !.suffix = FALSE, !.core = h.pre, !.mode = "idle",
                      !.last = "refused"]

TruncateOnRefusalMutantSpec ==
    Init /\ [][Next \/ SettleRefusalAfterTruncation]_vars

(***************************************************************************
Negative control: a failed catalog-slot write whose new image persisted is
reported as definitely not published.
***************************************************************************)
MisclassifyCatalogWrite ==
    /\ h.mode = "publishing"
    /\ run.phase = "SlotWriting"
    /\ run' = [run EXCEPT !.phase = "Failed", !.catalog = "new",
                          !.failure = "slot-write",
                          !.outcome = "definitely-not-published"]
    /\ UNCHANGED h

MisclassifiedCatalogWriteMutantSpec ==
    Init /\ [][Next \/ MisclassifyCatalogWrite]_vars

=============================================================================

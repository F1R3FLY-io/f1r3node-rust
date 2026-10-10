----------------------------- MODULE ChromaMirror -----------------------------
(***************************************************************************)
(* The ChromaDB mirror of a VectorDB space (P6, design v3 section 4.9;    *)
(* plan decisions 52, 58, 61, 66 (M1), 67 and 68).                         *)
(*                                                                         *)
(* One node plays deploys on a mirrored VectorDB space. The records of the *)
(* space are consensus state: each deploy maps the record set of its      *)
(* pre-state to its post-state, and its block holds the post-state. The    *)
(* mirror of the node is a copy of the records outside consensus. A deploy *)
(* may call ann: play asks the mirror (an advisory answer), records the    *)
(* answer, and the exact engine checks the answer against the state. The  *)
(* deploy then consumes the first stored record that the answer names.    *)
(*                                                                         *)
(* The mirror path: each evaluation keeps a journal of net writes. A final *)
(* result without errors publishes the journal (M1); any other result     *)
(* drops it, and the clear at the start of each root evaluation drops the *)
(* journal of an evaluation that no result settled (an exploratory        *)
(* evaluation). Publication never waits: a full queue marks the           *)
(* collection dirty. The writer applies each batch in order (registration, *)
(* upserts, deletes). A batch that still fails after its attempts marks   *)
(* the collection dirty, and it can leave a part of its writes applied.   *)
(* Blocks finalize in order. A reconciler pass for the newest finalized   *)
(* block repairs each registered channel of a collection whose watermark  *)
(* is below the block, or that is dirty, to the finalized view: orphans   *)
(* deleted, missing records added. A pass that finishes sets the          *)
(* watermark to max(watermark, block); a failed pass leaves the watermark *)
(* and marks a dirty collection dirty again.                               *)
(*                                                                         *)
(* Another node replays each deploy of the chain with a mirror of its own  *)
(* (any state). Replay uses the recorded answer of ann and never asks its *)
(* mirror, and it makes the deterministic checks of ann as play does      *)
(* (decision 67).                                                          *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   ConsensusIndependentOfMirror  the post-state of each deploy is a       *)
(*                                 function of its pre-state and its        *)
(*                                 recorded answer only.                    *)
(*   AdvisoryRecorded              replay uses the recorded answer of ann.  *)
(*   EqualHostWork                 play and replay reserve the same host    *)
(*                                 work for the checks of ann (decision     *)
(*                                 67).                                     *)
(*   PublishOnlyOnSuccess          every published write comes from a       *)
(*                                 deploy whose final result has no errors  *)
(*                                 (M1).                                    *)
(*   NetWritesExact                a published batch upserts exactly the    *)
(*                                 records that its deploy added and        *)
(*                                 deletes exactly the records that it      *)
(*                                 removed (a Rule E pair writes nothing).  *)
(*   PublishNeverWaits             a publication never waits for the        *)
(*                                 writer.                                  *)
(*   ReconciledEqualsFinalizedView after a pass that finishes, the mirror   *)
(*                                 holds the finalized view on every        *)
(*                                 registered channel.                      *)
(*   WatermarkHonest               the watermark never passes the block of  *)
(*                                 the last pass that finished.             *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant finds no       *)
(* violation:                                                              *)
(*   MirrorWriteInTxn = TRUE     MirrorWriteInTxnUnsafe refutes            *)
(*                               ConsensusIndependentOfMirror: the deploy  *)
(*                               writes the mirror inside its transaction, *)
(*                               and a failed write aborts it.             *)
(*   UnrecordedAdvisory = TRUE   UnrecordedAdvisoryUnsafe refutes          *)
(*                               AdvisoryRecorded: replay asks its own     *)
(*                               mirror.                                   *)
(*   ChecksOnlyInPlay = TRUE     AnnChecksOnlyInPlayUnsafe refutes         *)
(*                               EqualHostWork: replay returns the         *)
(*                               recorded answer before the checks.        *)
(*   PublishFailed = TRUE        PublishFailedDeployUnsafe refutes         *)
(*                               PublishOnlyOnSuccess.                     *)
(*   KeepJournal = TRUE          BeginScopeKeepsJournalUnsafe refutes      *)
(*                               PublishOnlyOnSuccess: the writes of an    *)
(*                               exploratory evaluation leak into the next *)
(*                               published deploy.                         *)
(*   RuleEUpsert = TRUE          RuleEUpsertUnsafe refutes NetWritesExact. *)
(*   BlockingPublish = TRUE      BlockingPublishUnsafe refutes             *)
(*                               PublishNeverWaits.                        *)
(*   KeepOrphans = TRUE          OrphansKeptUnsafe refutes                 *)
(*                               ReconciledEqualsFinalizedView.            *)
(*   WatermarkOnFailure = TRUE   WatermarkAfterFailureUnsafe refutes       *)
(*                               WatermarkHonest.                          *)
(***************************************************************************)
\* Code (rholang/src/rust/interpreter; the line numbers are those of the
\* commit of step 8d):
\*   Post                  disciplines/vectordb.rs:881-952 (eligible: the exact engine and its ids= filter)
\*   NetUpserts, NetDeletes, Upserts
\*                         spaces/mirror.rs:236-313 (MirrorCapture::writes: the first and the last event)
\*   journal               spaces/mirror.rs:408-426, 429-457 (MirrorJournal::record, take)
\*                         reified/runtime.rs:302-304 (a space job records its net writes)
\*   CheckWork             reified/system_processes.rs:541-586 (ann_request: the checks of decision 67)
\*   Deploy (M1)           reified/runtime.rs:315-320 (settle_mirror_journal), interpreter.rs:257, rho_runtime.rs:1808
\*   Explore               reified/runtime.rs:102-118 (begin_evaluation_scope clears the journal)
\*   Publish               reified/vectordb_services.rs:486-499 (MirrorWriter::publish: try_send, never waits)
\*   Apply                 reified/vectordb_services.rs:365-381 (apply), 455-482 (the writer task: attempts, dirty)
\*   Reconcile             reified/mirror_reconciler.rs:139-212 (reconcile), 216-230 (prepare_dirty), 235-248 (remark)
\*                         reified/mirror_reconciler.rs:253-296, 300-344 (repair, channel_repair)
\*   Replay                reified/system_processes.rs:485-511 (ann: the checks, then the recorded reply in replay)
\*   (the answer of play)  reified/system_processes.rs:641-686, 691-703 (ann_answer, produce_recorded)

EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Ids,                 \* the record ids (the odd ids are on channel c1, the even on c2)
    MaxDeploys,          \* the deploys of a run
    MaxEvents,           \* the steps of a run
    QCap,                \* the capacity of the queue of the writer
    MirrorWriteInTxn, UnrecordedAdvisory, ChecksOnlyInPlay, PublishFailed, KeepJournal,
    RuleEUpsert, BlockingPublish, KeepOrphans, WatermarkOnFailure

ASSUME Ids \subseteq 1..4 /\ Ids # {}
ASSUME MaxDeploys \in 1..6 /\ MaxEvents \in 1..20 /\ QCap \in 1..3
ASSUME {MirrorWriteInTxn, UnrecordedAdvisory, ChecksOnlyInPlay, PublishFailed, KeepJournal,
        RuleEUpsert, BlockingPublish, KeepOrphans, WatermarkOnFailure} \subseteq BOOLEAN

Chan(i) == IF i % 2 = 1 THEN "c1" ELSE "c2"
Chans == {Chan(i) : i \in Ids}

VARIABLES
    recs,        \* the records of the newest state of the node (consensus)
    chain,       \* the post-states of the blocks, in order
    final,       \* the number of finalized blocks
    deploys,     \* the deploys of the chain: pre-state, answer, post-state
    mirror,      \* the record ids that the mirror of the node holds
    registered,  \* the channels that the collection metadata registers
    queue,       \* the batches of the writer, in order
    dirty,       \* the collection misses writes
    dirtyChans,  \* the channels that the repair of a dirty collection needs
    wm,          \* the watermark of the collection
    repairedAt,  \* the block of the last pass that finished (0: none)
    journal,     \* the journal of the open evaluation: its writes and their origins
    events,      \* the steps so far
    ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk

vars == <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
          repairedAt, journal, events, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>

-----------------------------------------------------------------------------
(* Deploys *)

\* The operations of a deploy: add a record, remove one, peek one (Rule E:
\* removed and stored again), store and remove one in the same deploy, or
\* call ann and consume the first stored record of the answer.
Ops == {[kind |-> k, id |-> i] : k \in {"add", "remove", "peek", "transient"}, i \in Ids}
       \cup {[kind |-> "ann", id |-> 0]}

\* The exact engine: the post-state for the pre-state R and the answer A.
Post(op, R, A) ==
    CASE op.kind = "add" -> R \cup {op.id}
      [] op.kind = "remove" -> R \ {op.id}
      [] op.kind = "peek" -> R
      [] op.kind = "transient" -> R
      [] op.kind = "ann" ->
           LET hits == A \cap R IN
           IF hits = {} THEN R ELSE R \ {CHOOSE h \in hits : \A g \in hits : h <= g}

\* The events of the deploy on each record id, as the capture sees them:
\* the first and the last event decide the net write (P6 step 7b).
NetUpserts(op, R, A) == Post(op, R, A) \ R
NetDeletes(op, R, A) == R \ Post(op, R, A)
\* RuleEUpsert: a peek upserts the record that it stored again.
Upserts(op, R, A) ==
    NetUpserts(op, R, A) \cup (IF RuleEUpsert /\ op.kind = "peek" /\ op.id \in R THEN {op.id} ELSE {})

\* The checks of ann (decision 67): the host work that they reserve.
CheckWork(op) == IF op.kind = "ann" THEN 1 ELSE 0

Batch(ups, dels, origin) ==
    [ups |-> ups, dels |-> dels, chans |-> {Chan(i) : i \in ups \cup dels}, origin |-> origin]

-----------------------------------------------------------------------------
(* Actions *)

Init ==
    /\ recs = {}
    /\ chain = << >>
    /\ final = 0
    /\ deploys = << >>
    /\ mirror = {}
    /\ registered = {}
    /\ queue = << >>
    /\ dirty = FALSE
    /\ dirtyChans = {}
    /\ wm = 0
    /\ repairedAt = 0
    /\ journal = {}
    /\ events = 0
    /\ ciOk = TRUE /\ arOk = TRUE /\ hwOk = TRUE /\ pubOk = TRUE /\ netOk = TRUE
    /\ waitOk = TRUE /\ rOk = TRUE

\* Publication of the batches bs: it never waits, and a full queue marks
\* the collection dirty. Under BlockingPublish a full queue makes the
\* publication wait.
Publish(bs) ==
    IF bs = {} THEN /\ UNCHANGED <<queue, dirty, dirtyChans, waitOk>>
    ELSE LET b == CHOOSE b \in bs : TRUE IN
      IF Len(queue) < QCap
        THEN /\ queue' = Append(queue, b)
             /\ UNCHANGED <<dirty, dirtyChans, waitOk>>
        ELSE /\ queue' = queue
             /\ IF BlockingPublish
                  THEN /\ waitOk' = FALSE /\ UNCHANGED <<dirty, dirtyChans>>
                  ELSE /\ dirty' = TRUE /\ dirtyChans' = dirtyChans \cup b.chans
                       /\ UNCHANGED waitOk

\* A deploy that the node plays. ok: its final result has no errors. down:
\* the mirror is down during the deploy (it only matters under
\* MirrorWriteInTxn, where a failed mirror write aborts the deploy).
Deploy ==
    /\ events < MaxEvents /\ Len(deploys) < MaxDeploys
    /\ \E op \in Ops, ok \in BOOLEAN, down \in BOOLEAN :
         LET A == IF op.kind = "ann" THEN mirror ELSE {}
             post0 == Post(op, recs, A)
             aborted == MirrorWriteInTxn /\ down /\ post0 # recs
             committed == ok /\ ~aborted
             post == IF committed THEN post0 ELSE recs
             d == [pre |-> recs, op |-> op, answer |-> A, post |-> post, work |-> CheckWork(op),
                   ok |-> committed]
             \* The journal of this evaluation: the writes of the deploy, and
             \* under KeepJournal the writes that an exploratory evaluation left.
             own == IF committed
                      THEN {[ups |-> Upserts(op, recs, A), dels |-> NetDeletes(op, recs, A), origin |-> "deploy"]}
                      ELSE {[ups |-> Upserts(op, recs, A), dels |-> NetDeletes(op, recs, A), origin |-> "failed"]}
             kept == IF KeepJournal THEN journal ELSE {}
             entries == own \cup kept
             settled == IF committed \/ PublishFailed THEN entries ELSE {}
             writes == {e \in settled : e.ups # {} \/ e.dels # {}}
             ups == UNION {e.ups : e \in writes}
             dels == UNION {e.dels : e \in writes}
             bs == IF writes = {} THEN {}
                   ELSE {Batch(ups, dels, IF \A e \in writes : e.origin = "deploy" THEN "deploy" ELSE "other")}
         IN /\ recs' = post
            /\ chain' = Append(chain, post)
            /\ deploys' = Append(deploys, d)
            /\ journal' = {}
            /\ ciOk' = (ciOk /\ post = IF ok THEN Post(op, recs, A) ELSE recs)
            /\ pubOk' = (pubOk /\ \A b \in bs : b.origin = "deploy")
            /\ netOk' = (netOk /\ \A b \in bs :
                           b.origin # "deploy" \/ (b.ups = NetUpserts(op, recs, A) /\ b.dels = NetDeletes(op, recs, A)))
            /\ Publish(bs)
    /\ events' = events + 1
    /\ UNCHANGED <<final, mirror, registered, wm, repairedAt, arOk, hwOk, rOk>>

\* An exploratory evaluation (RhoRuntime::inj, no settle): its journal stays
\* until the clear at the start of the next root evaluation.
Explore ==
    /\ events < MaxEvents
    /\ \E op \in Ops \ {o \in Ops : o.kind = "ann"} :
         journal' = journal \cup {[ups |-> Upserts(op, recs, {}), dels |-> NetDeletes(op, recs, {}), origin |-> "explore"]}
    /\ events' = events + 1
    /\ UNCHANGED <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
                   repairedAt, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>

\* The writer applies the first batch: in full, or it fails after its
\* attempts with a part applied (registration and upserts, no deletes), and
\* the collection becomes dirty.
Apply ==
    /\ events < MaxEvents
    /\ queue # << >>
    /\ LET b == Head(queue) IN
       \E fails \in BOOLEAN :
         IF ~fails
           THEN /\ mirror' = (mirror \cup b.ups) \ b.dels
                /\ registered' = registered \cup b.chans
                /\ UNCHANGED <<dirty, dirtyChans>>
           ELSE \E partial \in BOOLEAN :
                  /\ mirror' = IF partial THEN mirror \cup b.ups ELSE mirror
                  /\ registered' = IF partial THEN registered \cup b.chans ELSE registered
                  /\ dirty' = TRUE
                  /\ dirtyChans' = dirtyChans \cup b.chans
    /\ queue' = Tail(queue)
    /\ events' = events + 1
    /\ UNCHANGED <<recs, chain, final, deploys, wm, repairedAt, journal, ciOk, arOk, hwOk, pubOk,
                   netOk, waitOk, rOk>>

Finalize ==
    /\ events < MaxEvents
    /\ final < Len(chain)
    /\ final' = final + 1
    /\ events' = events + 1
    /\ UNCHANGED <<recs, chain, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
                   repairedAt, journal, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>

OnChans(S, cs) == {i \in S : Chan(i) \in cs}

\* A reconciler pass for the newest finalized block. The dirty collection is
\* registered first (prepare_dirty). A pass may fail; a failed pass leaves
\* the watermark (WatermarkOnFailure: it sets it) and keeps the dirty mark.
Reconcile ==
    /\ events < MaxEvents
    /\ final > 0
    /\ wm < final \/ dirty
    /\ LET view == chain[final]
           cs == registered \cup dirtyChans
           \* The repaired mirror: orphans deleted (KeepOrphans: kept) and
           \* missing records added, on every registered channel.
           repaired == (mirror \ (IF KeepOrphans THEN {} ELSE OnChans(mirror, cs) \ view))
                       \cup OnChans(view, cs)
       IN \E fails \in BOOLEAN :
            IF ~fails
              THEN /\ mirror' = repaired
                   /\ registered' = cs
                   /\ wm' = IF final > wm THEN final ELSE wm
                   /\ repairedAt' = final
                   /\ dirty' = FALSE
                   /\ dirtyChans' = {}
                   /\ rOk' = (rOk /\ OnChans(repaired, cs) = OnChans(view, cs))
              ELSE /\ registered' = cs
                   /\ wm' = IF WatermarkOnFailure /\ final > wm THEN final ELSE wm
                   /\ UNCHANGED <<mirror, repairedAt, dirty, dirtyChans, rOk>>
    /\ events' = events + 1
    /\ UNCHANGED <<recs, chain, final, deploys, queue, journal, ciOk, arOk, hwOk, pubOk, netOk,
                   waitOk>>

\* Another node replays deploy k with a mirror of its own (any state). It
\* uses the recorded answer (UnrecordedAdvisory: it asks its mirror), and it
\* makes the checks of ann (ChecksOnlyInPlay: it returns first).
Replay ==
    /\ events < MaxEvents
    /\ \E k \in 1..Len(deploys), other \in SUBSET Ids :
         LET d == deploys[k]
             A == IF UnrecordedAdvisory /\ d.op.kind = "ann" THEN other ELSE d.answer
             work == IF ChecksOnlyInPlay THEN 0 ELSE CheckWork(d.op)
         IN /\ arOk' = (arOk /\ A = d.answer)
            /\ hwOk' = (hwOk /\ work = d.work)
    /\ events' = events + 1
    /\ UNCHANGED <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
                   repairedAt, journal, ciOk, pubOk, netOk, waitOk, rOk>>

Next == Deploy \/ Explore \/ Apply \/ Finalize \/ Reconcile \/ Replay

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants *)

TypeOK ==
    /\ recs \subseteq Ids
    /\ chain \in Seq(SUBSET Ids)
    /\ final \in 0..Len(chain)
    /\ mirror \subseteq Ids
    /\ registered \subseteq Chans
    /\ Len(queue) <= QCap
    /\ dirty \in BOOLEAN
    /\ dirtyChans \subseteq Chans
    /\ wm \in Nat /\ repairedAt \in Nat
    /\ {ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk} \subseteq BOOLEAN

ConsensusIndependentOfMirror == ciOk
AdvisoryRecorded == arOk
EqualHostWork == hwOk
PublishOnlyOnSuccess == pubOk
NetWritesExact == netOk
PublishNeverWaits == waitOk
ReconciledEqualsFinalizedView == rOk
WatermarkHonest == wm <= repairedAt

=============================================================================

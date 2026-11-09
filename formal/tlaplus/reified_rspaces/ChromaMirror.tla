----------------------------- MODULE ChromaMirror -----------------------------
(***************************************************************************)
(* The ChromaDB mirror of a VectorDB space (P6, design v3 section 4.9;    *)
(* plan decisions 52, 58, 61, 66, 67, 68, 98, 104 and 114).                *)
(*                                                                         *)
(* One node plays deploys on a mirrored VectorDB space. The records of the *)
(* space are consensus state: each deploy maps the record set of its      *)
(* pre-state to its post-state, and its block holds the post-state. The    *)
(* mirror of the node is a copy of the records outside consensus. A deploy *)
(* may call ann: play asks the mirror (an advisory answer), records the    *)
(* answer, and the exact engine checks the answer against the state. The  *)
(* deploy then consumes the first stored record that the answer names.    *)
(*                                                                         *)
(* Changed by decision 98 (the oracle mode of DR-114 of the base): a play *)
(* publishes the net writes of each commit at the commit, whatever the    *)
(* final result of its deploy, as base play calls a service at its COMM.  *)
(* Nothing is undone when the deploy fails, or when its block is rejected *)
(* or orphaned. An exploratory evaluation (the dry run of the cost        *)
(* estimate) plays on the services of the node, as the base dry run      *)
(* does, so it publishes too. No replay publishes, as no base replay      *)
(* calls a service. Publication never waits. The writer applies each      *)
(* batch in order, the upserts and then the deletes.                      *)
(*                                                                         *)
(* Changed by decision 114 (the oracle mode of File I/O): the mirror is   *)
(* an oracle store, and nothing repairs it. A batch that finds the queue  *)
(* full is dropped. A batch that still fails after its attempts is        *)
(* dropped, and it can leave a part of its writes applied.                *)
(*                                                                         *)
(* Retired by decision 114: "a full queue marks the collection dirty. The *)
(* writer applies each batch in order (registration, upserts, deletes). A *)
(* batch that still fails after its attempts marks the collection dirty,  *)
(* and it can leave a part of its writes applied. Blocks finalize in      *)
(* order. A reconciler pass for the newest finalized block repairs each   *)
(* registered channel of a collection whose watermark is below the block, *)
(* or that is dirty, to the finalized view: orphans deleted, missing      *)
(* records added. A pass that finishes sets the watermark to              *)
(* max(watermark, block); a failed pass leaves the watermark and marks a  *)
(* dirty collection dirty again."                                         *)
(*                                                                         *)
(* Retired by decision 98: "each evaluation keeps a journal of net        *)
(* writes. A final result without errors publishes the journal (M1); any  *)
(* other result drops it, and the clear at the start of each root         *)
(* evaluation drops the journal of an evaluation that no result settled   *)
(* (an exploratory evaluation)." Native replay also published its journal *)
(* to the mirror of the node that replays (rule MR-3).                    *)
(*                                                                         *)
(* The node also replays each deploy of the chain. Replay delivers the    *)
(* recorded answer of ann and never asks a mirror. Changed by decision    *)
(* 104 (DR-114 of the base): replay makes no check against consensus      *)
(* state. Retired by decision 104: "and it makes the deterministic checks *)
(* of ann as play does (decision 67)."                                    *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   ConsensusIndependentOfMirror  the post-state of each deploy is a       *)
(*                                 function of its pre-state and its        *)
(*                                 recorded answer only.                    *)
(*   AdvisoryRecorded              replay uses the recorded answer of ann.  *)
(*   NetWritesExact                a published batch of a play upserts     *)
(*                                 exactly the records that its commit     *)
(*                                 added and deletes exactly the records    *)
(*                                 that it removed (a Rule E pair writes    *)
(*                                 nothing).                                *)
(*   PublishNeverWaits             a publication never waits for the        *)
(*                                 writer.                                  *)
(*   ReplayNeverPublishes          no replay publishes (decision 98, DR-114:*)
(*                                 "No replay calls a service").            *)
(*   OnePublicationPerCommit       a commit of a play publishes its writes  *)
(*                                 at most once (decision 98, DR-114: "the  *)
(*                                 bound is one call per play").            *)
(* Retired by decision 98:                                                 *)
(*   PublishOnlyOnSuccess          every published write comes from a       *)
(*                                 deploy whose final result has no errors  *)
(*                                 (M1).                                    *)
(* Retired by decision 104 (replay makes no check, and DR-131 removed the *)
(* host work):                                                             *)
(*   EqualHostWork                 play and replay reserve the same host    *)
(*                                 work for the checks of ann (decision     *)
(*                                 67).                                     *)
(* Retired by decision 114 (nothing repairs the mirror):                  *)
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
(*                               AdvisoryRecorded: replay asks a mirror.   *)
(*   RuleEUpsert = TRUE          RuleEUpsertUnsafe refutes NetWritesExact. *)
(*   BlockingPublish = TRUE      BlockingPublishUnsafe refutes             *)
(*                               PublishNeverWaits: a full queue makes the *)
(*                               publication wait.                         *)
(*   ReplayPublishes = TRUE      ReplayPublishesUnsafe refutes             *)
(*                               ReplayNeverPublishes: the retired settle  *)
(*                               of native replay (MR-3) publishes the     *)
(*                               writes of the replayed deploy.            *)
(*   SettleAlsoPublishes = TRUE  SettleAlsoPublishesUnsafe refutes         *)
(*                               OnePublicationPerCommit: the retired      *)
(*                               settle of M1 publishes the writes of a    *)
(*                               deploy without errors a second time.      *)
(* Retired by decision 98 (their invariant PublishOnlyOnSuccess is        *)
(* retired):                                                               *)
(*   PublishFailed = TRUE        PublishFailedDeployUnsafe refuted         *)
(*                               PublishOnlyOnSuccess.                     *)
(*   KeepJournal = TRUE          BeginScopeKeepsJournalUnsafe refuted      *)
(*                               PublishOnlyOnSuccess: the writes of an    *)
(*                               exploratory evaluation leaked into the    *)
(*                               next published deploy.                    *)
(* Retired by decision 104 (its invariant EqualHostWork is retired):      *)
(*   ChecksOnlyInPlay = TRUE     AnnChecksOnlyInPlayUnsafe refuted         *)
(*                               EqualHostWork: replay returned the        *)
(*                               recorded answer before the checks.        *)
(* Retired by decision 114 (their invariants are retired):                *)
(*   KeepOrphans = TRUE          OrphansKeptUnsafe refuted                 *)
(*                               ReconciledEqualsFinalizedView.            *)
(*   WatermarkOnFailure = TRUE   WatermarkAfterFailureUnsafe refuted       *)
(*                               WatermarkHonest.                          *)
(***************************************************************************)
\* Code (rholang/src/rust/interpreter):
\*   Post                  spaces/disciplines/vectordb.rs (eligible: the exact engine and its ids= filter)
\*   NetUpserts, NetDeletes, Upserts
\*                         spaces/mirror.rs (MirrorCapture::writes: the first and the last event)
\*   CommitBatch           reified/runtime.rs (ReifiedRuntime::run_op: a play publishes the net
\*                         writes of the commit before the commit releases its stripes, and native
\*                         replay drops them)
\*   Publish               reified/vectordb_services.rs (MirrorWriter::publish: try_send, never
\*                         waits, and a full queue drops the batch)
\*   Apply                 reified/vectordb_services.rs (apply, and the writer task: its attempts,
\*                         and a batch that fails after them is dropped)
\*   Replay                reified/system_processes.rs (ann: replay delivers the recorded reply)
\*   (the answer of play)  reified/system_processes.rs (ann: the checks of play and ann_answer)
\* Retired by decision 98:
\*   journal               spaces/mirror.rs (MirrorJournal::record, take)
\*   Deploy (M1)           reified/runtime.rs (settle_mirror_journal), interpreter.rs, rho_runtime.rs
\*   Explore               reified/runtime.rs (begin_evaluation_scope cleared the journal)
\* Retired by decision 104:
\*   CheckWork             reified/system_processes.rs (ann_request: the checks of decision 67, in
\*                         play and replay)
\* Retired by decision 114:
\*   Reconcile             reified/mirror_reconciler.rs (reconcile, prepare_dirty, remark, repair,
\*                         channel_repair)
\*   dirty, registered     reified/vectordb_services.rs (the dirty marks of the writer and the
\*                         channel registration in the collection metadata)

EXTENDS Naturals, Sequences, FiniteSets

\* Changed by decision 98: PublishFailed and KeepJournal are retired, and ReplayPublishes and
\* SettleAlsoPublishes are added. Changed by decision 104: ChecksOnlyInPlay is retired. Changed by
\* decision 114: KeepOrphans and WatermarkOnFailure are retired, and the record ids have no
\* channels.
\* CONSTANTS
\*     Ids,                 \* the record ids (the odd ids are on channel c1, the even on c2)
\*     MaxDeploys,          \* the deploys of a run
\*     MaxEvents,           \* the steps of a run
\*     QCap,                \* the capacity of the queue of the writer
\*     MirrorWriteInTxn, UnrecordedAdvisory, ChecksOnlyInPlay, PublishFailed, KeepJournal,
\*     RuleEUpsert, BlockingPublish, KeepOrphans, WatermarkOnFailure
CONSTANTS
    Ids,                 \* the record ids
    MaxDeploys,          \* the deploys of a run
    MaxEvents,           \* the steps of a run
    QCap,                \* the capacity of the queue of the writer
    MirrorWriteInTxn, UnrecordedAdvisory, RuleEUpsert, BlockingPublish,
    ReplayPublishes, SettleAlsoPublishes

ASSUME Ids \subseteq 1..4 /\ Ids # {}
ASSUME MaxDeploys \in 1..6 /\ MaxEvents \in 1..20 /\ QCap \in 1..3
\* Changed by decisions 98, 104 and 114 (the flags above).
\* ASSUME {MirrorWriteInTxn, UnrecordedAdvisory, ChecksOnlyInPlay, PublishFailed, KeepJournal,
\*         RuleEUpsert, BlockingPublish, KeepOrphans, WatermarkOnFailure} \subseteq BOOLEAN
ASSUME {MirrorWriteInTxn, UnrecordedAdvisory, RuleEUpsert, BlockingPublish,
        ReplayPublishes, SettleAlsoPublishes} \subseteq BOOLEAN

\* Retired by decision 114: only the channel registration, the dirty marks and the reconciler
\* read the channels.
\* Chan(i) == IF i % 2 = 1 THEN "c1" ELSE "c2"
\* Chans == {Chan(i) : i \in Ids}

\* Changed by decision 98: the journal and the flag pubOk are retired, and the flags rpOk and
\* onceOk are added. Changed by decision 104: the flag hwOk is retired. Changed by decision 114:
\* chain, final, registered, dirty, dirtyChans, wm, repairedAt and the flag rOk are retired with
\* the reconciler.
\* VARIABLES
\*     recs,        \* the records of the newest state of the node (consensus)
\*     chain,       \* the post-states of the blocks, in order
\*     final,       \* the number of finalized blocks
\*     deploys,     \* the deploys of the chain: pre-state, answer, post-state
\*     mirror,      \* the record ids that the mirror of the node holds
\*     registered,  \* the channels that the collection metadata registers
\*     queue,       \* the batches of the writer, in order
\*     dirty,       \* the collection misses writes
\*     dirtyChans,  \* the channels that the repair of a dirty collection needs
\*     wm,          \* the watermark of the collection
\*     repairedAt,  \* the block of the last pass that finished (0: none)
\*     journal,     \* the journal of the open evaluation: its writes and their origins
\*     events,      \* the steps so far
\*     ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk
VARIABLES
    recs,        \* the records of the newest state of the node (consensus)
    deploys,     \* the deploys of the chain: pre-state, answer, post-state
    mirror,      \* the record ids that the mirror of the node holds
    queue,       \* the batches of the writer, in order
    events,      \* the steps so far
    ciOk, arOk, netOk, waitOk, rpOk, onceOk

\* Changed by decisions 98, 104 and 114 (the variables above).
\* vars == <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
\*           repairedAt, journal, events, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>
vars == <<recs, deploys, mirror, queue, events, ciOk, arOk, netOk, waitOk, rpOk, onceOk>>

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

\* Retired by decision 104: replay makes no check, and DR-131 removed the host work.
\* \* The checks of ann (decision 67): the host work that they reserve.
\* CheckWork(op) == IF op.kind = "ann" THEN 1 ELSE 0

\* Changed by decision 114: no channel registration and no dirty mark reads the channels of a
\* batch.
\* Batch(ups, dels, origin) ==
\*     [ups |-> ups, dels |-> dels, chans |-> {Chan(i) : i \in ups \cup dels}, origin |-> origin]
Batch(ups, dels, origin) == [ups |-> ups, dels |-> dels, origin |-> origin]

\* Added by decision 98: the batch of the net writes of one commit of the operation op on the
\* state R with the answer A. A commit without a net write publishes nothing.
CommitBatch(op, R, A, origin) ==
    LET ups == Upserts(op, R, A)
        dels == NetDeletes(op, R, A)
    IN IF ups = {} /\ dels = {} THEN {} ELSE {Batch(ups, dels, origin)}

\* Added by decision 98: the batches bs are the exact net writes of the commit of op.
Exact(bs, op, R, A) ==
    \A b \in bs : b.ups = NetUpserts(op, R, A) /\ b.dels = NetDeletes(op, R, A)

-----------------------------------------------------------------------------
(* Actions *)

\* Changed by decisions 98, 104 and 114 (the variables above).
\* Init ==
\*     /\ recs = {}
\*     /\ chain = << >>
\*     /\ final = 0
\*     /\ deploys = << >>
\*     /\ mirror = {}
\*     /\ registered = {}
\*     /\ queue = << >>
\*     /\ dirty = FALSE
\*     /\ dirtyChans = {}
\*     /\ wm = 0
\*     /\ repairedAt = 0
\*     /\ journal = {}
\*     /\ events = 0
\*     /\ ciOk = TRUE /\ arOk = TRUE /\ hwOk = TRUE /\ pubOk = TRUE /\ netOk = TRUE
\*     /\ waitOk = TRUE /\ rOk = TRUE
Init ==
    /\ recs = {}
    /\ deploys = << >>
    /\ mirror = {}
    /\ queue = << >>
    /\ events = 0
    /\ ciOk = TRUE /\ arOk = TRUE /\ netOk = TRUE
    /\ waitOk = TRUE /\ rpOk = TRUE /\ onceOk = TRUE

\* Changed by decision 98: under SettleAlsoPublishes one commit publishes its batch twice, so a
\* publication takes a sequence of batches (PubStep, PubSeq and AsSeq). Changed by decision 114:
\* a full queue drops the batch, and no dirty mark exists.
\* \* Publication of the batches bs: it never waits, and a full queue marks
\* \* the collection dirty. Under BlockingPublish a full queue makes the
\* \* publication wait.
\* Publish(bs) ==
\*     IF bs = {} THEN /\ UNCHANGED <<queue, dirty, dirtyChans, waitOk>>
\*     ELSE LET b == CHOOSE b \in bs : TRUE IN
\*       IF Len(queue) < QCap
\*         THEN /\ queue' = Append(queue, b)
\*              /\ UNCHANGED <<dirty, dirtyChans, waitOk>>
\*         ELSE /\ queue' = queue
\*              /\ IF BlockingPublish
\*                   THEN /\ waitOk' = FALSE /\ UNCHANGED <<dirty, dirtyChans>>
\*                   ELSE /\ dirty' = TRUE /\ dirtyChans' = dirtyChans \cup b.chans
\*                        /\ UNCHANGED waitOk

\* The publication of the batch b in the writer state st: it never waits, and a full queue drops
\* the batch (decision 114). Under BlockingPublish a full queue makes the publication wait.
PubStep(st, b) ==
    IF Len(st.queue) < QCap
      THEN [st EXCEPT !.queue = Append(st.queue, b)]
      ELSE IF BlockingPublish
             THEN [st EXCEPT !.waitOk = FALSE]
             ELSE st

RECURSIVE PubSeq(_, _)
PubSeq(st, bs) == IF bs = << >> THEN st ELSE PubSeq(PubStep(st, Head(bs)), Tail(bs))

\* The publication of the batches of the sequence bs, in order.
Publish(bs) ==
    LET st == PubSeq([queue |-> queue, waitOk |-> waitOk], bs)
    IN /\ queue' = st.queue
       /\ waitOk' = st.waitOk

\* The sequence of the batch of the set bs, which holds at most one batch.
AsSeq(bs) == IF bs = {} THEN << >> ELSE << CHOOSE b \in bs : TRUE >>

\* Changed by decision 98: the commit of the deploy publishes its net writes at the commit, so a
\* deploy with errors publishes them too. Changed by decision 104: a deploy keeps no check work.
\* Changed by decision 114: no block chain, because nothing repairs the mirror.
\* \* A deploy that the node plays. ok: its final result has no errors. down:
\* \* the mirror is down during the deploy (it only matters under
\* \* MirrorWriteInTxn, where a failed mirror write aborts the deploy).
\* Deploy ==
\*     /\ events < MaxEvents /\ Len(deploys) < MaxDeploys
\*     /\ \E op \in Ops, ok \in BOOLEAN, down \in BOOLEAN :
\*          LET A == IF op.kind = "ann" THEN mirror ELSE {}
\*              post0 == Post(op, recs, A)
\*              aborted == MirrorWriteInTxn /\ down /\ post0 # recs
\*              committed == ok /\ ~aborted
\*              post == IF committed THEN post0 ELSE recs
\*              d == [pre |-> recs, op |-> op, answer |-> A, post |-> post, work |-> CheckWork(op),
\*                    ok |-> committed]
\*              \* The journal of this evaluation: the writes of the deploy, and
\*              \* under KeepJournal the writes that an exploratory evaluation left.
\*              own == IF committed
\*                       THEN {[ups |-> Upserts(op, recs, A), dels |-> NetDeletes(op, recs, A), origin |-> "deploy"]}
\*                       ELSE {[ups |-> Upserts(op, recs, A), dels |-> NetDeletes(op, recs, A), origin |-> "failed"]}
\*              kept == IF KeepJournal THEN journal ELSE {}
\*              entries == own \cup kept
\*              settled == IF committed \/ PublishFailed THEN entries ELSE {}
\*              writes == {e \in settled : e.ups # {} \/ e.dels # {}}
\*              ups == UNION {e.ups : e \in writes}
\*              dels == UNION {e.dels : e \in writes}
\*              bs == IF writes = {} THEN {}
\*                    ELSE {Batch(ups, dels, IF \A e \in writes : e.origin = "deploy" THEN "deploy" ELSE "other")}
\*          IN /\ recs' = post
\*             /\ chain' = Append(chain, post)
\*             /\ deploys' = Append(deploys, d)
\*             /\ journal' = {}
\*             /\ ciOk' = (ciOk /\ post = IF ok THEN Post(op, recs, A) ELSE recs)
\*             /\ pubOk' = (pubOk /\ \A b \in bs : b.origin = "deploy")
\*             /\ netOk' = (netOk /\ \A b \in bs :
\*                            b.origin # "deploy" \/ (b.ups = NetUpserts(op, recs, A) /\ b.dels = NetDeletes(op, recs, A)))
\*             /\ Publish(bs)
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<final, mirror, registered, wm, repairedAt, arOk, hwOk, rOk>>
\* A deploy that the node plays. ok: its final result has no errors. down:
\* the mirror is down during the deploy (it only matters under
\* MirrorWriteInTxn, where a failed mirror write aborts the deploy). A
\* transaction that the mirror write aborts never commits, so it publishes
\* nothing. Under SettleAlsoPublishes the retired settle of M1 publishes the
\* writes of a deploy without errors a second time.
Deploy ==
    /\ events < MaxEvents /\ Len(deploys) < MaxDeploys
    /\ \E op \in Ops, ok \in BOOLEAN, down \in BOOLEAN :
         LET A == IF op.kind = "ann" THEN mirror ELSE {}
             post0 == Post(op, recs, A)
             aborted == MirrorWriteInTxn /\ down /\ post0 # recs
             committed == ok /\ ~aborted
             post == IF committed THEN post0 ELSE recs
             d == [pre |-> recs, op |-> op, answer |-> A, post |-> post, ok |-> committed]
             bs == IF aborted THEN {} ELSE CommitBatch(op, recs, A, "play")
             pubs == IF SettleAlsoPublishes /\ committed THEN AsSeq(bs) \o AsSeq(bs) ELSE AsSeq(bs)
         IN /\ recs' = post
            /\ deploys' = Append(deploys, d)
            /\ ciOk' = (ciOk /\ post = IF ok THEN Post(op, recs, A) ELSE recs)
            /\ netOk' = (netOk /\ Exact(bs, op, recs, A))
            /\ onceOk' = (onceOk /\ Len(pubs) <= 1)
            /\ Publish(pubs)
    /\ events' = events + 1
    /\ UNCHANGED <<mirror, arOk, rpOk>>

\* Changed by decision 98: the dry run plays on the services of the node, as the base dry run
\* does (spawn_offered_play_runtime), so its commit publishes its net writes.
\* \* An exploratory evaluation (RhoRuntime::inj, no settle): its journal stays
\* \* until the clear at the start of the next root evaluation.
\* Explore ==
\*     /\ events < MaxEvents
\*     /\ \E op \in Ops \ {o \in Ops : o.kind = "ann"} :
\*          journal' = journal \cup {[ups |-> Upserts(op, recs, {}), dels |-> NetDeletes(op, recs, {}), origin |-> "explore"]}
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
\*                    repairedAt, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>
\* An exploratory evaluation (the dry run of the cost estimate, RhoRuntime::inj). Its commit
\* publishes its net writes. It leaves no consensus state.
Explore ==
    /\ events < MaxEvents
    /\ \E op \in Ops \ {o \in Ops : o.kind = "ann"} :
         LET bs == CommitBatch(op, recs, {}, "play") IN
         /\ netOk' = (netOk /\ Exact(bs, op, recs, {}))
         /\ Publish(AsSeq(bs))
    /\ events' = events + 1
    /\ UNCHANGED <<recs, deploys, mirror, ciOk, arOk, rpOk, onceOk>>

\* Changed by decision 114: a batch that fails after its attempts is dropped. No registration
\* and no dirty mark exist.
\* \* The writer applies the first batch: in full, or it fails after its
\* \* attempts with a part applied (registration and upserts, no deletes), and
\* \* the collection becomes dirty.
\* Apply ==
\*     /\ events < MaxEvents
\*     /\ queue # << >>
\*     /\ LET b == Head(queue) IN
\*        \E fails \in BOOLEAN :
\*          IF ~fails
\*            THEN /\ mirror' = (mirror \cup b.ups) \ b.dels
\*                 /\ registered' = registered \cup b.chans
\*                 /\ UNCHANGED <<dirty, dirtyChans>>
\*            ELSE \E partial \in BOOLEAN :
\*                   /\ mirror' = IF partial THEN mirror \cup b.ups ELSE mirror
\*                   /\ registered' = IF partial THEN registered \cup b.chans ELSE registered
\*                   /\ dirty' = TRUE
\*                   /\ dirtyChans' = dirtyChans \cup b.chans
\*     /\ queue' = Tail(queue)
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<recs, chain, final, deploys, wm, repairedAt, journal, ciOk, arOk, hwOk, pubOk,
\*                    netOk, waitOk, rOk>>
\* The writer applies the first batch: in full, or it fails after its
\* attempts with a part applied (the upserts, no deletes), and the batch is
\* dropped.
Apply ==
    /\ events < MaxEvents
    /\ queue # << >>
    /\ LET b == Head(queue) IN
       \E fails \in BOOLEAN :
         IF ~fails
           THEN mirror' = (mirror \cup b.ups) \ b.dels
           ELSE \E partial \in BOOLEAN :
                  mirror' = IF partial THEN mirror \cup b.ups ELSE mirror
    /\ queue' = Tail(queue)
    /\ events' = events + 1
    /\ UNCHANGED <<recs, deploys, ciOk, arOk, netOk, waitOk, rpOk, onceOk>>

\* Retired by decision 114: only the reconciler read the finalized view.
\* Finalize ==
\*     /\ events < MaxEvents
\*     /\ final < Len(chain)
\*     /\ final' = final + 1
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<recs, chain, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
\*                    repairedAt, journal, ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk>>

\* Retired by decision 114: nothing repairs the mirror (the oracle mode of File I/O).
\* OnChans(S, cs) == {i \in S : Chan(i) \in cs}
\*
\* \* A reconciler pass for the newest finalized block. The dirty collection is
\* \* registered first (prepare_dirty). A pass may fail; a failed pass leaves
\* \* the watermark (WatermarkOnFailure: it sets it) and keeps the dirty mark.
\* Reconcile ==
\*     /\ events < MaxEvents
\*     /\ final > 0
\*     /\ wm < final \/ dirty
\*     /\ LET view == chain[final]
\*            cs == registered \cup dirtyChans
\*            \* The repaired mirror: orphans deleted (KeepOrphans: kept) and
\*            \* missing records added, on every registered channel.
\*            repaired == (mirror \ (IF KeepOrphans THEN {} ELSE OnChans(mirror, cs) \ view))
\*                        \cup OnChans(view, cs)
\*        IN \E fails \in BOOLEAN :
\*             IF ~fails
\*               THEN /\ mirror' = repaired
\*                    /\ registered' = cs
\*                    /\ wm' = IF final > wm THEN final ELSE wm
\*                    /\ repairedAt' = final
\*                    /\ dirty' = FALSE
\*                    /\ dirtyChans' = {}
\*                    /\ rOk' = (rOk /\ OnChans(repaired, cs) = OnChans(view, cs))
\*               ELSE /\ registered' = cs
\*                    /\ wm' = IF WatermarkOnFailure /\ final > wm THEN final ELSE wm
\*                    /\ UNCHANGED <<mirror, repairedAt, dirty, dirtyChans, rOk>>
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<recs, chain, final, deploys, queue, journal, ciOk, arOk, hwOk, pubOk, netOk,
\*                    waitOk>>

\* Changed by decision 98: no replay publishes, as no base replay calls a service (DR-114).
\* Changed by decision 104: replay makes no check against consensus state, as the base driver
\* (DR-114). Changed by decision 114: the replaying node is this node, and its mirror is the
\* only mirror of the model.
\* \* Another node replays deploy k with a mirror of its own (any state). It
\* \* uses the recorded answer (UnrecordedAdvisory: it asks its mirror), and it
\* \* makes the checks of ann (ChecksOnlyInPlay: it returns first).
\* Replay ==
\*     /\ events < MaxEvents
\*     /\ \E k \in 1..Len(deploys), other \in SUBSET Ids :
\*          LET d == deploys[k]
\*              A == IF UnrecordedAdvisory /\ d.op.kind = "ann" THEN other ELSE d.answer
\*              work == IF ChecksOnlyInPlay THEN 0 ELSE CheckWork(d.op)
\*          IN /\ arOk' = (arOk /\ A = d.answer)
\*             /\ hwOk' = (hwOk /\ work = d.work)
\*     /\ events' = events + 1
\*     /\ UNCHANGED <<recs, chain, final, deploys, mirror, registered, queue, dirty, dirtyChans, wm,
\*                    repairedAt, journal, ciOk, pubOk, netOk, waitOk, rOk>>
\* The node replays deploy k of the chain: the block of another proposer, or
\* its own block in certification. It uses the recorded answer
\* (UnrecordedAdvisory: it asks a mirror of its own). Under ReplayPublishes
\* the retired settle of native replay (MR-3) publishes the net writes of the
\* replayed deploy to the queue of the node.
Replay ==
    /\ events < MaxEvents
    /\ \E k \in 1..Len(deploys), other \in SUBSET Ids :
         LET d == deploys[k]
             A == IF UnrecordedAdvisory /\ d.op.kind = "ann" THEN other ELSE d.answer
             bs == IF ReplayPublishes /\ d.ok THEN CommitBatch(d.op, d.pre, d.answer, "replay") ELSE {}
         IN /\ arOk' = (arOk /\ A = d.answer)
            /\ rpOk' = (rpOk /\ bs = {})
            /\ Publish(AsSeq(bs))
    /\ events' = events + 1
    /\ UNCHANGED <<recs, deploys, mirror, ciOk, netOk, onceOk>>

\* Changed by decision 114: Finalize and Reconcile are retired.
\* Next == Deploy \/ Explore \/ Apply \/ Finalize \/ Reconcile \/ Replay
Next == Deploy \/ Explore \/ Apply \/ Replay

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants *)

\* Changed by decisions 98, 104 and 114 (the variables above).
\* TypeOK ==
\*     /\ recs \subseteq Ids
\*     /\ chain \in Seq(SUBSET Ids)
\*     /\ final \in 0..Len(chain)
\*     /\ mirror \subseteq Ids
\*     /\ registered \subseteq Chans
\*     /\ Len(queue) <= QCap
\*     /\ dirty \in BOOLEAN
\*     /\ dirtyChans \subseteq Chans
\*     /\ wm \in Nat /\ repairedAt \in Nat
\*     /\ {ciOk, arOk, hwOk, pubOk, netOk, waitOk, rOk} \subseteq BOOLEAN
TypeOK ==
    /\ recs \subseteq Ids
    /\ mirror \subseteq Ids
    /\ Len(queue) <= QCap
    /\ {ciOk, arOk, netOk, waitOk, rpOk, onceOk} \subseteq BOOLEAN

ConsensusIndependentOfMirror == ciOk
AdvisoryRecorded == arOk
\* Retired by decision 104: replay makes no check, and DR-131 removed the host work.
\* EqualHostWork == hwOk
\* Retired by decision 98 (DR-114): a play publishes at its commits whatever its final result.
\* PublishOnlyOnSuccess == pubOk
NetWritesExact == netOk
PublishNeverWaits == waitOk
\* Retired by decision 114: nothing repairs the mirror.
\* ReconciledEqualsFinalizedView == rOk
\* WatermarkHonest == wm <= repairedAt
\* Added by decision 98: the oracle mode of DR-114.
ReplayNeverPublishes == rpOk
OnePublicationPerCommit == onceOk

=============================================================================

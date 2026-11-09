-------------------------- MODULE CheckpointReplay --------------------------
(***************************************************************************)
(* Checkpoint and replay of Reified RSpaces substrate transactions (P1).   *)
(*                                                                         *)
(* The model composes a proposer (play) with a validator (replay) for one  *)
(* or two blocks. In each block, the proposer commits space transactions.  *)
(* Each stage step applies the HostSteps of one agent operation to the     *)
(* staged view: one HostStep, or two when a peek fire is followed by its   *)
(* re-store. TxnCommit commits the fold of the HostSteps under the play    *)
(* counter rule. At the end of the deploy, DeployEnd sweeps the volatile   *)
(* channels Vol. Then Checkpoint persists the hot store into the history   *)
(* trie. Rig binds the play log for the validator. The validator resets to *)
(* the root of the previous block and replays the same transactions. Each  *)
(* replay commit verifies and pairs every COMM against the recorded        *)
(* bindings, under the replay counter rule. Then the validator sweeps and  *)
(* checkpoints too.                                                        *)
(*                                                                         *)
(* Decision 36 (replay reproduces play). The deploy of a block has the     *)
(* phlo limit Budget. A committed operation charges its introduction, and  *)
(* a fire also charges its COMM. A charge that exceeds the limit is        *)
(* rejected, the attempt counts, so the spending equals the limit, and     *)
(* the deploy fails. The commit stops at that operation and keeps the      *)
(* operations before it (D6). Replay makes the same charges. A replay      *)
(* fire with no unpaired recorded occurrence stages unpaired: when its     *)
(* charges pass, the commit stops there with ReplayDivergence, after the   *)
(* operations before it. Staging computes where the commit stops (the      *)
(* first rejected charge or the first unpaired fire), and the staged view  *)
(* stops changing there. So the view at the stop is the prefix snapshot    *)
(* that the commit applies. The operations after the stop have no host     *)
(* effect, and their staging has none either.                              *)
(*                                                                         *)
(* A proposer can publish a log that omits the COMM of its last fire       *)
(* (Forge, risk R1 of the design). The validator then meets an unpaired    *)
(* fire whose charges pass, and replay fails closed.                       *)
(*                                                                         *)
(* Decision 45, Rule E (constant Peeks). An agent operation can be a peek. *)
(* A peek on a channel with a stored record fires. Its COMM removes the    *)
(* record, as the host COMM does, and the same transaction stores the      *)
(* same record again (the re-store), as the host reducer's produce_peeks   *)
(* sends it again. The re-store is a HostStep of its own: it has the       *)
(* identity of the record, it steps the counter of that identity, and it   *)
(* logs a produce event. A peek on a channel with no record parks a peek   *)
(* continuation. A produce that meets only a parked peek continuation      *)
(* fires it and then stores its datum again (the re-offer of decision 30,  *)
(* which is produce_peeks too). The agent fires a plain continuation       *)
(* before a peek continuation, so a re-store never meets a waiter.         *)
(*                                                                         *)
(* Changed by decision 90. DR-134 of feature/cost-accounted-rho deletes    *)
(* the legacy meter, so the merge of 702496775 removes Option P of         *)
(* decision 45. This module no longer has the legacy deploy (the constant  *)
(* Legacy), the chain of its charges or the ghost run of the host program. *)
(* U13 and the controls LegacyPreAfterObserve, LegacyTailDropped and       *)
(* RetainPeek go with them. Every deploy is a token deploy. Restore the    *)
(* Legacy mode with git show                                               *)
(* 6bf10a721:formal/tlaplus/reified_rspaces/CheckpointReplay.tla. Each     *)
(* retired configuration names its own restore command.                    *)
(*                                                                         *)
(* Invariants (T1-T6 are ids of the P1 map):                               *)
(*   T1 ReplayPrefixEqual     the replay log is a prefix of the play log,  *)
(*                            and equals it when the replay completes.     *)
(*   T2 SameRootIffSameState  two checkpoints have the same root exactly   *)
(*                            when they have the same hot store state.     *)
(*   T3 PairedOnce            replay pairs a COMM at most as often as the  *)
(*                            play log records it.                         *)
(*   T4 NoVolatileResidue     no history trie holds a volatile channel.    *)
(*   T6 ReplayOutcomeEqual    an honest block (no forged log) is never     *)
(*                            rejected, and its replayed deploy has the    *)
(*                            failure and the spending of play.            *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   UseUuid              identities carry a per-run nonce        -> T1    *)
(*   RootIgnoresVolatile  no sweep, the root skips Vol            -> T2    *)
(*   DoublePair           verify pairs under every bound key      -> T3    *)
(*   JoinResidue          the sweep keeps the join entries        -> T4    *)
(*   StagingDivergence    an unpaired replay fire fails staging   -> T6    *)
(*                        (the rule of decision 19)                        *)
(*   AcceptUnpaired       an unpaired replay fire commits         -> T1    *)
(*                                                                         *)
(* Inductive check (Apalache): IndInv implies T3 and T4. BASE checks Init  *)
(* => IndInv. STEP checks IndInv /\ Next => IndInv', from every state that *)
(* satisfies IndInv (IndInit). Thus T3 and T4 hold at every depth for the  *)
(* configured constants: a deploy with no peeks, and a deploy with peeks.  *)
(* Changed by decision 90: the checks with peeks ran in a legacy deploy    *)
(* before, and now they run in a token deploy. With DoublePair or          *)
(* JoinResidue, STEP must refute IndInv (negative controls). TLC also      *)
(* checks IndInv on every reachable state of the safe configurations. T1,  *)
(* T2 and T6 are checked by TLC on the full state space. The model claims  *)
(* no inductive form of them.                                              *)
(*                                                                         *)
(* Code at the merge of feature/cost-accounted-rho 4d67b5ccc. Before       *)
(* the merge, the code was HEAD 6f7a80da3 (rs = rspace++/src/rspace):      *)
(*   play produce/consume   rs/rspace/ops_produce.rs:32-248,               *)
(*                          rs/rspace/ops_consume.rs:31-202                *)
(*   play counter rule      rs/rspace/ops_produce.rs:185-191,              *)
(*                          rs/rspace/trace_log.rs:158-172                 *)
(*   replay counter rule    rs/replay_rspace.rs:1194, 1388, 1579-1600      *)
(*   rig, check_replay_data rs/replay_rspace.rs:480-558                    *)
(*   remove_bindings_for    rs/replay_rspace.rs:1498-1526                  *)
(*   checkpoint, reset      rs/rspace/ispace_impl.rs:60-114                *)
(*   hot store primitives   rs/hot_store.rs:720-783, 879-945, 1029-1125    *)
(*   failed deploy (play)   casper/src/rust/rholang/runtime.rs:934-958     *)
(*   status, cost (replay)  casper/src/rust/rholang/replay_runtime.rs:581-594 *)
(*   sweep, verify          p1-design-report §3.1, §2.4                    *)
(*   sweep guard (FR-9')    SpaceStoreHandle::sweep_volatile, HandleState  *)
(*   unpaired fire (D36)    TxnCore::verify, StagedFire.recorded,          *)
(*                          StagedConsumeFire.recorded, ReplayBackend::    *)
(*                          produce_fire and consume_observed              *)
(*                                                                         *)
(* Decision 45 at HEAD e9b082454 and its working tree of 2026-10-06. A     *)
(* path of rholang/src/rust/interpreter/ is given without that prefix,     *)
(* and the generic.rs lines are those of the working tree:                 *)
(*   peek removal (Rule E)  rs/rspace/ops_consume.rs:192-202               *)
(*                          (store_persistent_data removes every           *)
(*                          non-persistent candidate, peeks included)      *)
(*   re-store (Rule E)      spaces/generic.rs:1252-1267 (Cx::restore and    *)
(*                          Cx::restore_on), 1887-1896 (restore_peeked),   *)
(*                          1800-1884 (receive_head)                       *)
(*   produce_peeks          reduce.rs:986-1021, called at 780, 809, 934    *)
(*   peek continuation      rs/hot_store.rs:1315-1317                      *)
(*                          (continuation_identity includes the peeks)     *)
(*                                                                         *)
(* The operations use single-channel joins, so every COMM has one produce. *)
(* Every operation is non-persistent. The agent decides each outcome with  *)
(* a deterministic rule, and play and replay use the same rule. Casper     *)
(* reverts the store of a failed deploy in play and in replay alike, so    *)
(* the model keeps the committed prefix on both sides.                     *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

(* Apalache type aliases. TLC reads these lines as comments.               *)
\* @typeAlias: id = <<Str, Str, Int>>;
\* @typeAlias: comm = <<$id, $id, Int>>;
\* @typeAlias: iokey = <<Str, $id>>;
\* @typeAlias: ev = {kind: Str, p: $id, c: $id, t: Int};
\* @typeAlias: op = {kind: Str, ch: Str, val: Str, k: Str};
\* @typeAlias: store = {data: Str -> ($id -> Int), conts: Str -> ($id -> Int), joins: Str -> Bool};
\* @typeAlias: pstate = {st: $store, cnt: $id -> Int, log: Int -> $ev, len: Int};
\* @typeAlias: rstate = {st: $store, cnt: $id -> Int, log: Int -> $ev, len: Int, staged: $comm -> Int, ok: Bool};
\* @typeAlias: ck = {root: $store, state: $store};
\* @typeAlias: vw = {st: $store, cnt: $id -> Int, log: Int -> $ev, len: Int};
\* @typeAlias: plan = {fire1: Bool, m1: $comm, v1: $vw, two: Bool, v2: $vw};
CheckpointReplay_aliases == TRUE

CONSTANTS
    \* @type: Set(Str);
    Chans,
    \* @type: Set(Str);
    Vol,
    \* @type: Set(Str);
    Vals,
    \* @type: Set(Str);
    Konts,
    \* @type: Int;
    MaxTxnOps,
    \* @type: Int;
    MaxTxns,
    \* @type: Int;
    Blocks,
    \* @type: Int;
    Budget,
    \* @type: Bool;
    SpacesInBlockHash,
    \* @type: Bool;
    Forge,
    \* @type: Bool;
    UseUuid,
    \* @type: Bool;
    RootIgnoresVolatile,
    \* @type: Bool;
    DoublePair,
    \* @type: Bool;
    JoinResidue,
    \* @type: Bool;
    StagingDivergence,
    \* @type: Bool;
    AcceptUnpaired,
    \* TRUE: the agent issues peeks under Rule E (decision 45).
    \* @type: Bool;
    Peeks

ASSUME Vol \subseteq Chans
ASSUME MaxTxnOps \in 1..3
ASSUME MaxTxns \in 1..3
ASSUME Blocks \in 1..2
\* The continuation name of the peeks is not the name of a plain consume.
ASSUME "kp" \notin Konts

VARIABLES
    \* @type: Int;
    block,
    \* @type: Str;
    phase,
    \* @type: $store;
    pStore,
    \* @type: $id -> Int;
    pCount,
    \* @type: Int -> $ev;
    pLog,
    \* @type: Int;
    pLen,
    \* @type: Int -> (Int -> $op);
    prog,
    \* @type: Int;
    pNtx,
    \* @type: Int;
    pSpent,
    \* @type: Bool;
    pFailed,
    \* @type: Bool;
    forged,
    \* @type: $store;
    trie,
    \* @type: $store;
    preTrie,
    \* @type: $store;
    rTrie,
    \* @type: Set($ck);
    cks,
    \* @type: $store;
    rStore,
    \* @type: $id -> Int;
    rCount,
    \* @type: Int -> $ev;
    rLog,
    \* @type: Int;
    rLen,
    \* @type: Int;
    rTx,
    \* @type: Int;
    rSpent,
    \* @type: Bool;
    rFailed,
    \* @type: $iokey -> ($comm -> Int);
    rd,
    \* @type: Set($iokey);
    rigIO,
    \* @type: $comm -> Int;
    rigCount,
    \* @type: $comm -> Int;
    paired,
    \* @type: Bool;
    rOk,
    \* @type: Int -> $op;
    txn,
    \* @type: Int;
    pos,
    \* @type: $store;
    vSt,
    \* @type: $id -> Int;
    vCnt,
    \* @type: Int -> $ev;
    vLog,
    \* @type: Int;
    vLen,
    \* @type: $comm -> Int;
    vStaged,
    \* @type: Bool;
    vOk,
    \* @type: Int;
    vSpent,
    \* @type: Str;
    vStop

vars == <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, pSpent, pFailed, forged,
          trie, preTrie, rTrie, cks, rStore, rCount, rLog, rLen, rTx, rSpent, rFailed, rd,
          rigIO, rigCount, paired, rOk, txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent,
          vStop>>

\* The variables of the staged view of a transaction.
viewVars == <<txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent, vStop>>

\* The outcome variables of the deploy of the current block.
deployVars == <<pSpent, pFailed, forged, rSpent, rFailed>>

-----------------------------------------------------------------------------
(* Identities: Produce::create and Consume::create (trace/event.rs:155-236). *)
(* The nonce is 0 for content-hash identities. UseUuid gives each run its  *)
(* own nonce, as a random identifier would.                                *)

PlayNonce == IF UseUuid THEN 1 ELSE 0
ReplayNonce == IF UseUuid THEN 2 ELSE 0
\* The set {PlayNonce, ReplayNonce}, written without a repeated element:
\* Apalache keeps one cell for each listed element.
Nonces == IF UseUuid THEN {1, 2} ELSE {0}

\* The continuation names of peeks (Rule E). A peek continuation is a hot
\* store entry of its own (continuation_identity includes the peeks,
\* hot_store.rs:1315-1317), so the model gives it a name of its own.
\* @type: Set(Str);
PeekKonts == IF Peeks THEN {"kp"} ELSE {}
\* @type: Set(Str);
AllKonts == Konts \cup PeekKonts

\* @type: Set($id);
PIds == Chans \X Vals \X Nonces
\* @type: Set($id);
CIds == Chans \X AllKonts \X Nonces

\* The HostSteps of one agent operation: a produce stores or fires, a
\* consume parks or fires, and a peek fire or a produce that fires a peek
\* continuation adds the re-store. Each HostStep steps at most one counter.
\* A fire logs two events, and every other HostStep (the re-store too)
\* logs one, so an agent operation logs at most three events.
StepsPerOp == IF Peeks THEN 2 ELSE 1
EvPerOp == IF Peeks THEN 3 ELSE 2

MaxCnt == StepsPerOp * MaxTxns * MaxTxnOps
MaxLog == EvPerOp * MaxTxns * MaxTxnOps
Times == 0..MaxCnt

\* A COMM with one produce: <<consume, produce, times_repeated of it>>.
\* The consume and the produce use the same channel. This set equals
\* {m \in CIds \X PIds \X Times : m[1][1] = m[2][1]}. The injective map
\* form gives Apalache one cell per COMM and no filtered cells.
\* @type: Set($comm);
Comms == {<<<<c, k, n1>>, <<c, v, n2>>, t>> :
            c \in Chans, k \in AllKonts, n1 \in Nonces, v \in Vals, n2 \in Nonces, t \in Times}

\* replay_data keys: the IO events (replay_rspace.rs:71).
\* @type: Set($iokey);
IOKeys == {<<"C", q>> : q \in CIds} \cup {<<"P", p>> : p \in PIds}

\* The keys of a COMM: its consume, then its produce (design §2.4).
\* @type: $comm => Set($iokey);
Keys(m) == {<<"C", m[1]>>, <<"P", m[2]>>}

\* @type: $id;
NoP == <<"", "", 0>>
\* A placeholder for the COMM of a HostStep that does not fire. No COMM of
\* Comms equals it, and no rule reads it.
\* @type: $comm;
NoComm == <<NoP, NoP, 0>>
\* @type: $ev;
NoEv == [kind |-> "none", p |-> NoP, c |-> NoP, t |-> 0]
\* @type: $id => $ev;
PEv(p) == [kind |-> "P", p |-> p, c |-> NoP, t |-> 0]
\* @type: $id => $ev;
CEv(q) == [kind |-> "C", p |-> NoP, c |-> q, t |-> 0]
\* @type: $comm => $ev;
MEv(m) == [kind |-> "M", p |-> m[2], c |-> m[1], t |-> m[3]]
\* @type: Set($ev);
Events == {NoEv} \cup {PEv(p) : p \in PIds} \cup {CEv(q) : q \in CIds} \cup {MEv(m) : m \in Comms}

\* @type: $op;
Nop == [kind |-> "nop", ch |-> "", val |-> "", k |-> ""]
\* @type: (Str, Str) => $op;
ProduceOp(c, v) == [kind |-> "produce", ch |-> c, val |-> v, k |-> ""]
\* @type: (Str, Str) => $op;
ConsumeOp(c, k) == [kind |-> "consume", ch |-> c, val |-> "", k |-> k]
\* A peek receive (<<-) on c, with the continuation name of the peeks.
\* @type: Str => $op;
PeekOp(c) == [kind |-> "peek", ch |-> c, val |-> "", k |-> "kp"]
\* @type: Set($op);
Ops == {ProduceOp(c, v) : c \in Chans, v \in Vals} \cup {ConsumeOp(c, k) : c \in Chans, k \in Konts}
       \cup {PeekOp(c) : c \in IF Peeks THEN Chans ELSE {}}

\* A transaction: 1..MaxTxnOps operations, padded with Nop at the end. The
\* model states the shape as a predicate, so that Apalache never expands a
\* set of functions.
\* @type: Set(Int -> $op);
OpSeqs == [1..MaxTxnOps -> Ops \cup {Nop}]
\* @type: (Int -> $op) => Bool;
IsTxn(t) ==
    /\ t[1] /= Nop
    /\ \A i \in 1..(MaxTxnOps - 1) : t[i] = Nop => t[i + 1] = Nop
\* @type: Int -> $op;
NoTxn == [i \in 1..MaxTxnOps |-> Nop]

\* @type: $id -> Int;
EmptyPBag == [p \in PIds |-> 0]
\* @type: $id -> Int;
EmptyCBag == [q \in CIds |-> 0]
\* @type: $store;
EmptyStore == [data |-> [c \in Chans |-> EmptyPBag],
               conts |-> [c \in Chans |-> EmptyCBag],
               joins |-> [c \in Chans |-> FALSE]]
\* @type: $id -> Int;
EmptyCount == [p \in PIds |-> 0]
\* @type: Int -> $ev;
EmptyLog == [i \in 1..MaxLog |-> NoEv]
\* @type: Int -> (Int -> $op);
EmptyProg == [i \in 1..MaxTxns |-> NoTxn]

\* Data accumulates across blocks, counters reset at each checkpoint.
MaxStored == Blocks * MaxCnt
\* @type: Set($store);
StoreSet == [data : [Chans -> [PIds -> 0..MaxStored]],
             conts : [Chans -> [CIds -> 0..1]],
             joins : [Chans -> BOOLEAN]]

Phases == {"play", "playSwept", "rig", "replay", "replaySwept", "done"}

\* Where the commit of a staged transaction stops: nowhere ("none"), at a
\* rejected charge ("phlo"), or at an unpaired replay fire whose charges
\* pass ("div", ReplayDivergence).
Stops == {"none", "phlo", "div"}

\* The charges of a committed operation: its introduction, and the COMM of
\* a fire.
\* @type: Bool => Int;
OpCost(fires) == IF fires THEN 2 ELSE 1

\* The largest spending of a deploy: per agent operation, a charge of 3 (a
\* peek fire and its re-store), or of 2 (a fire) with no peeks. A larger
\* limit is never reached.
MaxBudget == (IF Peeks THEN 3 ELSE 2) * MaxTxns * MaxTxnOps
ASSUME Budget \in 1..MaxBudget

-----------------------------------------------------------------------------
(* The deterministic agent and the host steps.                             *)

\* The waiting continuations and the stored data that one copy can use.
\* @type: ($store, Str, Int) => Set($id);
Waiting(st, c, n) == {q \in CIds : q[1] = c /\ q[3] = n /\ st.conts[c][q] > 0}
\* @type: ($store, Str, Int) => Set($id);
Available(st, c, n) == {p \in PIds : p[1] = c /\ p[3] = n /\ st.data[c][p] > 0}
\* The waiting continuations that do not peek, and those that peek.
\* @type: ($store, Str, Int) => Set($id);
PlainWaiting(st, c, n) == {q \in Waiting(st, c, n) : q[2] \notin PeekKonts}
\* @type: ($store, Str, Int) => Set($id);
PeekWaiting(st, c, n) == {q \in Waiting(st, c, n) : q[2] \in PeekKonts}

\* remove_join after a non-persistent fire (hot_store.rs:1069-1113): the
\* join entry goes when the continuation list of the group becomes empty.
\* @type: (Str -> Bool, Str -> ($id -> Int), Str) => (Str -> Bool);
JoinsAfter(joins, conts, c) ==
    IF \A x \in CIds : conts[c][x] = 0 THEN [joins EXCEPT ![c] = FALSE] ELSE joins

\* The store after a produce fires the waiting continuation q on c.
\* @type: ($store, Str, $id) => $store;
FireCont(st, c, q) ==
    LET conts1 == [st.conts EXCEPT ![c][q] = 0]
    IN [data |-> st.data, conts |-> conts1, joins |-> JoinsAfter(st.joins, conts1, c)]

\* The store after a park: put_continuation (dedup by identity) and put_join
\* (ops_consume.rs:179-190, hot_store.rs:720-783, 1029-1052).
\* @type: ($store, Str, $id) => $store;
Park(st, c, q) ==
    [data |-> st.data, conts |-> [st.conts EXCEPT ![c][q] = 1], joins |-> [st.joins EXCEPT ![c] = TRUE]]

\* @type: ($store, Str, $id, Int) => $store;
AddDatum(st, c, p, d) ==
    [data |-> [st.data EXCEPT ![c][p] = @ + d], conts |-> st.conts, joins |-> st.joins]

\* The pairings that verify grants for COMM m (design §2.4). The correct
\* rule returns after the first bound key that still has an unpaired
\* binding. DoublePair pairs under every such key.
\* @type: ($comm, $comm -> Int) => Int;
Grants(m, staged) ==
    LET open == {k \in Keys(m) : rd[k][m] > staged[m]}
    IN IF open = {} THEN 0 ELSE IF DoublePair THEN Cardinality(open) ELSE 1

\* @type: ($store, $id -> Int, Int -> $ev, Int) => $vw;
VW(st, cnt, log, len) == [st |-> st, cnt |-> cnt, log |-> log, len |-> len]

\* @type: (Bool, $comm, $vw, Bool, $vw) => $plan;
Plan(fire1, m1, v1, two, v2) ==
    [fire1 |-> fire1, m1 |-> m1, v1 |-> v1, two |-> two, v2 |-> v2]

\* The HostSteps of an agent operation, by shape (design report D.1). A plan
\* gives whether the first HostStep fires, its COMM m1, and the view v1
\* after it. Then it gives whether a re-store follows (two), and the view v2
\* after the re-store (v2 = v1 when no re-store follows). n is the identity
\* nonce of the run. Changed by decision 90: a plan has no kind of its first
\* HostStep, because only the legacy charges read that kind.
\*
\* The counter rules give the same values. Play (ops_produce.rs:185-191,
\* trace_log.rs:158-172) counts an incoming produce as κ(p) + 1 in its COMM
\* and bumps κ(p) afterwards. Replay (replay_rspace.rs:1194, 1297) bumps
\* first and then counts. A consume COMM counts κ(p) in both runs, and every
\* produce that logs an event bumps κ(p) once.

\* A produce of the datum of op that stores it (no waiter).
\* @type: ($op, Int) => $plan;
PlanStore(op, n) ==
    LET c == op.ch
        p == <<c, op.val, n>>
        v1 == VW(AddDatum(vSt, c, p, 1), [vCnt EXCEPT ![p] = @ + 1],
                 [vLog EXCEPT ![vLen + 1] = PEv(p)], vLen + 1)
    IN Plan(FALSE, NoComm, v1, FALSE, v1)

\* A produce of the datum of op that fires the waiting continuation q. When
\* q peeks (restore), the re-store stores the datum again: this is the
\* re-offer of decision 30 and the produce of produce_peeks.
\* @type: ($op, Int, $id, Bool) => $plan;
PlanProduceFire(op, n, q, restore) ==
    LET c == op.ch
        p == <<c, op.val, n>>
        cnt1 == [vCnt EXCEPT ![p] = @ + 1]
        m == <<q, p, cnt1[p]>>
        v1 == VW(FireCont(vSt, c, q), cnt1,
                 [vLog EXCEPT ![vLen + 1] = PEv(p), ![vLen + 2] = MEv(m)], vLen + 2)
    IN Plan(TRUE, m, v1, restore,
            IF restore
            THEN VW(AddDatum(v1.st, c, p, 1), [cnt1 EXCEPT ![p] = @ + 1],
                    [v1.log EXCEPT ![vLen + 3] = PEv(p)], vLen + 3)
            ELSE v1)

\* A consume or a peek of op that fires the stored record p. Its COMM
\* removes the record (store_persistent_data, ops_consume.rs:192-202), and
\* for a peek (restore, Rule E) the re-store puts the same record back with
\* its identity (produce_peeks, reduce.rs:986-1021).
\* @type: ($op, Int, $id, Bool) => $plan;
PlanConsumeFire(op, n, p, restore) ==
    LET c == op.ch
        q == <<c, op.k, n>>
        m == <<q, p, vCnt[p]>>
        v1 == VW(AddDatum(vSt, c, p, -1), vCnt,
                 [vLog EXCEPT ![vLen + 1] = CEv(q), ![vLen + 2] = MEv(m)], vLen + 2)
    IN Plan(TRUE, m, v1, restore,
            IF restore
            THEN VW(AddDatum(v1.st, c, p, 1), [vCnt EXCEPT ![p] = @ + 1],
                    [v1.log EXCEPT ![vLen + 3] = PEv(p)], vLen + 3)
            ELSE v1)

\* A consume or a peek of op that parks its continuation (no stored record).
\* @type: ($op, Int) => $plan;
PlanPark(op, n) ==
    LET c == op.ch
        q == <<c, op.k, n>>
        v1 == VW(Park(vSt, c, q), vCnt, [vLog EXCEPT ![vLen + 1] = CEv(q)], vLen + 1)
    IN Plan(FALSE, NoComm, v1, FALSE, v1)

\* The staged view becomes v.
\* @type: $vw => Bool;
SetView(v) == vSt' = v.st /\ vCnt' = v.cnt /\ vLog' = v.log /\ vLen' = v.len

-----------------------------------------------------------------------------
(* Transactions. Begin copies the store into the staged view. Each stage   *)
(* step applies the HostSteps of one agent operation to the view, so       *)
(* TxnCommit commits the fold of the HostSteps of the transaction. No      *)
(* other action runs while pos > 0. A HostStep first makes its charges.    *)
(* When a charge fails, the commit stops at this HostStep (vStop =         *)
(* "phlo"). From a stop on, a stage step leaves the view as it is.         *)
(*                                                                         *)
(* The charges of a HostStep are the observer charges OpCost, against      *)
(* vSpent.                                                                 *)

\* Every operation of the transaction is staged. IF keeps TLC from reading
\* txn outside its domain.
StageDone == IF pos > MaxTxnOps THEN TRUE ELSE txn[pos] = Nop

\* The apply operators below give each variable its new value by a case
\* analysis over named stop points, so that Apalache keeps one transition
\* for each shape of HostSteps.

\* The play HostSteps of one agent operation (ops_produce.rs:32-248,
\* ops_consume.rs:31-202), with their charges. stop1: a charge of the
\* first HostStep fails, and no HostStep runs. stop2: a charge of the
\* re-store fails, and only the first HostStep runs.
\* @type: $plan => Bool;
PlayApply(pl) ==
    LET c1 == OpCost(pl.fire1)
        c2 == OpCost(FALSE)
        stop1 == ~(vSpent + c1 <= Budget)
        stop2 == ~stop1 /\ pl.two /\ ~(vSpent + c1 + c2 <= Budget)
        v == IF stop1 THEN VW(vSt, vCnt, vLog, vLen) ELSE IF stop2 THEN pl.v1 ELSE pl.v2
    IN /\ SetView(v)
       /\ vStop' = IF stop1 \/ stop2 THEN "phlo" ELSE vStop
       /\ vSpent' = CASE stop1 -> vSpent
                      [] stop2 \/ ~pl.two -> vSpent + c1
                      [] OTHER -> vSpent + c1 + c2
       /\ UNCHANGED <<vStaged, vOk>>

\* The replay HostSteps of one agent operation, re-stated over the staged
\* view (design §2.2, §2.4, decision 36). A fire whose observer charges
\* fail stops the commit there ("phlo"). Otherwise verify grants g
\* pairings. A fire with g = 0 stages unpaired: the commit stops there with
\* ReplayDivergence ("div"), after its charges pass. The unsafe rules:
\* StagingDivergence fails the staging of the transaction (vOk = FALSE,
\* the rule of decision 19), and AcceptUnpaired stages the fire as if it
\* paired, so the commit applies and logs it. The stop points: drop (the
\* staging fails), stop1 (an observer charge fails), div
\* (ReplayDivergence), stop2 (a charge of the re-store fails). At the first
\* three, no HostStep runs (none).
\* @type: $plan => Bool;
ReplayApply(pl) ==
    LET g == Grants(pl.m1, vStaged)
        unpaired == pl.fire1 /\ g = 0
        c1 == OpCost(pl.fire1)
        c2 == OpCost(FALSE)
        drop == unpaired /\ StagingDivergence
        stop1 == ~drop /\ ~(vSpent + c1 <= Budget)
        div == ~drop /\ ~stop1 /\ unpaired /\ ~AcceptUnpaired
        none == drop \/ stop1 \/ div
        stop2 == ~none /\ pl.two /\ ~(vSpent + c1 + c2 <= Budget)
        v == IF none THEN VW(vSt, vCnt, vLog, vLen) ELSE IF stop2 THEN pl.v1 ELSE pl.v2
    IN /\ SetView(v)
       /\ vOk' = IF drop THEN FALSE ELSE vOk
       /\ vStaged' = IF none \/ ~pl.fire1 THEN vStaged ELSE [vStaged EXCEPT ![pl.m1] = @ + g]
       /\ vStop' = CASE stop1 \/ stop2 -> "phlo"
                     [] div -> "div"
                     [] OTHER -> vStop
       /\ vSpent' = CASE drop \/ stop1 -> vSpent
                      [] div \/ stop2 \/ ~pl.two -> vSpent + c1
                      [] OTHER -> vSpent + c1 + c2

\* The HostSteps that the agent stages for the next operation of the
\* transaction, by shape, applied by Apply (PlayApply or ReplayApply). The
\* guards are exclusive and cover every operation. The agent fires a plain
\* continuation before a peek continuation, and the first waiter or record
\* that CHOOSE gives.
\* @type: (Int, $plan => Bool) => Bool;
StageNext(n, Apply(_)) ==
    LET op == txn[pos]
        c == op.ch
        WP == PlainWaiting(vSt, c, n)
        WK == PeekWaiting(vSt, c, n)
        A == Available(vSt, c, n)
        qPlain == CHOOSE x \in WP : TRUE
        qPeek == CHOOSE x \in WK : TRUE
        p == CHOOSE x \in A : TRUE
        fireCont == PlanProduceFire(op, n, qPlain, FALSE)
        firePeekCont == PlanProduceFire(op, n, qPeek, TRUE)
        store == PlanStore(op, n)
        fireRecord == PlanConsumeFire(op, n, p, FALSE)
        peekRecord == PlanConsumeFire(op, n, p, TRUE)
        park == PlanPark(op, n)
    IN \/ op.kind = "produce" /\ WP /= {} /\ Apply(fireCont)
       \/ Peeks /\ op.kind = "produce" /\ WP = {} /\ WK /= {} /\ Apply(firePeekCont)
       \/ op.kind = "produce" /\ WP = {} /\ WK = {} /\ Apply(store)
       \/ op.kind = "consume" /\ A /= {} /\ Apply(fireRecord)
       \/ Peeks /\ op.kind = "peek" /\ A /= {} /\ Apply(peekRecord)
       \/ op.kind /= "produce" /\ A = {} /\ Apply(park)

\* One play stage step, under the play counter rule.
PlayStage ==
    /\ phase = "play"
    /\ pos >= 1
    /\ ~StageDone
    /\ \/ /\ vStop /= "none"
          /\ UNCHANGED <<vSt, vCnt, vLog, vLen, vSpent, vStop, vStaged, vOk>>
       \/ /\ vStop = "none"
          /\ StageNext(PlayNonce, PlayApply)
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk, txn>>
    /\ UNCHANGED deployVars

\* One replay stage step, under the replay counter rule.
ReplayStage ==
    /\ phase = "replay"
    /\ pos >= 1
    /\ ~StageDone
    /\ vOk
    /\ \/ /\ vStop /= "none"
          /\ UNCHANGED <<vSt, vLog, vLen, vCnt, vStaged, vOk, vSpent, vStop>>
       \/ /\ vStop = "none"
          /\ StageNext(ReplayNonce, ReplayApply)
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk, txn>>
    /\ UNCHANGED deployVars

\* The commit fails the deploy at a stop "phlo". The cost of a failed
\* deploy is its limit.
\* @type: Bool;
CommitFails == vStop = "phlo"
\* @type: Int;
CommitCost == IF CommitFails THEN Budget ELSE vSpent

\* @type: $store => $store;
Sweep(st) ==
    IF RootIgnoresVolatile THEN st
    ELSE [data |-> [c \in Chans |-> IF c \in Vol THEN EmptyPBag ELSE st.data[c]],
          conts |-> [c \in Chans |-> IF c \in Vol THEN EmptyCBag ELSE st.conts[c]],
          joins |-> [c \in Chans |-> IF c \in Vol /\ ~JoinResidue THEN FALSE ELSE st.joins[c]]]

\* Checkpoint: the hot store changes become the history trie
\* (ispace_impl.rs:60-95). RootIgnoresVolatile drops the volatile keys.
\* @type: Str => Bool;
Persisted(c) == ~(RootIgnoresVolatile /\ c \in Vol)
\* @type: $store => $store;
Persist(st) ==
    [data |-> [c \in Chans |-> IF Persisted(c) THEN st.data[c] ELSE EmptyPBag],
     conts |-> [c \in Chans |-> IF Persisted(c) THEN st.conts[c] ELSE EmptyCBag],
     joins |-> [c \in Chans |-> IF Persisted(c) THEN st.joins[c] ELSE FALSE]]

\* The block hash covers the space state when SpacesInBlockHash holds.
\* @type: $store => $store;
RootOf(t) == IF SpacesInBlockHash THEN t ELSE EmptyStore

\* rig (replay_rspace.rs:480-540): every COMM of the log is bound under each
\* of its IO events that the log contains. The ranges run over 1..MaxLog
\* with the filter i <= pLen, because Apalache accepts only constant range
\* bounds. TypeOK gives pLen <= MaxLog, so the sets equal those over 1..pLen.
\* @type: $ev => $iokey;
EvKey(e) == IF e.kind = "P" THEN <<"P", e.p>> ELSE <<"C", e.c>>
\* @type: Set($iokey);
LogIO == {k \in IOKeys : \E i \in 1..MaxLog :
             i <= pLen /\ pLog[i].kind \in {"P", "C"} /\ EvKey(pLog[i]) = k}
\* @type: $comm => Int;
LogCount(m) == Cardinality({i \in 1..MaxLog : i <= pLen /\ pLog[i] = MEv(m)})

-----------------------------------------------------------------------------
(* Actions                                                                  *)

ZeroStaged == [m \in Comms |-> 0]

Init ==
    /\ block = 1
    /\ phase = "play"
    /\ pStore = EmptyStore
    /\ pCount = EmptyCount
    /\ pLog = EmptyLog
    /\ pLen = 0
    /\ prog = EmptyProg
    /\ pNtx = 0
    /\ pSpent = 0
    /\ pFailed = FALSE
    /\ forged = FALSE
    /\ trie = EmptyStore
    /\ preTrie = EmptyStore
    /\ rTrie = EmptyStore
    /\ cks = {[root |-> RootOf(EmptyStore), state |-> EmptyStore]}
    /\ rStore = EmptyStore
    /\ rCount = EmptyCount
    /\ rLog = EmptyLog
    /\ rLen = 0
    /\ rTx = 1
    /\ rSpent = 0
    /\ rFailed = FALSE
    /\ rd = [k \in IOKeys |-> [m \in Comms |-> 0]]
    /\ rigIO = {}
    /\ rigCount = ZeroStaged
    /\ paired = ZeroStaged
    /\ rOk = TRUE
    /\ txn = NoTxn
    /\ pos = 0
    /\ vSt = EmptyStore
    /\ vCnt = EmptyCount
    /\ vLog = EmptyLog
    /\ vLen = 0
    /\ vStaged = ZeroStaged
    /\ vOk = TRUE
    /\ vSpent = 0
    /\ vStop = "none"

\* begin: the agent picks the operations of a transaction and the staged
\* view starts as the hot store (design §1.3). A failed deploy begins no
\* further transaction.
PlayBegin ==
    /\ phase = "play"
    /\ pos = 0
    /\ pNtx < MaxTxns
    /\ ~pFailed
    /\ \E t \in OpSeqs : IsTxn(t) /\ txn' = t
    /\ pos' = 1
    /\ vSt' = pStore
    /\ vCnt' = pCount
    /\ vLog' = pLog
    /\ vLen' = pLen
    /\ vStaged' = ZeroStaged
    /\ vOk' = TRUE
    /\ vSpent' = pSpent
    /\ vStop' = "none"
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED deployVars

\* play TxnCommit: the commit applies the fold of the staged HostSteps up to
\* the stop. At a rejected charge the deploy fails, and the attempt counts,
\* so the spending equals the limit (casper/src/rust/rholang/runtime.rs:
\* 934-958 keeps the log of the failed deploy).
PlayCommit ==
    /\ phase = "play"
    /\ pos >= 1
    /\ StageDone
    /\ pSpent' = CommitCost
    /\ pFailed' = CommitFails
    /\ pStore' = vSt
    /\ pCount' = vCnt
    /\ pLog' = vLog
    /\ pLen' = vLen
    /\ prog' = [prog EXCEPT ![pNtx + 1] = txn]
    /\ pNtx' = pNtx + 1
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, trie, preTrie, rTrie, cks, rStore, rCount, rLog, rLen, rTx, rd,
                   rigIO, rigCount, paired, rOk, txn, vSt, vCnt, vLog, vLen, vStaged, vOk,
                   vSpent, vStop, forged, rSpent, rFailed>>

\* DeployEnd of the proposer: sweep Vol (design §3.1, decision D1). The
\* guard pos = 0 is the FR-9' guard of SpaceStoreHandle::sweep_volatile: the
\* handle refuses a sweep while the task of the deploy holds an open
\* transaction (NestedTransaction). A refused sweep changes nothing, so the
\* model omits it as a stutter step.
PlayDeployEnd ==
    /\ phase = "play"
    /\ pos = 0
    /\ pNtx >= 1
    /\ pStore' = Sweep(pStore)
    /\ phase' = "playSwept"
    /\ UNCHANGED <<block, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks, rStore,
                   rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars
    /\ UNCHANGED deployVars

\* Checkpoint of the proposer: persist, reset the counters, reload the
\* hot store from the history reader (ispace_impl.rs:60-95). The proposer
\* publishes its log. With Forge, it can omit the COMM of its last fire
\* (risk R1 of decision 36).
PlayCheckpoint ==
    /\ phase = "playSwept"
    /\ \/ /\ forged' = FALSE
          /\ UNCHANGED <<pLog, pLen>>
       \/ /\ Forge
          /\ pLen >= 1
          /\ pLog[pLen].kind = "M"
          /\ pLog' = [pLog EXCEPT ![pLen] = NoEv]
          /\ pLen' = pLen - 1
          /\ forged' = TRUE
    /\ trie' = Persist(pStore)
    /\ cks' = cks \cup {[root |-> RootOf(trie'), state |-> pStore]}
    /\ pStore' = trie'
    /\ pCount' = EmptyCount
    /\ phase' = "rig"
    /\ UNCHANGED <<block, prog, pNtx, preTrie, rTrie, rStore, rCount, rLog, rLen,
                   rTx, rd, rigIO, rigCount, paired, rOk, pSpent, pFailed, rSpent, rFailed>>
    /\ UNCHANGED viewVars

\* Rig: the validator resets to the root before the block (reset,
\* ispace_impl.rs:97-114) and binds the play log (replay_rspace.rs:480-540).
\* rigIO' and rigCount' get their values first, and rd' reads them. Thus
\* the model computes LogIO and each LogCount(m) once. The replayed deploy
\* starts with the full limit.
Rig ==
    /\ phase = "rig"
    /\ rigIO' = LogIO
    /\ rigCount' = [m \in Comms |-> LogCount(m)]
    /\ rd' = [k \in IOKeys |-> [m \in Comms |->
                IF k \in rigIO' /\ k \in Keys(m) THEN rigCount'[m] ELSE 0]]
    /\ paired' = ZeroStaged
    /\ rStore' = preTrie
    /\ rCount' = EmptyCount
    /\ rLog' = EmptyLog
    /\ rLen' = 0
    /\ rTx' = 1
    /\ rOk' = TRUE
    /\ rSpent' = 0
    /\ rFailed' = FALSE
    /\ phase' = "replay"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   pSpent, pFailed, forged>>
    /\ UNCHANGED viewVars

\* begin of a replayed transaction: the same operations as in play.
ReplayBegin ==
    /\ phase = "replay"
    /\ rOk
    /\ pos = 0
    /\ rTx <= pNtx
    /\ ~rFailed
    /\ txn' = prog[rTx]
    /\ pos' = 1
    /\ vSt' = rStore
    /\ vCnt' = rCount
    /\ vLog' = rLog
    /\ vLen' = rLen
    /\ vStaged' = ZeroStaged
    /\ vOk' = TRUE
    /\ vSpent' = rSpent
    /\ vStop' = "none"
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED deployVars

\* replay TxnCommit. A transaction whose staging failed (vOk = FALSE, only
\* under StagingDivergence) is dropped and fails the replay. Otherwise the
\* commit applies the fold of the staged HostSteps up to the stop, and it
\* removes the bindings of every pairing (remove_bindings_for,
\* replay_rspace.rs:1498-1526, saturating). A rejected charge fails the
\* deploy as in play. An unpaired fire fails the replay with
\* ReplayDivergence after the operations before it.
ReplayCommit ==
    /\ phase = "replay"
    /\ pos >= 1
    /\ IF vOk THEN StageDone ELSE TRUE
    /\ IF vOk
       THEN /\ rStore' = vSt
            /\ rCount' = vCnt
            /\ rLog' = vLog
            /\ rLen' = vLen
            /\ rd' = [k \in IOKeys |-> [m \in Comms |->
                        IF k \in Keys(m)
                        THEN IF rd[k][m] > vStaged[m] THEN rd[k][m] - vStaged[m] ELSE 0
                        ELSE rd[k][m]]]
            /\ paired' = [m \in Comms |-> paired[m] + vStaged[m]]
            /\ rTx' = rTx + 1
            /\ rSpent' = CommitCost
            /\ rFailed' = CommitFails
            /\ rOk' = (vStop /= "div")
       ELSE /\ rOk' = FALSE
            /\ UNCHANGED <<rStore, rCount, rLog, rLen, rd, paired, rTx, rSpent, rFailed>>
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rigIO, rigCount, txn, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent, vStop,
                   pSpent, pFailed, forged>>

\* A diverged replay rejects the block.
ReplayAbort ==
    /\ phase = "replay"
    /\ ~rOk
    /\ phase' = "done"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars
    /\ UNCHANGED deployVars

\* DeployEnd of the validator: sweep Vol, with the FR-9' guard pos = 0 (see
\* PlayDeployEnd). The deploy ends after the last transaction or at a
\* rejected charge.
ReplayDeployEnd ==
    /\ phase = "replay"
    /\ rOk
    /\ pos = 0
    /\ (rTx > pNtx \/ rFailed)
    /\ rStore' = Sweep(rStore)
    /\ phase' = "replaySwept"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars
    /\ UNCHANGED deployVars

\* Checkpoint of the validator, then the next block on the canonical chain.
\* The deploy of the next block starts with the full limit.
ReplayCheckpoint ==
    /\ phase = "replaySwept"
    /\ rTrie' = Persist(rStore)
    /\ cks' = cks \cup {[root |-> RootOf(rTrie'), state |-> rStore]}
    /\ rStore' = rTrie'
    /\ rCount' = EmptyCount
    /\ IF block < Blocks
       THEN /\ block' = block + 1
            /\ preTrie' = trie
            /\ pLog' = EmptyLog
            /\ pLen' = 0
            /\ prog' = EmptyProg
            /\ pNtx' = 0
            /\ rLog' = EmptyLog
            /\ rLen' = 0
            /\ pSpent' = 0
            /\ pFailed' = FALSE
            /\ forged' = FALSE
            /\ rSpent' = 0
            /\ rFailed' = FALSE
            /\ phase' = "play"
       ELSE /\ phase' = "done"
            /\ UNCHANGED <<block, preTrie, pLog, pLen, prog, pNtx, rLog, rLen>>
            /\ UNCHANGED deployVars
    /\ UNCHANGED <<pStore, pCount, trie, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars

Next ==
    \/ PlayBegin
    \/ PlayStage
    \/ PlayCommit
    \/ PlayDeployEnd
    \/ PlayCheckpoint
    \/ Rig
    \/ ReplayBegin
    \/ ReplayStage
    \/ ReplayCommit
    \/ ReplayAbort
    \/ ReplayDeployEnd
    \/ ReplayCheckpoint

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                               *)

TypeOK ==
    /\ block \in 1..Blocks
    /\ phase \in Phases
    /\ pStore \in StoreSet
    /\ pCount \in [PIds -> 0..MaxCnt]
    /\ pLog \in [1..MaxLog -> Events]
    /\ pLen \in 0..MaxLog
    /\ prog \in [1..MaxTxns -> OpSeqs]
    /\ pNtx \in 0..MaxTxns
    /\ pSpent \in 0..Budget
    /\ pFailed \in BOOLEAN
    /\ forged \in BOOLEAN
    /\ trie \in StoreSet
    /\ preTrie \in StoreSet
    /\ rTrie \in StoreSet
    /\ rStore \in StoreSet
    /\ rCount \in [PIds -> 0..MaxCnt]
    /\ rLog \in [1..MaxLog -> Events]
    /\ rLen \in 0..MaxLog
    /\ rTx \in 1..(MaxTxns + 1)
    /\ rSpent \in 0..Budget
    /\ rFailed \in BOOLEAN
    /\ rd \in [IOKeys -> [Comms -> 0..MaxLog]]
    /\ rigIO \subseteq IOKeys
    /\ rigCount \in [Comms -> 0..MaxLog]
    /\ paired \in [Comms -> 0..(2 * MaxLog)]
    /\ rOk \in BOOLEAN
    /\ txn \in OpSeqs
    /\ pos \in 0..(MaxTxnOps + 1)
    /\ vSt \in StoreSet
    /\ vCnt \in [PIds -> 0..MaxCnt]
    /\ vLog \in [1..MaxLog -> Events]
    /\ vLen \in 0..MaxLog
    /\ vStaged \in [Comms -> 0..(2 * MaxLog)]
    /\ vOk \in BOOLEAN
    /\ vSpent \in 0..Budget
    /\ vStop \in Stops

\* T1: the validator reproduces the play log event by event.
ReplayPrefixEqual ==
    /\ rLen <= pLen
    /\ \A i \in 1..rLen : rLog[i] = pLog[i]
    /\ phase = "replaySwept" => rLen = pLen

\* T2: the root commits to the hot store state at every checkpoint.
SameRootIffSameState ==
    \A x, y \in cks : (x.root = y.root) <=> (x.state = y.state)

\* T3: replay pairs each recorded COMM occurrence at most once.
PairedOnce == \A m \in Comms : paired[m] <= rigCount[m]

\* T4: no history trie holds data, continuations or joins of Vol.
\* @type: $store => Bool;
NoResidue(st) ==
    \A c \in Vol : st.data[c] = EmptyPBag /\ st.conts[c] = EmptyCBag /\ ~st.joins[c]
NoVolatileResidue == NoResidue(trie) /\ NoResidue(rTrie)

\* T6 (decision 36): an honest block (its log is the commit log of play) is
\* never rejected, and the replayed deploy ends with the failure and the
\* spending of play. Casper compares the status, then the cost
\* (casper/src/rust/rholang/replay_runtime.rs:581-594).
ReplayOutcomeEqual ==
    ~forged =>
        /\ rOk
        /\ (phase = "replaySwept" => (rFailed = pFailed /\ rSpent = pSpent))

-----------------------------------------------------------------------------
(* Inductive invariant for the Apalache BASE and STEP checks of T3 and T4.  *)
(* The strengthening says: a swept store has no volatile residue; the      *)
(* bindings hold the recorded occurrences that replay has not paired yet;  *)
(* and the pairings of an open replay transaction stay within them. The    *)
(* shape, progress and data conjuncts make the bounds of TypeOK inductive. *)

\* An open transaction (pos >= 1) exists only in play or in replay. An open
\* play transaction has a free program slot. An open replay transaction
\* replays a recorded transaction. A failed replay (rOk = FALSE) has no
\* open transaction, because the commit that fails it closes it.
TxnShapeInv ==
    /\ phase \notin {"play", "replay"} => pos = 0
    /\ (phase = "play" /\ pos >= 1) => pNtx < MaxTxns
    /\ (phase = "replay" /\ pos >= 1) => rTx <= pNtx
    /\ (phase = "replay" /\ ~rOk) => pos = 0

\* Each agent operation adds at most EvPerOp log events and at most
\* StepsPerOp produces to one counter. A transaction has at most MaxTxnOps
\* operations. The play counters are zero from the play checkpoint to the
\* next block.
ProgressInv ==
    /\ pLen <= EvPerOp * MaxTxnOps * pNtx
    /\ rLen <= EvPerOp * MaxTxnOps * (rTx - 1)
    /\ \A p \in PIds : pCount[p] <= StepsPerOp * MaxTxnOps * pNtx
    /\ \A p \in PIds : rCount[p] <= StepsPerOp * MaxTxnOps * (rTx - 1)
    /\ phase \in {"rig", "replay", "replaySwept", "done"} => pCount = EmptyCount
    /\ (phase = "play" /\ pos >= 1) =>
          /\ vLen <= pLen + EvPerOp * (pos - 1)
          /\ \A p \in PIds : vCnt[p] <= pCount[p] + StepsPerOp * (pos - 1)
    /\ (phase = "replay" /\ pos >= 1) =>
          /\ vLen <= rLen + EvPerOp * (pos - 1)
          /\ \A p \in PIds : vCnt[p] <= rCount[p] + StepsPerOp * (pos - 1)

\* A block adds at most MaxCnt copies of one datum, and every added copy
\* increments the counter of its produce. Data survives the checkpoints.
\* @type: ($store, Int, $id -> Int) => Bool;
DataBound(st, base, cnt) ==
    \A c \in Chans, p \in PIds : st.data[c][p] <= base + cnt[p]

DataInv ==
    /\ phase = "play" => DataBound(pStore, (block - 1) * MaxCnt, pCount)
    /\ phase /= "play" => DataBound(pStore, block * MaxCnt, EmptyCount)
    /\ phase /= "done" => DataBound(rStore, (block - 1) * MaxCnt, rCount)
    /\ phase = "done" => DataBound(rStore, block * MaxCnt, EmptyCount)
    /\ pos >= 1 => DataBound(vSt, (block - 1) * MaxCnt, vCnt)
    /\ DataBound(trie, block * MaxCnt, EmptyCount)
    /\ DataBound(preTrie, (block - 1) * MaxCnt, EmptyCount)

BindingInv ==
    phase \in {"replay", "replaySwept"} =>
        \A k \in IOKeys, m \in Comms :
            rd[k][m] = IF k \in rigIO /\ k \in Keys(m) THEN rigCount[m] - paired[m] ELSE 0

StagedInv ==
    (phase = "replay" /\ pos >= 1 /\ vOk) =>
        \A m \in Comms : paired[m] + vStaged[m] <= rigCount[m]

SweptClean ==
    /\ phase = "playSwept" => NoResidue(pStore)
    /\ phase = "replaySwept" => NoResidue(rStore)

IndInv ==
    /\ TypeOK
    /\ TxnShapeInv
    /\ ProgressInv
    /\ DataInv
    /\ PairedOnce
    /\ NoVolatileResidue
    /\ BindingInv
    /\ StagedInv
    /\ SweptClean

\* The STEP initializer: every state that satisfies IndInv. No action and
\* no conjunct of IndInv reads cks, so the initializer fixes cks to the
\* empty set without loss of generality.
IndInit ==
    /\ block \in 1..Blocks
    /\ phase \in Phases
    /\ pStore \in StoreSet
    /\ pCount \in [PIds -> 0..MaxCnt]
    /\ pLog \in [1..MaxLog -> Events]
    /\ pLen \in 0..MaxLog
    /\ prog \in [1..MaxTxns -> OpSeqs]
    /\ pNtx \in 0..MaxTxns
    /\ pSpent \in 0..Budget
    /\ pFailed \in BOOLEAN
    /\ forged \in BOOLEAN
    /\ trie \in StoreSet
    /\ preTrie \in StoreSet
    /\ rTrie \in StoreSet
    /\ cks = {}
    /\ rStore \in StoreSet
    /\ rCount \in [PIds -> 0..MaxCnt]
    /\ rLog \in [1..MaxLog -> Events]
    /\ rLen \in 0..MaxLog
    /\ rTx \in 1..(MaxTxns + 1)
    /\ rSpent \in 0..Budget
    /\ rFailed \in BOOLEAN
    /\ rd \in [IOKeys -> [Comms -> 0..MaxLog]]
    /\ rigIO \in SUBSET IOKeys
    /\ rigCount \in [Comms -> 0..MaxLog]
    /\ paired \in [Comms -> 0..(2 * MaxLog)]
    /\ rOk \in BOOLEAN
    /\ txn \in OpSeqs
    /\ pos \in 0..(MaxTxnOps + 1)
    /\ vSt \in StoreSet
    /\ vCnt \in [PIds -> 0..MaxCnt]
    /\ vLog \in [1..MaxLog -> Events]
    /\ vLen \in 0..MaxLog
    /\ vStaged \in [Comms -> 0..(2 * MaxLog)]
    /\ vOk \in BOOLEAN
    /\ vSpent \in 0..Budget
    /\ vStop \in Stops
    /\ IndInv
=============================================================================

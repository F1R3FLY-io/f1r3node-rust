-------------------------- MODULE CheckpointReplay --------------------------
(***************************************************************************)
(* Checkpoint and replay of Reified RSpaces substrate transactions (P1).    *)
(*                                                                         *)
(* The model composes a proposer (play) with a validator (replay) for one  *)
(* or two blocks. In each block, the proposer commits space transactions.  *)
(* Each stage step applies one HostStep to the staged view, and TxnCommit  *)
(* commits the fold of the HostSteps under the play counter rule. At the   *)
(* end of the deploy, DeployEnd sweeps the volatile channels Vol. Then     *)
(* Checkpoint persists the hot store into the history trie. Rig binds the  *)
(* play log for the validator. The validator resets to the root of the    *)
(* previous block and replays the same transactions. Each replay commit    *)
(* verifies and pairs every COMM against the recorded bindings, under the  *)
(* replay counter rule. Then the validator sweeps and checkpoints too.     *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   T1 ReplayPrefixEqual     the replay log is a prefix of the play log,  *)
(*                            and equals it when the replay completes.     *)
(*   T2 SameRootIffSameState  two checkpoints have the same root exactly   *)
(*                            when they have the same hot store state.     *)
(*   T3 PairedOnce            replay pairs a COMM at most as often as the  *)
(*                            play log records it.                         *)
(*   T4 NoVolatileResidue     no history trie holds a volatile channel.    *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                  *)
(*   UseUuid              identities carry a per-run nonce        -> T1   *)
(*   RootIgnoresVolatile  no sweep, the root skips Vol            -> T2   *)
(*   DoublePair           verify pairs under every bound key      -> T3   *)
(*   JoinResidue          the sweep keeps the join entries        -> T4   *)
(*                                                                         *)
(* Inductive check (Apalache): IndInv implies T3 and T4. BASE checks       *)
(* Init => IndInv. STEP checks IndInv /\ Next => IndInv', from every state *)
(* that satisfies IndInv (IndInit). Thus T3 and T4 hold at every depth for *)
(* the configured constants. With DoublePair or JoinResidue, STEP must     *)
(* refute IndInv (negative controls). TLC also checks IndInv on every      *)
(* reachable state of the safe configurations.                             *)
(*                                                                         *)
(* Code at HEAD 6f7a80da3 (rs = rspace++/src/rspace):                      *)
(*   play produce/consume   rs/rspace/ops_produce.rs:31-248,               *)
(*                          rs/rspace/ops_consume.rs:31-202                *)
(*   play counter rule      rs/rspace/ops_produce.rs:185-191,              *)
(*                          rs/rspace/trace_log.rs:158-172                 *)
(*   replay counter rule    rs/replay_rspace.rs:1104, 1298, 1489-1510      *)
(*   rig, check_replay_data rs/replay_rspace.rs:390-468                    *)
(*   remove_bindings_for    rs/replay_rspace.rs:1408-1436                  *)
(*   checkpoint, reset      rs/rspace/ispace_impl.rs:53-107                *)
(*   hot store primitives   rs/hot_store.rs:660-723, 819-885, 957-1053     *)
(*   sweep, verify          p1-design-report §3.1, §2.4                    *)
(*   sweep guard (FR-9')    SpaceStoreHandle::sweep_volatile, HandleState  *)
(*                                                                         *)
(* The operations use single-channel joins, so every COMM has one produce. *)
(* Every operation is non-persistent. The agent decides each outcome with  *)
(* a deterministic rule, and play and replay use the same rule.            *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

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
    \* @type: Bool;
    SpacesInBlockHash,
    \* @type: Bool;
    UseUuid,
    \* @type: Bool;
    RootIgnoresVolatile,
    \* @type: Bool;
    DoublePair,
    \* @type: Bool;
    JoinResidue

ASSUME Vol \subseteq Chans
ASSUME MaxTxnOps \in 1..3
ASSUME MaxTxns \in 1..3
ASSUME Blocks \in 1..2

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
    vOk

vars == <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
          cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk,
          txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk>>

\* The variables of the staged view of a transaction.
viewVars == <<txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk>>

-----------------------------------------------------------------------------
(* Identities: Produce::create and Consume::create (trace/event.rs:238-337). *)
(* The nonce is 0 for content-hash identities. UseUuid gives each run its  *)
(* own nonce, as a random identifier would.                                *)

PlayNonce == IF UseUuid THEN 1 ELSE 0
ReplayNonce == IF UseUuid THEN 2 ELSE 0
\* The set {PlayNonce, ReplayNonce}, written without a repeated element:
\* Apalache keeps one cell for each listed element.
Nonces == IF UseUuid THEN {1, 2} ELSE {0}

\* @type: Set($id);
PIds == Chans \X Vals \X Nonces
\* @type: Set($id);
CIds == Chans \X Konts \X Nonces

MaxCnt == MaxTxns * MaxTxnOps
MaxLog == 2 * MaxTxns * MaxTxnOps
Times == 0..MaxCnt

\* A COMM with one produce: <<consume, produce, times_repeated of it>>.
\* The consume and the produce use the same channel. This set equals
\* {m \in CIds \X PIds \X Times : m[1][1] = m[2][1]}. The injective map
\* form gives Apalache one cell per COMM and no filtered cells.
\* @type: Set($comm);
Comms == {<<<<c, k, n1>>, <<c, v, n2>>, t>> :
            c \in Chans, k \in Konts, n1 \in Nonces, v \in Vals, n2 \in Nonces, t \in Times}

\* replay_data keys: the IO events (replay_rspace.rs:64).
\* @type: Set($iokey);
IOKeys == {<<"C", q>> : q \in CIds} \cup {<<"P", p>> : p \in PIds}

\* The keys of a COMM: its consume, then its produce (design §2.4).
\* @type: $comm => Set($iokey);
Keys(m) == {<<"C", m[1]>>, <<"P", m[2]>>}

\* @type: $id;
NoP == <<"", "", 0>>
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
\* @type: Set($op);
Ops == {ProduceOp(c, v) : c \in Chans, v \in Vals} \cup {ConsumeOp(c, k) : c \in Chans, k \in Konts}

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

-----------------------------------------------------------------------------
(* The deterministic agent and the host steps.                             *)

\* The waiting continuations and the stored data that one copy can use.
\* @type: ($store, Str, Int) => Set($id);
Waiting(st, c, n) == {q \in CIds : q[1] = c /\ q[3] = n /\ st.conts[c][q] > 0}
\* @type: ($store, Str, Int) => Set($id);
Available(st, c, n) == {p \in PIds : p[1] = c /\ p[3] = n /\ st.data[c][p] > 0}

\* remove_join after a non-persistent fire (hot_store.rs:1009-1053): the
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
\* (ops_consume.rs:179-190, hot_store.rs:660-723, 957-980).
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

-----------------------------------------------------------------------------
(* Transactions. Begin copies the store into the staged view. Each stage   *)
(* step applies one HostStep to the view, so TxnCommit commits the fold of *)
(* the HostSteps of the transaction. No other action runs while pos > 0.   *)

\* Every operation of the transaction is staged. IF keeps TLC from reading
\* txn outside its domain.
StageDone == IF pos > MaxTxnOps THEN TRUE ELSE txn[pos] = Nop

\* One play HostStep (ops_produce.rs:31-248, ops_consume.rs:31-202) under
\* the play counter rule: the COMM counts the incoming produce as κ(p) + 1
\* (ops_produce.rs:185-191), and log_produce bumps κ(p) afterwards
\* (trace_log.rs:158-172).
PlayStage ==
    /\ phase = "play"
    /\ pos >= 1
    /\ ~StageDone
    /\ LET op == txn[pos]
           c == op.ch
       IN IF op.kind = "produce"
          THEN LET p == <<c, op.val, PlayNonce>>
                   W == Waiting(vSt, c, PlayNonce)
               IN IF W /= {}
                  THEN LET q == CHOOSE x \in W : TRUE
                           m == <<q, p, vCnt[p] + 1>>
                       IN /\ vSt' = FireCont(vSt, c, q)
                          /\ vLog' = [vLog EXCEPT ![vLen + 1] = PEv(p), ![vLen + 2] = MEv(m)]
                          /\ vLen' = vLen + 2
                          /\ vCnt' = [vCnt EXCEPT ![p] = @ + 1]
                  ELSE /\ vSt' = AddDatum(vSt, c, p, 1)
                       /\ vLog' = [vLog EXCEPT ![vLen + 1] = PEv(p)]
                       /\ vLen' = vLen + 1
                       /\ vCnt' = [vCnt EXCEPT ![p] = @ + 1]
          ELSE LET q == <<c, op.k, PlayNonce>>
                   A == Available(vSt, c, PlayNonce)
               IN IF A /= {}
                  THEN LET p == CHOOSE x \in A : TRUE
                           m == <<q, p, vCnt[p]>>
                       IN /\ vSt' = AddDatum(vSt, c, p, -1)
                          /\ vLog' = [vLog EXCEPT ![vLen + 1] = CEv(q), ![vLen + 2] = MEv(m)]
                          /\ vLen' = vLen + 2
                          /\ UNCHANGED vCnt
                  ELSE /\ vSt' = Park(vSt, c, q)
                       /\ vLog' = [vLog EXCEPT ![vLen + 1] = CEv(q)]
                       /\ vLen' = vLen + 1
                       /\ UNCHANGED vCnt
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk,
                   txn, vStaged, vOk>>

\* One replay HostStep, re-stated over the staged view (design §2.2, §2.4)
\* under the replay counter rule: increment first (replay_rspace.rs:1104),
\* then count (1297). Every COMM must pass verify; a failure marks the
\* transaction as diverged.
ReplayStage ==
    /\ phase = "replay"
    /\ pos >= 1
    /\ ~StageDone
    /\ vOk
    /\ LET op == txn[pos]
           c == op.ch
       IN IF op.kind = "produce"
          THEN LET p == <<c, op.val, ReplayNonce>>
                   cnt1 == [vCnt EXCEPT ![p] = @ + 1]
                   W == Waiting(vSt, c, ReplayNonce)
               IN IF W /= {}
                  THEN LET q == CHOOSE x \in W : TRUE
                           m == <<q, p, cnt1[p]>>
                           g == Grants(m, vStaged)
                       IN IF g = 0
                          THEN /\ vOk' = FALSE
                               /\ UNCHANGED <<vSt, vLog, vLen, vCnt, vStaged>>
                          ELSE /\ vSt' = FireCont(vSt, c, q)
                               /\ vLog' = [vLog EXCEPT ![vLen + 1] = PEv(p), ![vLen + 2] = MEv(m)]
                               /\ vLen' = vLen + 2
                               /\ vCnt' = cnt1
                               /\ vStaged' = [vStaged EXCEPT ![m] = @ + g]
                               /\ UNCHANGED vOk
                  ELSE /\ vSt' = AddDatum(vSt, c, p, 1)
                       /\ vLog' = [vLog EXCEPT ![vLen + 1] = PEv(p)]
                       /\ vLen' = vLen + 1
                       /\ vCnt' = cnt1
                       /\ UNCHANGED <<vStaged, vOk>>
          ELSE LET q == <<c, op.k, ReplayNonce>>
                   A == Available(vSt, c, ReplayNonce)
               IN IF A /= {}
                  THEN LET p == CHOOSE x \in A : TRUE
                           m == <<q, p, vCnt[p]>>
                           g == Grants(m, vStaged)
                       IN IF g = 0
                          THEN /\ vOk' = FALSE
                               /\ UNCHANGED <<vSt, vLog, vLen, vCnt, vStaged>>
                          ELSE /\ vSt' = AddDatum(vSt, c, p, -1)
                               /\ vLog' = [vLog EXCEPT ![vLen + 1] = CEv(q), ![vLen + 2] = MEv(m)]
                               /\ vLen' = vLen + 2
                               /\ vStaged' = [vStaged EXCEPT ![m] = @ + g]
                               /\ UNCHANGED <<vCnt, vOk>>
                  ELSE /\ vSt' = Park(vSt, c, q)
                       /\ vLog' = [vLog EXCEPT ![vLen + 1] = CEv(q)]
                       /\ vLen' = vLen + 1
                       /\ UNCHANGED <<vCnt, vStaged, vOk>>
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk, txn>>

\* @type: $store => $store;
Sweep(st) ==
    IF RootIgnoresVolatile THEN st
    ELSE [data |-> [c \in Chans |-> IF c \in Vol THEN EmptyPBag ELSE st.data[c]],
          conts |-> [c \in Chans |-> IF c \in Vol THEN EmptyCBag ELSE st.conts[c]],
          joins |-> [c \in Chans |-> IF c \in Vol /\ ~JoinResidue THEN FALSE ELSE st.joins[c]]]

\* Checkpoint: the hot store changes become the history trie
\* (ispace_impl.rs:53-88). RootIgnoresVolatile drops the volatile keys.
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

\* rig (replay_rspace.rs:390-450): every COMM of the log is bound under each
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
    /\ trie = EmptyStore
    /\ preTrie = EmptyStore
    /\ rTrie = EmptyStore
    /\ cks = {[root |-> RootOf(EmptyStore), state |-> EmptyStore]}
    /\ rStore = EmptyStore
    /\ rCount = EmptyCount
    /\ rLog = EmptyLog
    /\ rLen = 0
    /\ rTx = 1
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

\* begin: the agent picks the operations of a transaction and the staged
\* view starts as the hot store (design §1.3).
PlayBegin ==
    /\ phase = "play"
    /\ pos = 0
    /\ pNtx < MaxTxns
    /\ \E t \in OpSeqs : IsTxn(t) /\ txn' = t
    /\ pos' = 1
    /\ vSt' = pStore
    /\ vCnt' = pCount
    /\ vLog' = pLog
    /\ vLen' = pLen
    /\ vStaged' = ZeroStaged
    /\ vOk' = TRUE
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>

\* play TxnCommit: the commit applies the fold of the staged HostSteps.
PlayCommit ==
    /\ phase = "play"
    /\ pos >= 1
    /\ StageDone
    /\ pStore' = vSt
    /\ pCount' = vCnt
    /\ pLog' = vLog
    /\ pLen' = vLen
    /\ prog' = [prog EXCEPT ![pNtx + 1] = txn]
    /\ pNtx' = pNtx + 1
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, trie, preTrie, rTrie, cks, rStore, rCount, rLog, rLen, rTx, rd,
                   rigIO, rigCount, paired, rOk, txn, vSt, vCnt, vLog, vLen, vStaged, vOk>>

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

\* Checkpoint of the proposer: persist, reset the counters, reload the
\* hot store from the history reader (ispace_impl.rs:53-88).
PlayCheckpoint ==
    /\ phase = "playSwept"
    /\ trie' = Persist(pStore)
    /\ cks' = cks \cup {[root |-> RootOf(trie'), state |-> pStore]}
    /\ pStore' = trie'
    /\ pCount' = EmptyCount
    /\ phase' = "rig"
    /\ UNCHANGED <<block, pLog, pLen, prog, pNtx, preTrie, rTrie, rStore, rCount, rLog, rLen,
                   rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars

\* Rig: the validator resets to the root before the block (reset,
\* ispace_impl.rs:90-107) and binds the play log (replay_rspace.rs:390-450).
\* rigIO' and rigCount' get their values first, and rd' reads them. Thus
\* the model computes LogIO and each LogCount(m) once.
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
    /\ phase' = "replay"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks>>
    /\ UNCHANGED viewVars

\* begin of a replayed transaction: the same operations as in play.
ReplayBegin ==
    /\ phase = "replay"
    /\ rOk
    /\ pos = 0
    /\ rTx <= pNtx
    /\ txn' = prog[rTx]
    /\ pos' = 1
    /\ vSt' = rStore
    /\ vCnt' = rCount
    /\ vLog' = rLog
    /\ vLen' = rLen
    /\ vStaged' = ZeroStaged
    /\ vOk' = TRUE
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>

\* replay TxnCommit: a diverged transaction is dropped and fails the
\* replay. Otherwise the commit applies the fold of the staged HostSteps
\* and removes the bindings of every pairing (remove_bindings_for,
\* replay_rspace.rs:1408-1436, saturating).
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
            /\ UNCHANGED rOk
       ELSE /\ rOk' = FALSE
            /\ UNCHANGED <<rStore, rCount, rLog, rLen, rd, paired, rTx>>
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rigIO, rigCount, txn, vSt, vCnt, vLog, vLen, vStaged, vOk>>

\* A diverged replay rejects the block.
ReplayAbort ==
    /\ phase = "replay"
    /\ ~rOk
    /\ phase' = "done"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars

\* DeployEnd of the validator: sweep Vol, with the FR-9' guard pos = 0 (see
\* PlayDeployEnd).
ReplayDeployEnd ==
    /\ phase = "replay"
    /\ rOk
    /\ pos = 0
    /\ rTx > pNtx
    /\ rStore' = Sweep(rStore)
    /\ phase' = "replaySwept"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars

\* Checkpoint of the validator, then the next block on the canonical chain.
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
            /\ phase' = "play"
       ELSE /\ phase' = "done"
            /\ UNCHANGED <<block, preTrie, pLog, pLen, prog, pNtx, rLog, rLen>>
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
    /\ trie \in StoreSet
    /\ preTrie \in StoreSet
    /\ rTrie \in StoreSet
    /\ rStore \in StoreSet
    /\ rCount \in [PIds -> 0..MaxCnt]
    /\ rLog \in [1..MaxLog -> Events]
    /\ rLen \in 0..MaxLog
    /\ rTx \in 1..(MaxTxns + 1)
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

\* Each staged operation adds at most two log events and at most one
\* produce to one counter. A transaction has at most MaxTxnOps operations.
\* The play counters are zero from the play checkpoint to the next block.
ProgressInv ==
    /\ pLen <= 2 * MaxTxnOps * pNtx
    /\ rLen <= 2 * MaxTxnOps * (rTx - 1)
    /\ \A p \in PIds : pCount[p] <= MaxTxnOps * pNtx
    /\ \A p \in PIds : rCount[p] <= MaxTxnOps * (rTx - 1)
    /\ phase \in {"rig", "replay", "replaySwept", "done"} => pCount = EmptyCount
    /\ (phase = "play" /\ pos >= 1) =>
          /\ vLen <= pLen + 2 * (pos - 1)
          /\ \A p \in PIds : vCnt[p] <= pCount[p] + (pos - 1)
    /\ (phase = "replay" /\ pos >= 1) =>
          /\ vLen <= rLen + 2 * (pos - 1)
          /\ \A p \in PIds : vCnt[p] <= rCount[p] + (pos - 1)

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
    /\ trie \in StoreSet
    /\ preTrie \in StoreSet
    /\ rTrie \in StoreSet
    /\ cks = {}
    /\ rStore \in StoreSet
    /\ rCount \in [PIds -> 0..MaxCnt]
    /\ rLog \in [1..MaxLog -> Events]
    /\ rLen \in 0..MaxLog
    /\ rTx \in 1..(MaxTxns + 1)
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
    /\ IndInv
=============================================================================

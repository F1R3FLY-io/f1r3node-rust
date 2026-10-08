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
(* Decision 45, Option P (constant Legacy). In a legacy deploy, the        *)
(* observer calls charge no phlo. Each staged operation pays what          *)
(* ChargingRSpace charges for the equivalent host call: a pre-charge       *)
(* before the call, and the post-charges of handle_result after it. A      *)
(* refund is a negative charge, and a zero refund is logged too. The       *)
(* callback before_observe of operation i charges the post-charges of      *)
(* operation i - 1 (the carry) and then the pre-charge of i. After the     *)
(* commit, the tail charges the post-charges of the last operation.        *)
(* charge_legacy rejects a charge when the remaining budget is negative    *)
(* before it. Otherwise it subtracts the charge, logs it, and fails when   *)
(* the remaining budget is negative after it. The first failure stops the  *)
(* run. In callback i, the run stops before operation i, and the           *)
(* operations before i stay applied. In the tail, the run stops after the  *)
(* full commit. The deploy then fails and costs its limit (total_cost).    *)
(* Replay stages the same operations and runs the same callbacks. A ghost  *)
(* run of the equivalent host program makes the pre-charge and then the    *)
(* post-charges of each host operation directly, and U13 compares the two  *)
(* runs. The re-store of a peek is a host operation of its own there.      *)
(*                                                                         *)
(* Invariants (T1-T6 are ids of the P1 map, and U13 is an id of the P4     *)
(* map):                                                                   *)
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
(*   U13 LegacyChargesFollowHost  in a legacy deploy, play has the legacy  *)
(*                            log, the remaining budget and the stop point *)
(*                            of the equivalent host program whenever no   *)
(*                            play transaction is open, and a completed    *)
(*                            replay has the legacy log of that program.   *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   UseUuid              identities carry a per-run nonce        -> T1    *)
(*   RootIgnoresVolatile  no sweep, the root skips Vol            -> T2    *)
(*   DoublePair           verify pairs under every bound key      -> T3    *)
(*   JoinResidue          the sweep keeps the join entries        -> T4    *)
(*   StagingDivergence    an unpaired replay fire fails staging   -> T6    *)
(*                        (the rule of decision 19)                        *)
(*   AcceptUnpaired       an unpaired replay fire commits         -> T1    *)
(*   LegacyPreAfterObserve  the pre-charge of a fire follows its  -> T6    *)
(*                        observer calls, so a replay fire that            *)
(*                        play left unpaired diverges before it            *)
(*   LegacyTailDropped    no tail after the commit                -> U13   *)
(*   RetainPeek           a peek fire keeps its record and has no -> U13   *)
(*                        re-store (FR-10, before decision 45)             *)
(*                                                                         *)
(* Inductive check (Apalache): IndInv implies T3 and T4. BASE checks       *)
(* Init => IndInv. STEP checks IndInv /\ Next => IndInv', from every state *)
(* that satisfies IndInv (IndInit). Thus T3 and T4 hold at every depth for *)
(* the configured constants: a token deploy with no peeks, and a legacy    *)
(* deploy with peeks (LegacyInv bounds the legacy states). With DoublePair *)
(* or JoinResidue, STEP must refute IndInv (negative controls). TLC also   *)
(* checks IndInv on every reachable state of the safe configurations. T1,  *)
(* T2, T6 and U13 are checked by TLC on the full state space. The model    *)
(* claims no inductive form of them.                                       *)
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
(*   hot store primitives   rs/hot_store.rs:720-783, 879-945, 1017-1113    *)
(*   failed deploy (play)   casper/src/rust/rholang/runtime.rs:934-958     *)
(*   status, cost (replay)  casper/src/rust/rholang/replay_runtime.rs:619-632 *)
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
(*   re-store (Rule E)      spaces/generic.rs:1066-1081 (Cx::restore and    *)
(*                          Cx::restore_on), 1265-1274 (restore_peeked),   *)
(*                          1180-1262 (receive_head)                       *)
(*   produce_peeks          reduce.rs:981-1016, called at 780, 809, 934    *)
(*   peek continuation      rs/hot_store.rs:1315-1317                      *)
(*                          (continuation_identity includes the peeks)     *)
(*   pre-charges            storage/charging_rspace.rs:96-100              *)
(*                          (consume), 133-134 (produce)                   *)
(*   host call, result      storage/charging_rspace.rs:104-117, 137-149    *)
(*   post-charges           storage/charging_rspace.rs:298-349             *)
(*                          (handle_result: refunds 320-337, event fee     *)
(*                          of the trigger 339-343, COMM fee 345, no       *)
(*                          match 347)                                     *)
(*   refund of produces     storage/charging_rspace.rs:351-381             *)
(*   charge values          accounting/costs.rs:359-396                    *)
(*   charge_legacy          accounting/mod.rs:871-890                      *)
(*   legacy cost            accounting/mod.rs:2584-2598 (total_cost)       *)
(*   no observer phlo       accounting/mod.rs:950-952                      *)
(*                          (reserve_consensus_identity returns at once in *)
(*                          a legacy deploy)                               *)
(*   chain                  reified/legacy_charges.rs: LegacyChain,        *)
(*                          LegacyChain::attach (callback i: the carry,    *)
(*                          then the pre-charge), LegacyChain::finish (the *)
(*                          tail after the commit)                         *)
(*                                                                         *)
(* The operations use single-channel joins, so every COMM has one produce. *)
(* Every operation is non-persistent. The agent decides each outcome with  *)
(* a deterministic rule, and play and replay use the same rule. Casper     *)
(* reverts the store of a failed deploy in play and in replay alike, so    *)
(* the model keeps the committed prefix on both sides. The legacy charge   *)
(* values are abstract (see LSP, LE, LSC and LCE). They give every         *)
(* HostStep a positive net charge, so the model does not cover a record    *)
(* so large that a host fire refunds more than it charges, and a deploy    *)
(* cost that is negative (design report F.5).                              *)
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
\* @typeAlias: plan = {k1: Str, fire1: Bool, m1: $comm, v1: $vw, two: Bool, v2: $vw};
\* @typeAlias: leg = {log: Int -> Int, len: Int, rem: Int};
\* @typeAlias: lres = {ls: $leg, ok: Bool};
\* @typeAlias: hres = {ls: $leg, ops: Int, failed: Bool};
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
    Peeks,
    \* TRUE: the deploy is a legacy deploy, and the space operations pay the
    \* storage charges of ChargingRSpace (decision 45, Option P).
    \* @type: Bool;
    Legacy,
    \* @type: Bool;
    LegacyPreAfterObserve,
    \* @type: Bool;
    LegacyTailDropped,
    \* @type: Bool;
    RetainPeek

ASSUME Vol \subseteq Chans
ASSUME MaxTxnOps \in 1..3
ASSUME MaxTxns \in 1..3
ASSUME Blocks \in 1..2
\* The continuation name of the peeks is not the name of a plain consume.
ASSUME "kp" \notin Konts
\* The legacy controls change the legacy charges, so they need Legacy, and
\* RetainPeek needs peeks.
ASSUME (LegacyPreAfterObserve \/ LegacyTailDropped \/ RetainPeek) => Legacy
ASSUME RetainPeek => Peeks

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
    vStop,
    \* The legacy state (log, remaining budget) of the staged view.
    \* @type: $leg;
    vLeg,
    \* The post kind of the last staged operation: the carry of the chain.
    \* @type: Str;
    vCarry,
    \* The HostSteps that the staged view of play has applied in the deploy.
    \* @type: Int;
    vOps,
    \* The legacy state of play after its last commit.
    \* @type: $leg;
    pLeg,
    \* The HostSteps that play has applied in the deploy.
    \* @type: Int;
    pOps,
    \* The legacy state of replay after its last commit.
    \* @type: $leg;
    rLeg,
    \* The ghost run of the equivalent host program of play: its legacy
    \* state, the host operations that it ran, and whether it failed.
    \* @type: $leg;
    hLeg,
    \* @type: Int;
    hOps,
    \* @type: Bool;
    hFailed

vars == <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, pSpent, pFailed, forged,
          trie, preTrie, rTrie, cks, rStore, rCount, rLog, rLen, rTx, rSpent, rFailed, rd,
          rigIO, rigCount, paired, rOk, txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent,
          vStop, vLeg, vCarry, vOps, pLeg, pOps, rLeg, hLeg, hOps, hFailed>>

\* The variables of the staged view of a transaction.
viewVars == <<txn, pos, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent, vStop, vLeg, vCarry, vOps>>

\* The outcome variables of the deploy of the current block.
deployVars == <<pSpent, pFailed, forged, rSpent, rFailed>>

\* The committed legacy state of the deploy of the current block, and the
\* ghost run of its host program.
legVars == <<pLeg, pOps, rLeg, hLeg, hOps, hFailed>>

-----------------------------------------------------------------------------
(* Identities: Produce::create and Consume::create (trace/event.rs:238-337). *)
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

\* The observer charges of a committed HostStep. A legacy deploy makes the
\* observer calls with no phlo: reserve_consensus_identity returns at once
\* (accounting/mod.rs:950-952).
\* @type: Bool => Int;
ObsCost(fires) == IF Legacy THEN 0 ELSE OpCost(fires)

-----------------------------------------------------------------------------
(* Legacy charges (decision 45, Option P).                                  *)
(*                                                                         *)
(* A HostStep has a kind: "store" (a produce that stores, the re-store     *)
(* too), "park" (a consume or a peek that parks), "pfire" (a produce that  *)
(* fires), "cfire" (a consume or a peek that fires), or "kept" (a peek     *)
(* fire that keeps its record, only under RetainPeek). ChargingRSpace      *)
(* charges the pre-charge of the kind before the host call and the         *)
(* post-charges of its post kind after it.                                 *)

\* Abstract charge values, one for each label of ChargingRSpace. The host
\* charges SP(c, d) = |c| + |d| bytes ("produces storage",
\* storage_cost_produce, accounting/costs.rs:376-380), SC(cs, ps, k) =
\* |cs| + |ps| + |k| bytes ("consume storage", storage_cost_consume,
\* accounting/costs.rs:359-374), E(1) = 64 ("event storage cost",
\* event_storage_cost, accounting/costs.rs:391-396) and CE(1) = 128 ("comm
\* event storage cost", comm_event_storage_cost, accounting/costs.rs:382-389).
\* The values keep the four labels distinct, keep CE(1) = 2 * E(1), and give
\* every HostStep a positive net charge.
LSP == 1
LE == 2
LSC == 3
LCE == 4
NegSP == 0 - LSP
NegSC == 0 - LSC

\* The pre-charge of a HostStep: the storage of its datum (a produce,
\* charging_rspace.rs:133-134) or of its continuation (a consume or a peek,
\* charging_rspace.rs:96-100).
\* @type: Str => Int;
PreCharge(k) == IF k \in {"store", "pfire"} THEN LSP ELSE LSC

\* The post kind of a HostStep kind.
\* @type: Str => Str;
PostKind(k) ==
    IF k \in {"store", "park"} THEN "event" ELSE IF k = "kept" THEN "kept" ELSE "fire"

\* The post-charges of handle_result (charging_rspace.rs:298-349), by post
\* kind. "event": a HostStep with no match pays the event fee of its one
\* channel (:347). "fire": a fire of a trigger and a waiter that are not
\* persistent refunds the continuation (:320-325, 330-333), refunds the
\* removed datum (:327-328, 334-337, 351-381), and pays the event fee of
\* the trigger (:339-343) and the COMM fee (:345). "kept": a peek fire that
\* keeps its record reports it as persistent, so its refund is a logged
\* zero. "none": no post-charges (the carry before the first callback).
\* @type: Str => Seq(Int);
PostList(k) ==
    CASE k = "event" -> <<LE>>
      [] k = "fire" -> <<NegSC, NegSP, LE, LCE>>
      [] k = "kept" -> <<NegSC, 0, LE, LCE>>
      [] OTHER -> <<>>

\* The legacy log entries of one deploy: an agent operation makes at most
\* seven (a peek: the pre-charge and four post-charges of its fire, then
\* the pre-charge and the post-charge of its re-store), and at most five
\* with no peeks.
MaxLeg == IF Legacy THEN (IF Peeks THEN 7 ELSE 5) * MaxTxns * MaxTxnOps ELSE 1

\* The HostSteps that play applies in one deploy.
MaxOps == StepsPerOp * MaxTxns * MaxTxnOps

\* The largest spending of a deploy: per agent operation, an observer
\* charge of 3 (a peek fire and its re-store) in a token deploy, or the
\* positive legacy charges of a peek (3 + 2 + 4 + 1 + 2 = 12) in a legacy
\* deploy. A larger limit is never reached.
MaxBudget == (IF Legacy THEN 12 ELSE IF Peeks THEN 3 ELSE 2) * MaxTxns * MaxTxnOps
ASSUME Budget \in 1..MaxBudget

\* The legacy states. A charge list has at most five charges (the carry,
\* at most four, and a pre-charge), so RemAfter covers every list. A charge
\* fails only when it is positive and the remaining budget before it is
\* zero or more, so the remaining budget never falls below -LCE.
\* @type: Set($leg);
LegSet == [log : [1..MaxLeg -> NegSC..LCE], len : 0..MaxLeg, rem : (0 - LCE)..Budget]
\* @type: $leg;
InitLeg == [log |-> [i \in 1..MaxLeg |-> 0], len |-> 0, rem |-> Budget]
Carries == {"none", "event", "fire", "kept"}

\* The remaining budget r after the first i charges of cs (Len(cs) <= 5).
\* @type: (Int, Seq(Int), Int) => Int;
RemAfter(r, cs, i) ==
    r - (IF i >= 1 THEN cs[1] ELSE 0) - (IF i >= 2 THEN cs[2] ELSE 0)
      - (IF i >= 3 THEN cs[3] ELSE 0) - (IF i >= 4 THEN cs[4] ELSE 0)
      - (IF i >= 5 THEN cs[5] ELSE 0)

\* The first charge of cs after which the remaining budget, from r, is
\* negative, or 0 when every charge passes.
\* @type: (Int, Seq(Int)) => Int;
FirstFail(r, cs) ==
    IF Len(cs) >= 1 /\ RemAfter(r, cs, 1) < 0 THEN 1
    ELSE IF Len(cs) >= 2 /\ RemAfter(r, cs, 2) < 0 THEN 2
    ELSE IF Len(cs) >= 3 /\ RemAfter(r, cs, 3) < 0 THEN 3
    ELSE IF Len(cs) >= 4 /\ RemAfter(r, cs, 4) < 0 THEN 4
    ELSE IF Len(cs) >= 5 /\ RemAfter(r, cs, 5) < 0 THEN 5
    ELSE 0

\* RuntimeBudget::charge_legacy (accounting/mod.rs:871-890) over the charges
\* cs, in order, from the legacy state ls. A charge is rejected with no
\* entry when the remaining budget is negative before it (:876-878).
\* Otherwise it is subtracted and logged (:879-885), and it fails when the
\* remaining budget is negative after it (:886-888). The first failure
\* stops the list. ok is TRUE when every charge passes.
\* The LET names bind the parameters once: Apalache substitutes a parameter
\* at each of its uses, and a nullary LET keeps one copy.
\* @type: ($leg, Seq(Int)) => $lres;
ChargeList(ls, cs) ==
    LET L == ls
        C == cs
        f == FirstFail(L.rem, C)
        n == IF f = 0 THEN Len(C) ELSE f
    IN IF Len(C) = 0 THEN [ls |-> L, ok |-> TRUE]
       ELSE IF L.rem < 0 THEN [ls |-> L, ok |-> FALSE]
       ELSE [ls |-> [log |-> [i \in 1..MaxLeg |->
                                 IF L.len < i /\ i <= L.len + n THEN C[i - L.len]
                                 ELSE L.log[i]],
                     len |-> L.len + n,
                     rem |-> RemAfter(L.rem, C, n)],
             ok |-> f = 0]

\* The legacy charges of the space run: none in a token deploy.
\* @type: ($leg, Seq(Int)) => $lres;
LegCharge(ls, cs) ==
    LET L == ls
    IN IF Legacy THEN ChargeList(L, cs) ELSE [ls |-> L, ok |-> TRUE]

\* One operation of the equivalent host program, as ChargingRSpace runs it
\* (charging_rspace.rs:79-151): the pre-charge, then the host call, then the
\* post-charges. A failed pre-charge stops before the call, and a failed
\* post-charge stops after it. A failed program runs nothing more.
\* @type: ($leg, Int, Bool, Str) => $hres;
HostOp(ls, ops, failed, k) ==
    LET L == ls
        K == k
        a == ChargeList(L, <<PreCharge(K)>>)
        b == ChargeList(a.ls, PostList(PostKind(K)))
    IN IF failed THEN [ls |-> L, ops |-> ops, failed |-> TRUE]
       ELSE IF ~a.ok THEN [ls |-> a.ls, ops |-> ops, failed |-> TRUE]
       ELSE [ls |-> b.ls, ops |-> ops + 1, failed |-> ~b.ok]

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
\* (ops_consume.rs:179-190, hot_store.rs:720-783, 1017-1040).
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

\* @type: (Str, Bool, $comm, $vw, Bool, $vw) => $plan;
Plan(k1, fire1, m1, v1, two, v2) ==
    [k1 |-> k1, fire1 |-> fire1, m1 |-> m1, v1 |-> v1, two |-> two, v2 |-> v2]

\* The HostSteps of an agent operation, by shape (design report D.1). A plan
\* gives the kind k1 of the first HostStep, whether it fires, its COMM m1,
\* and the view v1 after it; then whether a re-store follows (two), and the
\* view v2 after the re-store (v2 = v1 when no re-store follows). n is the
\* identity nonce of the run.
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
    IN Plan("store", FALSE, NoComm, v1, FALSE, v1)

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
    IN Plan("pfire", TRUE, m, v1, restore,
            IF restore
            THEN VW(AddDatum(v1.st, c, p, 1), [cnt1 EXCEPT ![p] = @ + 1],
                    [v1.log EXCEPT ![vLen + 3] = PEv(p)], vLen + 3)
            ELSE v1)

\* A consume or a peek of op that fires the stored record p. Its COMM
\* removes the record (store_persistent_data, ops_consume.rs:192-202), and
\* for a peek (restore, Rule E) the re-store puts the same record back with
\* its identity (produce_peeks, reduce.rs:981-1016). The kind "kept" is the
\* peek fire of RetainPeek: the record stays, and no re-store follows.
\* @type: ($op, Int, $id, Str, Bool) => $plan;
PlanConsumeFire(op, n, p, k1, restore) ==
    LET c == op.ch
        q == <<c, op.k, n>>
        m == <<q, p, vCnt[p]>>
        v1 == VW(IF k1 = "kept" THEN vSt ELSE AddDatum(vSt, c, p, -1), vCnt,
                 [vLog EXCEPT ![vLen + 1] = CEv(q), ![vLen + 2] = MEv(m)], vLen + 2)
    IN Plan(k1, TRUE, m, v1, restore,
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
    IN Plan("park", FALSE, NoComm, v1, FALSE, v1)

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
(* The charges of a HostStep: in a token deploy, the observer charges      *)
(* ObsCost against vSpent; in a legacy deploy, the callback of the chain   *)
(* (LegacyChain::attach): the carry, then the pre-charge, against vLeg.    *)
(* The post kind of the HostStep becomes the carry.                        *)

\* Every operation of the transaction is staged. IF keeps TLC from reading
\* txn outside its domain.
StageDone == IF pos > MaxTxnOps THEN TRUE ELSE txn[pos] = Nop

\* The ghost host program runs the host operations of one agent operation
\* in a legacy deploy: the HostSteps of the plan, where a kept peek fire is
\* still the host's removing fire followed by the produce of produce_peeks.
\* @type: $plan => Bool;
HostStep(pl) ==
    LET h1 == HostOp(hLeg, hOps, hFailed, IF pl.k1 = "kept" THEN "cfire" ELSE pl.k1)
        h2 == IF pl.two \/ pl.k1 = "kept" THEN HostOp(h1.ls, h1.ops, h1.failed, "store") ELSE h1
    IN /\ hLeg' = IF Legacy THEN h2.ls ELSE hLeg
       /\ hOps' = IF Legacy THEN h2.ops ELSE hOps
       /\ hFailed' = IF Legacy THEN h2.failed ELSE hFailed

\* The apply operators below give each variable its new value by a case
\* analysis over named stop points, so that Apalache keeps one transition
\* for each shape of HostSteps.

\* The play HostSteps of one agent operation (ops_produce.rs:32-248,
\* ops_consume.rs:31-202), with their charges. stop1: a charge of the
\* first HostStep fails, and no HostStep runs. stop2: a charge of the
\* re-store fails, and only the first HostStep runs. LegacyPreAfterObserve
\* does not change play: in a legacy deploy the observer calls charge no
\* phlo, so a pre-charge after them makes the same charges in the same
\* order, before the events and the effects of its HostStep.
\* @type: $plan => Bool;
PlayApply(pl) ==
    LET c1 == ObsCost(pl.fire1)
        c2 == ObsCost(FALSE)
        r1 == LegCharge(vLeg, PostList(vCarry) \o <<PreCharge(pl.k1)>>)
        r2 == LegCharge(r1.ls, PostList(PostKind(pl.k1)) \o <<PreCharge("store")>>)
        stop1 == ~(r1.ok /\ vSpent + c1 <= Budget)
        stop2 == ~stop1 /\ pl.two /\ ~(r2.ok /\ vSpent + c1 + c2 <= Budget)
        v == IF stop1 THEN VW(vSt, vCnt, vLog, vLen) ELSE IF stop2 THEN pl.v1 ELSE pl.v2
    IN /\ HostStep(pl)
       /\ SetView(v)
       /\ vStop' = IF stop1 \/ stop2 THEN "phlo" ELSE vStop
       /\ vSpent' = CASE stop1 -> vSpent
                      [] stop2 \/ ~pl.two -> vSpent + c1
                      [] OTHER -> vSpent + c1 + c2
       /\ vLeg' = IF stop1 \/ ~pl.two THEN r1.ls ELSE r2.ls
       /\ vCarry' = CASE ~Legacy \/ stop1 \/ stop2 -> "none"
                      [] pl.two -> PostKind("store")
                      [] OTHER -> PostKind(pl.k1)
       /\ vOps' = CASE ~Legacy \/ stop1 -> vOps
                    [] stop2 \/ ~pl.two -> vOps + 1
                    [] OTHER -> vOps + 2
       /\ UNCHANGED <<vStaged, vOk>>

\* The replay HostSteps of one agent operation, re-stated over the staged
\* view (design §2.2, §2.4, decision 36). The callback makes its charges
\* first, and the observer charges follow. A fire whose charges fail stops
\* the commit there ("phlo"). Otherwise verify grants g pairings. A fire
\* with g = 0 stages unpaired: the commit stops there with
\* ReplayDivergence ("div"), after its charges pass. The unsafe rules:
\* StagingDivergence fails the staging of the transaction (vOk = FALSE,
\* the rule of decision 19), AcceptUnpaired stages the fire as if it
\* paired, so the commit applies and logs it, and LegacyPreAfterObserve
\* makes the pre-charge of a fire after its observer calls (late), so an
\* unpaired fire stops with ReplayDivergence before that pre-charge.
\* The stop points: drop (the staging fails), stop1 (a charge of the
\* callback or an observer charge fails), div (ReplayDivergence), stopLate
\* (the late pre-charge fails), stop2 (a charge of the re-store fails). At
\* the first four, no HostStep runs (none).
\* @type: $plan => Bool;
ReplayApply(pl) ==
    LET g == Grants(pl.m1, vStaged)
        unpaired == pl.fire1 /\ g = 0
        late == LegacyPreAfterObserve /\ pl.fire1
        c1 == ObsCost(pl.fire1)
        c2 == ObsCost(FALSE)
        r1 == LegCharge(vLeg, PostList(vCarry) \o (IF late THEN <<>> ELSE <<PreCharge(pl.k1)>>))
        r1b == IF late THEN LegCharge(r1.ls, <<PreCharge(pl.k1)>>) ELSE r1
        r2 == LegCharge(r1b.ls, PostList(PostKind(pl.k1)) \o <<PreCharge("store")>>)
        drop == unpaired /\ StagingDivergence
        stop1 == ~drop /\ ~(r1.ok /\ vSpent + c1 <= Budget)
        div == ~drop /\ ~stop1 /\ unpaired /\ ~AcceptUnpaired
        stopLate == ~drop /\ ~stop1 /\ ~div /\ ~r1b.ok
        none == drop \/ stop1 \/ div \/ stopLate
        stop2 == ~none /\ pl.two /\ ~(r2.ok /\ vSpent + c1 + c2 <= Budget)
        v == IF none THEN VW(vSt, vCnt, vLog, vLen) ELSE IF stop2 THEN pl.v1 ELSE pl.v2
    IN /\ UNCHANGED <<vOps, hLeg, hOps, hFailed>>
       /\ SetView(v)
       /\ vOk' = IF drop THEN FALSE ELSE vOk
       /\ vStaged' = IF none \/ ~pl.fire1 THEN vStaged ELSE [vStaged EXCEPT ![pl.m1] = @ + g]
       /\ vStop' = CASE stop1 \/ stopLate \/ stop2 -> "phlo"
                     [] div -> "div"
                     [] OTHER -> vStop
       /\ vSpent' = CASE drop \/ stop1 -> vSpent
                      [] div \/ stopLate \/ stop2 \/ ~pl.two -> vSpent + c1
                      [] OTHER -> vSpent + c1 + c2
       /\ vLeg' = CASE drop -> vLeg
                    [] stop1 \/ div -> r1.ls
                    [] stopLate \/ ~pl.two -> r1b.ls
                    [] OTHER -> r2.ls
       /\ vCarry' = CASE drop -> vCarry
                      [] ~Legacy \/ stop1 \/ div \/ stopLate \/ stop2 -> "none"
                      [] pl.two -> PostKind("store")
                      [] OTHER -> PostKind(pl.k1)

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
        fireRecord == PlanConsumeFire(op, n, p, "cfire", FALSE)
        peekRecord == PlanConsumeFire(op, n, p, "cfire", TRUE)
        keepRecord == PlanConsumeFire(op, n, p, "kept", FALSE)
        park == PlanPark(op, n)
    IN \/ op.kind = "produce" /\ WP /= {} /\ Apply(fireCont)
       \/ Peeks /\ op.kind = "produce" /\ WP = {} /\ WK /= {} /\ Apply(firePeekCont)
       \/ op.kind = "produce" /\ WP = {} /\ WK = {} /\ Apply(store)
       \/ op.kind = "consume" /\ A /= {} /\ Apply(fireRecord)
       \/ Peeks /\ ~RetainPeek /\ op.kind = "peek" /\ A /= {} /\ Apply(peekRecord)
       \/ RetainPeek /\ op.kind = "peek" /\ A /= {} /\ Apply(keepRecord)
       \/ op.kind /= "produce" /\ A = {} /\ Apply(park)

\* One play stage step, under the play counter rule. The ghost host program
\* runs the same operation while the view has not stopped.
PlayStage ==
    /\ phase = "play"
    /\ pos >= 1
    /\ ~StageDone
    /\ \/ /\ vStop /= "none"
          /\ UNCHANGED <<vSt, vCnt, vLog, vLen, vSpent, vStop, vLeg, vCarry, vOps, hLeg, hOps,
                         hFailed, vStaged, vOk>>
       \/ /\ vStop = "none"
          /\ StageNext(PlayNonce, PlayApply)
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk,
                   txn, pLeg, pOps, rLeg>>
    /\ UNCHANGED deployVars

\* One replay stage step, under the replay counter rule.
ReplayStage ==
    /\ phase = "replay"
    /\ pos >= 1
    /\ ~StageDone
    /\ vOk
    /\ \/ /\ vStop /= "none"
          /\ UNCHANGED <<vSt, vLog, vLen, vCnt, vStaged, vOk, vSpent, vStop, vLeg, vCarry, vOps,
                         hLeg, hOps, hFailed>>
       \/ /\ vStop = "none"
          /\ StageNext(ReplayNonce, ReplayApply)
    /\ pos' = pos + 1
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk, txn,
                   pLeg, pOps, rLeg>>
    /\ UNCHANGED deployVars

\* The tail of the chain (LegacyChain::finish): after the commit, the
\* post-charges of the last HostStep, when the commit applied every
\* HostStep. LegacyTailDropped omits it.
\* @type: $lres;
ChainTail == LegCharge(vLeg, IF vStop = "none" /\ ~LegacyTailDropped THEN PostList(vCarry) ELSE <<>>)

\* The commit fails the deploy at a stop "phlo" or at a failed tail. The
\* cost of a failed deploy is its limit. A legacy deploy otherwise costs
\* its limit minus its remaining budget (total_cost,
\* accounting/mod.rs:2584-2598).
\* @type: $lres => Bool;
CommitFails(t) == vStop = "phlo" \/ ~t.ok
\* @type: $lres => Int;
CommitCost(t) == IF CommitFails(t) THEN Budget ELSE IF Legacy THEN Budget - t.ls.rem ELSE vSpent

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
    /\ vLeg = InitLeg
    /\ vCarry = "none"
    /\ vOps = 0
    /\ pLeg = InitLeg
    /\ pOps = 0
    /\ rLeg = InitLeg
    /\ hLeg = InitLeg
    /\ hOps = 0
    /\ hFailed = FALSE

\* begin: the agent picks the operations of a transaction and the staged
\* view starts as the hot store (design §1.3). Each transaction has its own
\* chain, so the carry starts empty. A failed deploy begins no further
\* transaction.
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
    /\ vLeg' = pLeg
    /\ vCarry' = "none"
    /\ vOps' = pOps
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED legVars
    /\ UNCHANGED deployVars

\* play TxnCommit: the commit applies the fold of the staged HostSteps up to
\* the stop, and then the tail of the chain runs. At a rejected charge the
\* deploy fails, and the attempt counts, so the spending equals the limit
\* (casper/src/rust/rholang/runtime.rs:934-958 keeps the log of the failed
\* deploy).
PlayCommit ==
    /\ phase = "play"
    /\ pos >= 1
    /\ StageDone
    /\ LET t == ChainTail
       IN /\ pSpent' = CommitCost(t)
          /\ pFailed' = CommitFails(t)
          /\ pLeg' = t.ls
    /\ pStore' = vSt
    /\ pCount' = vCnt
    /\ pLog' = vLog
    /\ pLen' = vLen
    /\ pOps' = vOps
    /\ prog' = [prog EXCEPT ![pNtx + 1] = txn]
    /\ pNtx' = pNtx + 1
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, trie, preTrie, rTrie, cks, rStore, rCount, rLog, rLen, rTx, rd,
                   rigIO, rigCount, paired, rOk, txn, vSt, vCnt, vLog, vLen, vStaged, vOk,
                   vSpent, vStop, forged, rSpent, rFailed, vLeg, vCarry, vOps, rLeg, hLeg,
                   hOps, hFailed>>

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
    /\ UNCHANGED legVars
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
    /\ UNCHANGED legVars

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
    /\ rLeg' = InitLeg
    /\ phase' = "replay"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   pSpent, pFailed, forged, pLeg, pOps, hLeg, hOps, hFailed>>
    /\ UNCHANGED viewVars

\* begin of a replayed transaction: the same operations as in play, with a
\* chain of its own.
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
    /\ vLeg' = rLeg
    /\ vCarry' = "none"
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk, vOps>>
    /\ UNCHANGED legVars
    /\ UNCHANGED deployVars

\* replay TxnCommit. A transaction whose staging failed (vOk = FALSE, only
\* under StagingDivergence) is dropped and fails the replay. Otherwise the
\* commit applies the fold of the staged HostSteps up to the stop, runs the
\* tail of the chain, and removes the bindings of every pairing
\* (remove_bindings_for, replay_rspace.rs:1498-1526, saturating). A
\* rejected charge fails the deploy as in play. An unpaired fire fails the
\* replay with ReplayDivergence after the operations before it.
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
            /\ LET t == ChainTail
               IN /\ rSpent' = CommitCost(t)
                  /\ rFailed' = CommitFails(t)
                  /\ rLeg' = t.ls
            /\ rOk' = (vStop /= "div")
       ELSE /\ rOk' = FALSE
            /\ UNCHANGED <<rStore, rCount, rLog, rLen, rd, paired, rTx, rSpent, rFailed, rLeg>>
    /\ pos' = 0
    /\ UNCHANGED <<block, phase, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie,
                   cks, rigIO, rigCount, txn, vSt, vCnt, vLog, vLen, vStaged, vOk, vSpent, vStop,
                   pSpent, pFailed, forged, vLeg, vCarry, vOps, pLeg, pOps, hLeg, hOps, hFailed>>

\* A diverged replay rejects the block.
ReplayAbort ==
    /\ phase = "replay"
    /\ ~rOk
    /\ phase' = "done"
    /\ UNCHANGED <<block, pStore, pCount, pLog, pLen, prog, pNtx, trie, preTrie, rTrie, cks,
                   rStore, rCount, rLog, rLen, rTx, rd, rigIO, rigCount, paired, rOk>>
    /\ UNCHANGED viewVars
    /\ UNCHANGED legVars
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
    /\ UNCHANGED legVars
    /\ UNCHANGED deployVars

\* Checkpoint of the validator, then the next block on the canonical chain.
\* The deploy of the next block starts with the full limit, and its ghost
\* host program starts empty.
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
            /\ pLeg' = InitLeg
            /\ pOps' = 0
            /\ rLeg' = InitLeg
            /\ hLeg' = InitLeg
            /\ hOps' = 0
            /\ hFailed' = FALSE
            /\ phase' = "play"
       ELSE /\ phase' = "done"
            /\ UNCHANGED <<block, preTrie, pLog, pLen, prog, pNtx, rLog, rLen>>
            /\ UNCHANGED deployVars
            /\ UNCHANGED legVars
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
    /\ vLeg \in LegSet
    /\ vCarry \in Carries
    /\ vOps \in 0..MaxOps
    /\ pLeg \in LegSet
    /\ pOps \in 0..MaxOps
    /\ rLeg \in LegSet
    /\ hLeg \in LegSet
    /\ hOps \in 0..MaxOps
    /\ hFailed \in BOOLEAN

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
\* (casper/src/rust/rholang/replay_runtime.rs:619-632).
ReplayOutcomeEqual ==
    ~forged =>
        /\ rOk
        /\ (phase = "replaySwept" => (rFailed = pFailed /\ rSpent = pSpent))

\* U13 (decision 45, Option P): in a legacy deploy, whenever play has no open
\* transaction, play has the legacy log, the remaining budget and the stop
\* point of the equivalent host program: the same host operations ran, and
\* the deploy failed exactly when the program failed. A completed replay
\* has the legacy state of that program.
LegacyChargesFollowHost ==
    Legacy =>
        /\ ~(phase = "play" /\ pos >= 1) =>
              /\ pLeg.log = hLeg.log
              /\ pLeg.len = hLeg.len
              /\ pLeg.rem = hLeg.rem
              /\ pOps = hOps
              /\ pFailed = hFailed
        /\ phase = "replaySwept" => rLeg = hLeg

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

\* The legacy entries that one agent operation charges at most (a peek: five
\* for its fire, two for its re-store), and none in a token deploy.
LegPerOp == IF Legacy THEN (IF Peeks THEN 7 ELSE 5) ELSE 0
\* The agent operations of the open play transaction that are staged.
\* @type: Int;
OpenPlayOps == IF phase = "play" /\ pos >= 1 THEN pos - 1 ELSE 0

\* The legacy states keep the bounds of TypeOK. An agent operation charges
\* at most LegPerOp entries and applies at most StepsPerOp HostSteps. The
\* staged view still owes the carry, and the ghost host program has
\* charged it already. A run that has not failed has a remaining budget of
\* zero or more, so the cost of a commit stays within 0..Budget.
LegacyInv ==
    /\ pLeg.len <= LegPerOp * MaxTxnOps * pNtx
    /\ rLeg.len <= LegPerOp * MaxTxnOps * (rTx - 1)
    /\ hLeg.len <= LegPerOp * (MaxTxnOps * pNtx + OpenPlayOps)
    /\ pOps <= StepsPerOp * MaxTxnOps * pNtx
    /\ hOps <= StepsPerOp * (MaxTxnOps * pNtx + OpenPlayOps)
    /\ (phase = "play" /\ pos >= 1) =>
          /\ vLeg.len + Len(PostList(vCarry)) <= pLeg.len + LegPerOp * (pos - 1)
          /\ vOps <= pOps + StepsPerOp * (pos - 1)
    /\ (phase = "replay" /\ pos >= 1) =>
          vLeg.len + Len(PostList(vCarry)) <= rLeg.len + LegPerOp * (pos - 1)
    /\ (pos >= 1 /\ vStop /= "phlo") => vLeg.rem >= 0
    /\ ~pFailed => pLeg.rem >= 0
    /\ ~rFailed => rLeg.rem >= 0
    /\ ~hFailed => hLeg.rem >= 0

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
    /\ LegacyInv

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
    /\ vLeg \in LegSet
    /\ vCarry \in Carries
    /\ vOps \in 0..MaxOps
    /\ pLeg \in LegSet
    /\ pOps \in 0..MaxOps
    /\ rLeg \in LegSet
    /\ hLeg \in LegSet
    /\ hOps \in 0..MaxOps
    /\ hFailed \in BOOLEAN
    /\ IndInv
=============================================================================

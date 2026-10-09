---------------------------- MODULE NativeHandles ----------------------------
(***************************************************************************)
(* Native spaces, their handles and handle calls across blocks (P4).       *)
(*                                                                         *)
(* A factory call with the random state r creates the space sid = D(r)     *)
(* (I-19): it stores the config record on C(sid), opens the space counter  *)
(* S(sid) and parks the handle continuation on H(sid), a persistent        *)
(* single-channel receive. A second creation of one sid aborts with        *)
(* SpaceAlreadyExists. A handle call this!(...) runs one collection        *)
(* operation when C(sid) holds the config, and aborts with DanglingHandle  *)
(* otherwise (a fabricated H(sid) Par). A restart keeps every durable      *)
(* space. Two branches that call handles on disjoint collections merge     *)
(* like two contract calls: checks #1, #2 and #3 of HEAD find nothing.     *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   HandleUnique                one config record and one handle          *)
(*                               continuation for each created sid, none   *)
(*                               for the others                            *)
(*   DanglingIsDeterministicError a call aborts with DanglingHandle        *)
(*                               exactly when its space has no config      *)
(*   DurableSurvivesRestart      every space that existed at a restart     *)
(*                               still has its config and its handle       *)
(*   HandleCallsMergeLikeContracts two calls on disjoint collections of    *)
(*                               live spaces never conflict. Two calls on  *)
(*                               one opened collection race on its         *)
(*                               counter                                   *)
(*                                                                         *)
(* Unsafe control (refutes exactly HandleCallsMergeLikeContracts):         *)
(*   SingleChannelHandle = FALSE  JoinDescriptorUnsafe: the handle         *)
(*                               continuation joins H with a descriptor    *)
(*                               channel, so the pre-state join of H(sid)  *)
(*                               is [[H, Desc]]. Every produce on H then   *)
(*                               touches a join of length 2, and check #3  *)
(*                               reports a conflict for every pair of      *)
(*                               calls.                                    *)
(*                                                                         *)
(* Inductive check (Apalache): HandleIndInv holds the four invariants and  *)
(* the facts that make them inductive (live spaces are durable, only live  *)
(* spaces have collection counters, and no counter or log grows faster     *)
(* than the steps). BASE checks Init => HandleIndInv, STEP checks          *)
(* HandleIndInv /\ Next => HandleIndInv'. With SingleChannelHandle =       *)
(* FALSE, STEP must refute HandleIndInv in its                             *)
(* HandleCallsMergeLikeContracts conjunct (negative control).              *)
(*                                                                         *)
(* Code (the P4 commit, with the paths without a prefix under              *)
(* rholang/src/rust/interpreter,                                           *)
(* rs = rspace++/src/rspace):                                              *)
(*   Create               reified/factory.rs:75-121 (the factory call),    *)
(*                        spaces/types/id.rs:13-22 (derive_space_id),      *)
(*                        spaces/generic.rs:2703-2821 (prepare_create:     *)
(*                        SpaceAlreadyExists, config, S(sid), handle)      *)
(*   Call                 reified/handle.rs:221-276 (HandleProcess::       *)
(*                        call: read_config, DanglingHandle, the method)   *)
(*   the handle receive   reified/factory.rs:125-131, 135-160              *)
(*                        (handle_pattern, handle_continuation)            *)
(*   check #1, #2         rs/merger/merging_logic.rs:279-386, 388-429      *)
(*   check #3             rs/merger/merging_logic.rs:431-458               *)
(*   the base joins       casper/src/rust/merging/block_index.rs:67-71     *)
(*                        (produce_touches_pre_state_join: a join of       *)
(*                        length > 1)                                      *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Three sids: s1 = D(r1), s2 = D(r2), and s3, which no call derives   *)
(*     (a fabricated handle). D is injective (Blake2b in the model).       *)
(*   - Every space is durable: P4 creates only default spaces.             *)
(*   - A call is one produce on one collection. Its events are the user    *)
(*     produce on H, which the persistent handle continuation consumes,    *)
(*     the forwarder produce on HANDLE_OP, which the persistent install    *)
(*     consumes, the counter COMM of the collection, and the reply. Each   *)
(*     produce and each continuation of a call is new in its branch,       *)
(*     except the pre-state counter record and the two persistent          *)
(*     continuations.                                                      *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

(* Apalache type aliases. TLC reads these lines as comments.               *)
\* @typeAlias: entry = {sid: Str, outcome: Str, existed: Bool};
\* @typeAlias: ev = <<Str, Str, Str, Int, Str>>;
NativeHandles_aliases == TRUE

CONSTANTS
    \* TRUE: the handle continuation is a single-channel receive on H(sid).
    \* FALSE: the Unsafe join with a descriptor channel.
    \* @type: Bool;
    SingleChannelHandle,
    \* The number of steps of one run.
    \* @type: Int;
    MaxSteps

ASSUME SingleChannelHandle \in BOOLEAN
ASSUME MaxSteps \in 1..5

VARIABLES
    \* The config records on C(sid).
    \* @type: Str -> Int;
    config,
    \* The handle continuations parked on H(sid).
    \* @type: Str -> Int;
    handles,
    \* The collection counters S(sid,k): -1 before the first operation.
    \* @type: Str -> (Str -> Int);
    counters,
    \* The durability of each space.
    \* @type: Str -> Bool;
    durable,
    \* The handle calls, in order (slots 1..nlog).
    \* @type: Int -> $entry;
    clog,
    \* The number of handle calls so far.
    \* @type: Int;
    nlog,
    \* The spaces that existed at the last restart.
    \* @type: Set(Str);
    restarted,
    \* The number of steps so far.
    \* @type: Int;
    steps

vars == <<config, handles, counters, durable, clog, nlog, restarted, steps>>

Rands == {"r1", "r2"}
Sids == {"s1", "s2", "s3"}
Colls == {"k1", "k2"}
Slots == 1..MaxSteps
Outcomes == {"none", "reply", "DanglingHandle"}
NoEntry == [sid |-> "s3", outcome |-> "none", existed |-> FALSE]

\* derive_space_id: injective on the random states of the calls.
\* @type: (Str) => Str;
D(r) == IF r = "r1" THEN "s1" ELSE "s2"

\* @type: (Str) => Bool;
Live(s) == config[s] >= 1

Init ==
    /\ config = [s \in Sids |-> 0]
    /\ handles = [s \in Sids |-> 0]
    /\ counters = [s \in Sids |-> [k \in Colls |-> -1]]
    /\ durable = [s \in Sids |-> FALSE]
    /\ clog = [i \in Slots |-> NoEntry]
    /\ nlog = 0
    /\ restarted = {}
    /\ steps = 0

\* A factory call with the random state r (prepare_create).
\* @type: (Str) => Bool;
Create(r) ==
    LET s == D(r) IN
    /\ steps < MaxSteps
    /\ steps' = steps + 1
    /\ IF Live(s)
       THEN \* SpaceAlreadyExists: the deploy aborts and the store keeps its records
            UNCHANGED <<config, handles, counters, durable>>
       ELSE /\ config' = [config EXCEPT ![s] = 1]
            /\ handles' = [handles EXCEPT ![s] = 1]
            /\ durable' = [durable EXCEPT ![s] = TRUE]
            /\ UNCHANGED counters
    /\ UNCHANGED <<clog, nlog, restarted>>

\* A handle call on H(s) that produces into the collection k.
\* @type: (Str, Str) => Bool;
Call(s, k) ==
    /\ steps < MaxSteps
    /\ steps' = steps + 1
    /\ IF Live(s) /\ handles[s] >= 1
       THEN /\ counters' = [counters EXCEPT ![s][k] = IF @ < 0 THEN 1 ELSE @ + 1]
            /\ clog' = [clog EXCEPT ![nlog + 1] = [sid |-> s, outcome |-> "reply", existed |-> TRUE]]
       ELSE /\ clog' = [clog EXCEPT ![nlog + 1] = [sid |-> s, outcome |-> "DanglingHandle", existed |-> Live(s)]]
            /\ UNCHANGED counters
    /\ nlog' = nlog + 1
    /\ UNCHANGED <<config, handles, durable, restarted>>

\* A checkpoint and a restart: the durable records stay, the volatile ones
\* go.
Restart ==
    /\ steps < MaxSteps
    /\ steps' = steps + 1
    /\ restarted' = {s \in Sids : Live(s)}
    /\ config' = [s \in Sids |-> IF durable[s] THEN config[s] ELSE 0]
    /\ handles' = [s \in Sids |-> IF durable[s] THEN handles[s] ELSE 0]
    /\ counters' = [s \in Sids |-> IF durable[s] THEN counters[s] ELSE [k \in Colls |-> -1]]
    /\ UNCHANGED <<durable, clog, nlog>>

Next ==
    \/ \E r \in Rands : Create(r)
    \/ \E s \in Sids, k \in Colls : Call(s, k)
    \/ Restart

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* The events of one handle call, for checks #1 to #3                      *)

\* The produces that a call of branch b on (s, k) consumes without a peek:
\* the pre-state counter record of the collection (when it exists) and the
\* produces of the branch (the user produce on H, the forwarder produce on
\* HANDLE_OP and the reply). A tuple is <<kind, sid, coll, value, maker>>.
\* @type: (Str, Str, Str) => Set($ev);
Consumed(s, k, b) ==
    {<<"user", s, "-", 0, b>>, <<"ops", s, "-", 0, b>>, <<"reply", s, "-", 0, b>>}
    \cup (IF counters[s][k] >= 0 THEN {<<"S", s, k, counters[s][k], "base">>}
          ELSE {<<"SP", s, "-", 0, "base">>})

\* The non-persistent continuations that a call creates and COMMs: the
\* counter continuation of the branch. The handle continuation and the
\* HANDLE_OP install are persistent pre-state continuations.
\* @type: (Str, Str, Str) => Set($ev);
NonPersistentConsumes(s, k, b) == {<<"K", s, k, 0, b>>}

\* The produces of a call that touch a pre-state join of length > 1.
\* @type: (Str) => Set(Str);
TouchesBaseJoin(s) == IF SingleChannelHandle THEN {} ELSE {s}

\* Checks #1 and #3 of HEAD for two calls in two branches. Check #2 finds
\* nothing: no call leaves a consume that is created and not destroyed.
\* @type: (Str, Str, Str, Str) => Bool;
Conflict(s1, k1, s2, k2) ==
    \/ Consumed(s1, k1, "A") \cap Consumed(s2, k2, "B") /= {}
    \/ NonPersistentConsumes(s1, k1, "A") \cap NonPersistentConsumes(s2, k2, "B") /= {}
    \/ TouchesBaseJoin(s1) \cup TouchesBaseJoin(s2) /= {}

-----------------------------------------------------------------------------
(* Invariants                                                              *)

TypeOK ==
    /\ config \in [Sids -> 0..2]
    /\ handles \in [Sids -> 0..2]
    /\ counters \in [Sids -> [Colls -> -1..MaxSteps]]
    /\ durable \in [Sids -> BOOLEAN]
    /\ clog \in [Slots -> [sid : Sids, outcome : Outcomes, existed : BOOLEAN]]
    /\ nlog \in 0..MaxSteps
    /\ restarted \subseteq Sids
    /\ steps \in 0..MaxSteps

HandleUnique ==
    /\ \A s \in Sids : config[s] <= 1 /\ handles[s] = config[s]
    /\ config["s3"] = 0

DanglingIsDeterministicError ==
    \A i \in Slots : i <= nlog =>
        (clog[i].outcome = "DanglingHandle" <=> ~clog[i].existed)

DurableSurvivesRestart == \A s \in restarted : config[s] = 1 /\ handles[s] = 1

HandleCallsMergeLikeContracts ==
    \A s1, s2 \in Sids, k1, k2 \in Colls :
        (Live(s1) /\ Live(s2) /\ counters[s1][k1] >= 0 /\ counters[s2][k2] >= 0) =>
            (Conflict(s1, k1, s2, k2) <=> (s1 = s2 /\ k1 = k2))

\* The inductive strengthening for the Apalache BASE and STEP checks.
HandleIndInv ==
    /\ TypeOK
    /\ HandleUnique
    /\ DanglingIsDeterministicError
    /\ DurableSurvivesRestart
    /\ HandleCallsMergeLikeContracts
    /\ \A s \in Sids : Live(s) => durable[s]
    /\ \A s \in Sids, k \in Colls : counters[s][k] >= 0 => Live(s)
    /\ \A s \in Sids, k \in Colls : counters[s][k] <= steps
    /\ \A i \in Slots : i > nlog => clog[i] = NoEntry
    /\ nlog <= steps

\* The STEP initializer: every state within the bounds that satisfies
\* HandleIndInv.
IndInit ==
    /\ config \in [Sids -> 0..2]
    /\ handles \in [Sids -> 0..2]
    /\ counters \in [Sids -> [Colls -> -1..MaxSteps]]
    /\ durable \in [Sids -> BOOLEAN]
    /\ clog \in [Slots -> [sid : Sids, outcome : Outcomes, existed : BOOLEAN]]
    /\ nlog \in 0..MaxSteps
    /\ restarted \in SUBSET Sids
    /\ steps \in 0..MaxSteps
    /\ HandleIndInv

=============================================================================

-------------------------- MODULE TxnLockCoverage --------------------------
(***************************************************************************)
(* Lock coverage of Reified RSpaces substrate transactions (P1, FR-4).    *)
(*                                                                         *)
(* Space transactions and ordinary host operations share one hot store.   *)
(* A transaction takes the striped locks of its lock keys at begin. It    *)
(* stages operations against a staged view and decides each outcome as    *)
(* the host would decide it against that view. Commit applies the staged *)
(* outcomes and releases the locks. A host produce or consume runs        *)
(* atomically under its own stripes. The model explores every            *)
(* interleaving of begins, stages, commits and host operations.          *)
(*                                                                         *)
(* Invariant:                                                              *)
(*   T5 Serializable  the committed history equals a serial order: some   *)
(*                    order of the committed units (one unit for each     *)
(*                    transaction, one for each host operation) gives     *)
(*                    every recorded outcome under the host rule, and it  *)
(*                    ends in the current store.                          *)
(*                                                                         *)
(* FR-4 (LockCoverage::require, SpaceTxnError::UnlockedChannel): staging  *)
(* fails closed unless the transaction holds the stripe of every channel *)
(* that the operation touches, in the phase that the host uses for that  *)
(* channel. A produce needs phase A for its channel and phase B for every *)
(* channel of its join groups. A consume needs both phases for every     *)
(* channel of its group. begin takes both phases for every lock key      *)
(* (striped_locks::consume_lock), so one stripe set per transaction       *)
(* covers both phases.                                                    *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly T5):                             *)
(*   CheckCoverage = FALSE      a transaction may touch a channel without *)
(*                              holding its stripe (review F-LOCK-1).     *)
(*   CheckJoinCoverage = FALSE  a produce does not check the phase-B      *)
(*                              stripes of its join channels. Two         *)
(*                              transactions on disjoint stripes then     *)
(*                              each store one half of a join match.      *)
(*                                                                         *)
(* Inductive check (Apalache): LockIndInv says that the commit order is a *)
(* valid serial order so far (serialOk), that open transactions hold     *)
(* disjoint stripes, that every staged outcome is covered and valid      *)
(* against the store, and that each staged view agrees with the store on *)
(* the keys that its stripes protect. BASE checks Init => LockIndInv and *)
(* STEP checks LockIndInv /\ Next => LockIndInv'. With CheckCoverage =    *)
(* FALSE, STEP must refute LockIndInv (negative control). TLC checks     *)
(* LockIndInv and CommitOrderWitness on every reachable state of the     *)
(* safe configuration.                                                    *)
(*                                                                         *)
(* Code at the merge of feature/cost-accounted-rho 4d67b5ccc. Before       *)
(* the merge, the code was HEAD 6f7a80da3 (rs = rspace++/src/rspace):      *)
(*   stripes, phases      rs/striped_locks.rs:16-82 (acquire_locks,        *)
(*                        consume_lock), rs/rspace/locks.rs:24-56         *)
(*                        (produce_lock: phase A on the channel, phase B  *)
(*                        on the channels of its join groups)             *)
(*   host produce/consume rs/rspace/ops_produce.rs:32-248,                 *)
(*                        rs/rspace/ops_consume.rs:31-202,                 *)
(*                        rs/rspace/ispace_impl.rs:343-370                 *)
(* P1 substrate (new files, cited by item):                                *)
(*   Begin                SpaceTxnHost::begin (consume_lock of the lock   *)
(*                        keys), LockCoverage::new, HeldStripes           *)
(*   Covered, Stage       LockCoverage::require with LockUse::Produced,   *)
(*                        LockUse::Joined and LockUse::Consumed           *)
(*                        (TxnCore::stage_produce, TxnCore::admit_group)  *)
(*   Commit               HostTxn::commit, commit_staged                  *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Channels a and b have distinct stripes. The continuation keys are  *)
(*     the canonical groups of at most two channels: <<a>>, <<b>>,        *)
(*     <<a, b>>, and the groups <<a, a>> and <<b, b>> that repeat a       *)
(*     channel (FR-1': a canonical group is non-decreasing by hash and    *)
(*     can repeat a channel). Every pattern matches, every record is      *)
(*     non-persistent, and the store keeps counts. The join groups of a   *)
(*     channel are the groups with a waiting continuation (the lockstep   *)
(*     invariant of the hot store).                                       *)
(*   - A repeated channel binds one record for each of its positions: a   *)
(*     non-persistent record binds at most once per COMM, and the         *)
(*     incoming datum of a produce binds exactly one position of its      *)
(*     channel (check_incoming). Mult counts the positions. The stripe    *)
(*     set of a group is the stripe set of its channel set, because       *)
(*     acquire_locks sorts and deduplicates the stripe indices.           *)
(*   - A host operation is one atomic step that needs all of its stripes  *)
(*     free. The host takes them in sorted order, so the abstraction      *)
(*     omits waiting states and adds no behavior.                         *)
(*   - The staged view is the store at begin plus the staged effects. The *)
(*     code loads each key at its first touch. Under FR-4 no other actor  *)
(*     changes a covered key while the transaction is open, so the two   *)
(*     views agree on every key that staging reads.                       *)
(*   - A staged fire whose records are gone at commit stops the commit    *)
(*     there (Inconsistent). The operations before it stay applied        *)
(*     (decision D6). The commit-time state check of FR-7 adds no        *)
(*     behavior: it reports a difference after the effect.               *)
(*   - The bounds keep every fold explicit: at most two staged operations *)
(*     per transaction, at most four committed units, and at most eight   *)
(*     committed operations. The module has no recursive operator,        *)
(*     because Apalache does not accept one.                              *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

(* Apalache type aliases. TLC reads these lines as comments.               *)
\* @typeAlias: op = {kind: Str, tgt: Seq(Str)};
\* @typeAlias: out = {res: Str, grp: Seq(Str)};
\* @typeAlias: sop = {op: $op, out: $out};
\* @typeAlias: store = {data: Str -> Int, conts: Seq(Str) -> Int};
\* @typeAlias: run = {ok: Bool, st: $store};
\* @typeAlias: commit = {st: $store, applied: Seq($sop)};
TxnLockCoverage_aliases == TRUE

CONSTANTS
    \* The transaction identifiers.
    \* @type: Set(Str);
    Txns,
    \* The number of transactions (Apalache needs constant integer ranges).
    \* @type: Int;
    NumTxns,
    \* The staged operations of one transaction.
    \* @type: Int;
    MaxTxnOps,
    \* The host operations of one run.
    \* @type: Int;
    MaxHostOps,
    \* TRUE: FR-4 checks lock coverage at staging. FALSE: an Unsafe control.
    \* @type: Bool;
    CheckCoverage,
    \* TRUE: a produce checks the phase-B stripes of its join channels.
    \* @type: Bool;
    CheckJoinCoverage

ASSUME NumTxns = Cardinality(Txns)
ASSUME MaxTxnOps \in 1..2
ASSUME MaxHostOps \in 0..2
ASSUME NumTxns + MaxHostOps <= 4
ASSUME (NumTxns + MaxHostOps) * MaxTxnOps <= 8
ASSUME CheckCoverage \in BOOLEAN
ASSUME CheckJoinCoverage \in BOOLEAN

VARIABLES
    \* The hot store: data counts per channel, continuation counts per group.
    \* @type: $store;
    store,
    \* "idle", "open" or "done" for each transaction.
    \* @type: Str -> Str;
    tstate,
    \* The stripes that each open transaction holds (both phases).
    \* @type: Str -> Set(Int);
    held,
    \* The staged view of each open transaction.
    \* @type: Str -> $store;
    vstore,
    \* The staged operations of each transaction (slots 1..slen[t]).
    \* @type: Str -> (Int -> $sop);
    staged,
    \* The number of staged operations of each transaction.
    \* @type: Str -> Int;
    slen,
    \* The committed units in commit order.
    \* @type: Seq(Seq($sop));
    hist,
    \* The number of host operations so far.
    \* @type: Int;
    hostOps,
    \* Ghost: every committed unit was valid under the host rule at its
    \* commit, so the commit order is a serial order.
    \* @type: Bool;
    serialOk

vars == <<store, tstate, held, vstore, staged, slen, hist, hostOps, serialOk>>

-----------------------------------------------------------------------------
(* Channels, groups and stripes                                             *)

Chans == {"a", "b"}
\* The canonical groups of at most two channels (FR-1'): non-decreasing by
\* hash, so <<"a", "a">> and <<"b", "b">> repeat a channel.
\* @type: Set(Seq(Str));
Groups == {<<"a">>, <<"b">>, <<"a", "b">>, <<"a", "a">>, <<"b", "b">>}
Stripes == {1, 2}
\* @type: (Str) => Int;
Stripe(c) == IF c = "a" THEN 1 ELSE 2
\* @type: (Seq(Str)) => Set(Str);
GroupChans(g) == {g[i] : i \in DOMAIN g}
\* @type: (Set(Str)) => Set(Int);
StripesOf(S) == {Stripe(c) : c \in S}
\* The number of positions of channel x in group g. Every group has one or
\* two channels.
\* @type: (Seq(Str), Str) => Int;
Mult(g, x) == (IF g[1] = x THEN 1 ELSE 0) + (IF Len(g) >= 2 /\ g[2] = x THEN 1 ELSE 0)
\* The stored records of channel x that a fire of group g binds. A produce
\* on channel c binds its incoming datum at one position of c.
\* @type: (Seq(Str), Str, Str) => Int;
Need(g, x, c) == Mult(g, x) - (IF x = c THEN 1 ELSE 0)

\* An operation: a produce on the channel tgt[1], or a consume on the group
\* tgt.
\* @type: Set($op);
Ops == {[kind |-> "produce", tgt |-> <<c>>] : c \in Chans}
       \cup {[kind |-> "consume", tgt |-> g] : g \in Groups}

\* An outcome: store, park, or fire with the group that fires.
\* @type: Set($out);
Outcomes == [res : {"store", "park", "fire"}, grp : Groups]

\* @type: Set($sop);
StagedOps == [op : Ops, out : Outcomes]

\* The value of an unused staged slot.
\* @type: $sop;
NoOp == [op |-> [kind |-> "produce", tgt |-> <<"a">>], out |-> [res |-> "store", grp |-> <<"a">>]]

\* @type: $store;
Empty == [data |-> [c \in Chans |-> 0], conts |-> [g \in Groups |-> 0]]

-----------------------------------------------------------------------------
(* The host rule                                                            *)

\* The join groups of a channel: the groups with a waiting continuation.
\* @type: ($store, Str) => Set(Seq(Str));
JoinsOf(s, c) == {g \in Groups : c \in GroupChans(g) /\ s.conts[g] > 0}

\* A produce on c fires the continuation of g when every other position of
\* g has a stored record.
\* @type: ($store, Str, Seq(Str)) => Bool;
ProduceFires(s, c, g) ==
    /\ g \in JoinsOf(s, c)
    /\ \A x \in GroupChans(g) : s.data[x] >= Need(g, x, c)

\* A consume on g fires when every position of g has a stored record.
\* @type: ($store, Seq(Str)) => Bool;
ConsumeFires(s, g) == \A x \in GroupChans(g) : s.data[x] >= Mult(g, x)

\* The host rule: the outcome is one that the host can give op in s.
\* @type: ($store, $op, $out) => Bool;
HostAllows(s, op, out) ==
    IF op.kind = "produce"
    THEN IF \E g \in Groups : ProduceFires(s, op.tgt[1], g)
         THEN out.res = "fire" /\ ProduceFires(s, op.tgt[1], out.grp)
         ELSE out.res = "store" /\ out.grp = op.tgt
    ELSE IF ConsumeFires(s, op.tgt)
         THEN out.res = "fire" /\ out.grp = op.tgt
         ELSE out.res = "park" /\ out.grp = op.tgt

\* The effect of an outcome. A fire removes the records that it binds: one
\* record for each other position of the group (produce) or for each
\* position (consume). A produce fire removes the waiting continuation.
\* @type: ($store, $op, $out) => $store;
Apply(s, op, out) ==
    IF op.kind = "produce"
    THEN IF out.res = "store"
         THEN [data |-> [x \in Chans |-> IF x = op.tgt[1] THEN s.data[x] + 1 ELSE s.data[x]],
               conts |-> s.conts]
         ELSE [data |-> [x \in Chans |->
                           IF x \in GroupChans(out.grp)
                           THEN s.data[x] - Need(out.grp, x, op.tgt[1]) ELSE s.data[x]],
               conts |-> [g \in Groups |-> IF g = out.grp THEN s.conts[g] - 1 ELSE s.conts[g]]]
    ELSE IF out.res = "park"
         THEN [data |-> s.data,
               conts |-> [g \in Groups |-> IF g = op.tgt THEN s.conts[g] + 1 ELSE s.conts[g]]]
         ELSE [data |-> [x \in Chans |->
                           IF x \in GroupChans(op.tgt) THEN s.data[x] - Mult(op.tgt, x) ELSE s.data[x]],
               conts |-> s.conts]

\* The records that an outcome removes are present.
\* @type: ($store, $op, $out) => Bool;
Applicable(s, op, out) ==
    IF out.res /= "fire" THEN TRUE
    ELSE IF op.kind = "produce"
         THEN /\ s.conts[out.grp] > 0
              /\ \A x \in GroupChans(out.grp) : s.data[x] >= Need(out.grp, x, op.tgt[1])
         ELSE \A x \in GroupChans(op.tgt) : s.data[x] >= Mult(op.tgt, x)

\* One step of a serial run under the host rule.
\* @type: ($run, $sop) => $run;
SerialStep(r, x) ==
    IF r.ok /\ HostAllows(r.st, x.op, x.out)
    THEN [ok |-> TRUE, st |-> Apply(r.st, x.op, x.out)]
    ELSE [ok |-> FALSE, st |-> r.st]

-----------------------------------------------------------------------------
(* Locks                                                                    *)

\* @type: Set(Int);
AllHeld == UNION {held[t] : t \in Txns}

\* FR-4: the stripes that an operation needs, in the state s of staging.
\* @type: (Str, $store, $op) => Bool;
CoveredAt(t, s, op) ==
    IF op.kind = "produce"
    THEN /\ Stripe(op.tgt[1]) \in held[t]
         /\ CheckJoinCoverage =>
               \A g \in JoinsOf(s, op.tgt[1]) : StripesOf(GroupChans(g)) \subseteq held[t]
    ELSE StripesOf(GroupChans(op.tgt)) \subseteq held[t]

\* The stripes that a host operation takes (produce_lock, consume_lock).
\* @type: ($store, $op) => Set(Int);
HostStripes(s, op) ==
    IF op.kind = "produce"
    THEN {Stripe(op.tgt[1])} \cup UNION {StripesOf(GroupChans(g)) : g \in JoinsOf(s, op.tgt[1])}
    ELSE StripesOf(GroupChans(op.tgt))

-----------------------------------------------------------------------------
(* Actions                                                                  *)

Init ==
    /\ store = Empty
    /\ tstate = [t \in Txns |-> "idle"]
    /\ held = [t \in Txns |-> {}]
    /\ vstore = [t \in Txns |-> Empty]
    /\ staged = [t \in Txns |-> [i \in 1..MaxTxnOps |-> NoOp]]
    /\ slen = [t \in Txns |-> 0]
    /\ hist = <<>>
    /\ hostOps = 0
    /\ serialOk = TRUE

\* begin(lock_keys): both phases of every lock key, once all are free.
\* @type: (Str) => Bool;
Begin(t) ==
    /\ tstate[t] = "idle"
    /\ \E K \in SUBSET Chans :
          /\ K /= {}
          /\ StripesOf(K) \cap AllHeld = {}
          /\ held' = [held EXCEPT ![t] = StripesOf(K)]
    /\ tstate' = [tstate EXCEPT ![t] = "open"]
    /\ vstore' = [vstore EXCEPT ![t] = store]
    /\ UNCHANGED <<store, staged, slen, hist, hostOps, serialOk>>

\* Close a transaction: release its stripes and forget its view and slots.
\* @type: (Str) => Bool;
Close(t) ==
    /\ tstate' = [tstate EXCEPT ![t] = "done"]
    /\ held' = [held EXCEPT ![t] = {}]
    /\ vstore' = [vstore EXCEPT ![t] = Empty]
    /\ staged' = [staged EXCEPT ![t] = [i \in 1..MaxTxnOps |-> NoOp]]
    /\ slen' = [slen EXCEPT ![t] = 0]

\* Stage one operation with an outcome that the host rule allows in the
\* view. Without coverage, FR-4 fails the transaction closed
\* (UnlockedChannel): no effect, and the locks are released.
\* @type: (Str) => Bool;
Stage(t) ==
    /\ tstate[t] = "open"
    /\ slen[t] < MaxTxnOps
    /\ \E op \in Ops, out \in Outcomes :
          /\ HostAllows(vstore[t], op, out)
          /\ IF CheckCoverage /\ ~CoveredAt(t, vstore[t], op)
             THEN /\ Close(t)
                  /\ UNCHANGED <<store, hist, hostOps, serialOk>>
             ELSE /\ staged' = [staged EXCEPT ![t][slen[t] + 1] = [op |-> op, out |-> out]]
                  /\ slen' = [slen EXCEPT ![t] = slen[t] + 1]
                  /\ vstore' = [vstore EXCEPT ![t] = Apply(vstore[t], op, out)]
                  /\ UNCHANGED <<store, tstate, held, hist, hostOps, serialOk>>

\* Commit applies the staged outcomes in order. It stops at the first
\* outcome whose records are gone. The outcomes before it stay applied.
\* serialOk records whether every applied outcome is one that the host rule
\* gives at its point. The singleton existentials name the intermediate
\* stores once, so Apalache does not copy their expressions.
\* @type: (Str) => Bool;
Commit(t) ==
    /\ tstate[t] = "open"
    /\ slen[t] > 0
    /\ LET x1 == staged[t][1]
           x2 == staged[t][2]
       IN \E s1 \in {Apply(store, x1.op, x1.out)} :
          \E s2 \in {Apply(s1, x2.op, x2.out)} :
             LET ok1 == Applicable(store, x1.op, x1.out)
                 ok2 == slen[t] >= 2 /\ Applicable(s1, x2.op, x2.out)
             IN /\ store' = IF ~ok1 THEN store ELSE IF ~ok2 THEN s1 ELSE s2
                /\ hist' = IF ~ok1 THEN hist
                           ELSE IF ~ok2 THEN Append(hist, <<x1>>)
                           ELSE Append(hist, <<x1, x2>>)
                /\ serialOk' = (/\ serialOk
                                /\ ok1 => HostAllows(store, x1.op, x1.out)
                                /\ (ok1 /\ ok2) => HostAllows(s1, x2.op, x2.out))
    /\ Close(t)
    /\ UNCHANGED hostOps

\* A host produce or consume: atomic under its stripes, with an outcome
\* that the host rule allows in the store.
HostOp ==
    /\ hostOps < MaxHostOps
    /\ \E op \in Ops, out \in Outcomes :
          /\ HostStripes(store, op) \cap AllHeld = {}
          /\ HostAllows(store, op, out)
          /\ store' = Apply(store, op, out)
          /\ hist' = Append(hist, <<[op |-> op, out |-> out]>>)
    /\ hostOps' = hostOps + 1
    /\ UNCHANGED <<tstate, held, vstore, staged, slen, serialOk>>

Next ==
    \/ \E t \in Txns : Begin(t) \/ Stage(t) \/ Commit(t)
    \/ HostOp

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                               *)

\* Every operation adds at most one record or continuation.
Budget == NumTxns * MaxTxnOps + MaxHostOps
\* @type: Set($store);
StoreSet == [data : [Chans -> 0..Budget], conts : [Groups -> 0..Budget]]

MaxUnits == NumTxns + MaxHostOps

\* A store with counts in 0..Budget. The membership tests are written per
\* element, because Apalache does not expand a set of functions.
\* @type: ($store) => Bool;
StoreOK(s) ==
    /\ DOMAIN s.data = Chans
    /\ DOMAIN s.conts = Groups
    /\ \A c \in Chans : s.data[c] \in 0..Budget
    /\ \A g \in Groups : s.conts[g] \in 0..Budget

\* A staged view: natural counts. Its unprotected keys can be stale, so
\* only the store has the inductive bound.
\* @type: ($store) => Bool;
ViewOK(s) ==
    /\ DOMAIN s.data = Chans
    /\ DOMAIN s.conts = Groups
    /\ \A c \in Chans : s.data[c] >= 0
    /\ \A g \in Groups : s.conts[g] >= 0

\* @type: ($sop) => Bool;
StagedOpOK(x) ==
    /\ x.op \in Ops
    /\ x.out.res \in {"store", "park", "fire"}
    /\ x.out.grp \in Groups

TypeOK ==
    /\ StoreOK(store)
    /\ DOMAIN tstate = Txns /\ DOMAIN held = Txns /\ DOMAIN vstore = Txns
    /\ DOMAIN staged = Txns /\ DOMAIN slen = Txns
    /\ \A t \in Txns :
          /\ tstate[t] \in {"idle", "open", "done"}
          /\ held[t] \subseteq Stripes
          /\ ViewOK(vstore[t])
          /\ DOMAIN staged[t] = 1..MaxTxnOps
          /\ \A i \in 1..MaxTxnOps : StagedOpOK(staged[t][i])
          /\ slen[t] \in 0..MaxTxnOps
    /\ Len(hist) <= MaxUnits
    /\ \A i \in 1..MaxUnits :
          i <= Len(hist) =>
             /\ Len(hist[i]) \in 1..MaxTxnOps
             /\ \A j \in 1..MaxTxnOps : j <= Len(hist[i]) => StagedOpOK(hist[i][j])
    /\ hostOps \in 0..MaxHostOps
    /\ serialOk \in BOOLEAN

\* The permutations of 1..n, for every n up to MaxUnits (a constant table).
\* @type: Int -> Set(Int -> Int);
PermTable == [n \in 0..MaxUnits |->
                {f \in [1..n -> 1..n] : \A i, j \in 1..n : i /= j => f[i] /= f[j]}]

\* The operations of the committed units in the order perm (MaxUnits <= 4).
\* @type: (Int -> Int) => Seq($sop);
Flatten(perm) ==
    LET \* @type: (Int) => Seq($sop);
        U(i) == IF i <= Len(hist) THEN hist[perm[i]] ELSE <<>>
    IN U(1) \o U(2) \o U(3) \o U(4)

\* One step of a serial run at position i of the operation sequence f.
\* @type: ($run, Seq($sop), Int) => $run;
StepAt(r, f, i) == IF i <= Len(f) THEN SerialStep(r, f[i]) ELSE r

\* The serial run of f from the empty store gives every recorded outcome
\* under the host rule and ends in the current store. The flattened
\* sequence has at most eight operations. The singleton existentials name
\* each intermediate run once.
\* @type: (Seq($sop)) => Bool;
SerialRunOK(f) ==
    \E r1 \in {StepAt([ok |-> TRUE, st |-> Empty], f, 1)} :
    \E r2 \in {StepAt(r1, f, 2)} :
    \E r3 \in {StepAt(r2, f, 3)} :
    \E r4 \in {StepAt(r3, f, 4)} :
    \E r5 \in {StepAt(r4, f, 5)} :
    \E r6 \in {StepAt(r5, f, 6)} :
    \E r7 \in {StepAt(r6, f, 7)} :
    \E r8 \in {StepAt(r7, f, 8)} :
       r8.ok /\ r8.st = store

\* T5: the committed history equals a serial order of its units.
Serializable ==
    \E perm \in PermTable[Len(hist)] : SerialRunOK(Flatten(perm))

\* Auxiliary: when serialOk holds, the commit order itself is the witness.
CommitOrderWitness ==
    serialOk => SerialRunOK(Flatten([i \in 1..MaxUnits |-> i]))

-----------------------------------------------------------------------------
(* Inductive invariant for the Apalache BASE and STEP checks.               *)

\* The keys that the stripes of t protect: no other actor writes them while
\* t is open.
\* @type: (Str, Str) => Bool;
ProtData(t, c) == Stripe(c) \in held[t]
\* @type: (Str, Seq(Str)) => Bool;
ProtConts(t, g) == StripesOf(GroupChans(g)) \cap held[t] /= {}

\* An open transaction: every staged outcome is covered and allowed by the
\* host rule, in order, from the store, and the staged view agrees with the
\* store after the staged outcomes on every protected key.
\* @type: (Str) => Bool;
OpenInv(t) ==
    LET x1 == staged[t][1]
        x2 == staged[t][2]
    IN \E stagedOne \in {Apply(store, x1.op, x1.out)} :
       \E stagedTwo \in {Apply(stagedOne, x2.op, x2.out)} :
          LET after == IF slen[t] >= 2 THEN stagedTwo ELSE IF slen[t] = 1 THEN stagedOne ELSE store
          IN /\ slen[t] >= 1 => CoveredAt(t, store, x1.op) /\ HostAllows(store, x1.op, x1.out)
             /\ slen[t] >= 2 => CoveredAt(t, stagedOne, x2.op) /\ HostAllows(stagedOne, x2.op, x2.out)
             /\ \A c \in Chans : ProtData(t, c) => vstore[t].data[c] = after.data[c]
             /\ \A g \in Groups : ProtConts(t, g) => vstore[t].conts[g] = after.conts[g]

\* Only open transactions hold stripes and staged operations.
ShapeInv ==
    \A t \in Txns :
        /\ tstate[t] /= "open" => held[t] = {} /\ slen[t] = 0
        /\ tstate[t] = "open" => held[t] /= {}

LocksDisjoint == \A t, u \in Txns : t /= u => held[t] \cap held[u] = {}

\* The operations that can still reach the store: the host operations left,
\* and MaxTxnOps for each transaction that is not done. Each one adds at
\* most one record or continuation, so the bounds of TypeOK are inductive.
Pending == (MaxHostOps - hostOps) + MaxTxnOps * Cardinality({t \in Txns : tstate[t] /= "done"})

BudgetInv ==
    /\ \A c \in Chans : store.data[c] + Pending <= Budget
    /\ \A g \in Groups : store.conts[g] + Pending <= Budget

LockIndInv ==
    /\ TypeOK
    /\ serialOk
    /\ ShapeInv
    /\ LocksDisjoint
    /\ BudgetInv
    /\ \A t \in Txns : tstate[t] = "open" => OpenInv(t)

\* The STEP initializer: every state that satisfies LockIndInv. No action
\* and no conjunct of LockIndInv reads hist, so the initializer fixes hist
\* to the empty sequence without loss of generality.
IndInit ==
    /\ store \in StoreSet
    /\ tstate \in [Txns -> {"idle", "open", "done"}]
    /\ held \in [Txns -> SUBSET Stripes]
    /\ \E vd \in [Txns -> [Chans -> 0..Budget]], vc \in [Txns -> [Groups -> 0..Budget]] :
          vstore = [t \in Txns |-> [data |-> vd[t], conts |-> vc[t]]]
    /\ staged \in [Txns -> [1..MaxTxnOps -> StagedOps]]
    /\ slen \in [Txns -> 0..MaxTxnOps]
    /\ hist = <<>>
    /\ hostOps \in 0..MaxHostOps
    /\ serialOk \in BOOLEAN
    /\ LockIndInv

=============================================================================

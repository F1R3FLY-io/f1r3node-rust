-------------------------- MODULE SpaceCoordination --------------------------
(***************************************************************************)
(* The coordination rules of P7 (design v3, W1 and W3): the claims of a    *)
(* seq space, the send rule of seq names, the qualifier gate, and the use  *)
(* context.                                                                *)
(*                                                                         *)
(* Participants form a tree. A split replaces a participant by two         *)
(* children (F26). A participant that waits for a job does not split. A    *)
(* seq space has the channels x and y. A program job of participant p      *)
(* uses one channel c: it checks the claim of c, and later it commits or   *)
(* fails. The check admits the use when c has no claim or its claim is a   *)
(* prefix of p (SQ-3). A refused check aborts the deploy (SeqConcurrent).  *)
(* A commit makes p the claim of c and records the use (SQ-4). A failed    *)
(* job aborts the deploy and changes no claim. The scheduler runs the jobs *)
(* of one space one at a time between their check and their commit (the    *)
(* atomic guard, F11). An internal job (a re-issue or a settle job) uses   *)
(* no channel, so the model has no step for it (A3).                       *)
(*                                                                         *)
(* A send of a plain message delivers its names to a process that can     *)
(* receive them. A handle produce delivers its channel argument (a name of *)
(* its own space, which is exempt) and its message to the handle process.  *)
(* The scan of SQ-2 aborts a send whose message holds a seq name. A        *)
(* factory call of an outer store, a qualifier and an execution model      *)
(* creates a space when gate 3 accepts it (QT-4, QT-5). A use block runs   *)
(* its body in a child with the context S, and the rest of the process in  *)
(* the other child with the context of the parent.                         *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   SeqExclusive       for each channel, the admitted uses form one       *)
(*                      prefix chain of participant paths                  *)
(*   ClaimTracksLastUse the claim of a channel is its last admitted use    *)
(*   SeqNotSent         no delivered message holds a seq name              *)
(*   QualifierSupport   a HashSet space is a seq space, and a seq space    *)
(*                      exists only in native execution                    *)
(*   UseBalance         the context of each participant is its lexical     *)
(*                      context                                            *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   AtomicGuard = FALSE   SeqGuardToctouUnsafe: two jobs check one claim  *)
(*                         before either commits -> SeqExclusive           *)
(*   ScanEnabled = FALSE   SeqScanSkippedUnsafe: route_send does not scan  *)
(*                         -> SeqNotSent                                   *)
(*   NativeOnlySeq = FALSE QualifierSupportUnsafe: gate 3 accepts seq in   *)
(*                         legacy execution -> QualifierSupport            *)
(*   UseRestores = FALSE   UseBalanceUnsafe: the use context leaks into    *)
(*                         the rest of the process -> UseBalance           *)
(*                                                                         *)
(* Code (paths under rholang/src/rust/interpreter):                        *)
(*   claims        spaces/ephemeral.rs:207-231 (SeqClaims::open),          *)
(*                 236-270 (claim), 274 (store)                            *)
(*   the uses      spaces/generic.rs:1162-1168 (Cx::claim), 1181-1203,      *)
(*                 1220-1246, 1287-1320, 1353-1396, 1427-1461 (store_on,   *)
(*                 park_on, fire_offer_on, fire_receive_group,             *)
(*                 fire_internal), spaces/join.rs:722-760 (own_claims)     *)
(*   check, commit spaces/generic.rs:2219-2273 (run_plan),                 *)
(*                 2544-2704 (run_with): the claims open with the plan     *)
(*                 and are stored after a successful commit                *)
(*   internal jobs reified/reducer_hooks.rs:1249 (the re-issue runs with    *)
(*                 OpOrigin::Internal); SpaceOp::Settle is internal by its *)
(*                 variant                                                 *)
(*   atomic guard  deterministic_reduction.rs:787-793 (frontier_ready),    *)
(*                 reified/schedule.rs:84-97 (the footprint of a space     *)
(*                 job holds the key of its space)                         *)
(*   the scan      reified/seq.rs:77-101 (check_send), 156-176 (scan),     *)
(*                 reified/reducer_hooks.rs:546-682 (route_send)           *)
(*   gate 3        spaces/factory/constructor.rs:93-178 (validate_in)      *)
(*   use context   reified/scope.rs:185-193 (wrap_body),                   *)
(*                 reified/reducer_hooks.rs:281-311 (eval_use)             *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - A participant path is a sequence over {1, 2}: the index of each     *)
(*     split child. The code appends the segment (step, index + 1), and    *)
(*     the claims compare whole segments, so the prefix order is the same. *)
(*   - Sequential uses of one participant share its path, so the model    *)
(*     needs no separate step counter for them.                           *)
(*   - A message is the set of its names: the seq names nx and ny of the   *)
(*     space, and the ordinary name o.                                     *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* TRUE: the jobs of one space run one at a time between their check and
    \* their commit.
    AtomicGuard,
    \* TRUE: route_send scans the message of every send.
    ScanEnabled,
    \* TRUE: gate 3 refuses seq in legacy execution.
    NativeOnlySeq,
    \* TRUE: the use block keeps the context of the rest of the process.
    UseRestores,
    \* The depth bound of the participant tree.
    MaxDepth,
    \* The number of steps of one run.
    MaxSteps

ASSUME AtomicGuard \in BOOLEAN /\ ScanEnabled \in BOOLEAN
ASSUME NativeOnlySeq \in BOOLEAN /\ UseRestores \in BOOLEAN
ASSUME MaxDepth \in 1..3 /\ MaxSteps \in 1..8

Chans == {"x", "y"}
SeqNames == {"nx", "ny"}
Names == SeqNames \cup {"o"}
Parts == UNION {[1..n -> {1, 2}] : n \in 0..MaxDepth}
Contexts == {"default", "S"}

PlainMessages == {{"o"}, {"nx"}, {"o", "ny"}}
HandleArguments == {"nx"}
HandleMessages == {{}, {"o"}, {"ny"}}
Deliveries == [kind : {"plain", "handle"}, arg : Names \cup {"none"}, names : SUBSET Names]

Outers == {"hashmap", "hashset"}
Qualifiers == {"default", "seq"}
Models == {"legacy", "native"}
Spaces == [outer : Outers, q : Qualifiers, model : Models]

IsPrefix(a, b) == Len(a) <= Len(b) /\ SubSeq(b, 1, Len(a)) = a

VARIABLES
    parts,      \* the live participants
    claimed,    \* the channels with a claim
    claim,      \* the claim of each claimed channel
    pending,    \* the jobs between their check and their commit
    uses,       \* the admitted uses of each channel, in commit order
    delivered,  \* the delivered messages
    spaces,     \* the created spaces
    ctx,        \* the context of each participant
    lex,        \* the lexical context of each participant
    aborted,    \* TRUE when the deploy aborted
    steps       \* the number of steps so far

vars == <<parts, claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted, steps>>

TypeOK ==
    /\ parts \subseteq Parts
    /\ claimed \subseteq Chans
    /\ claim \in [Chans -> Parts]
    /\ pending \subseteq (Parts \X Chans)
    /\ uses \in [Chans -> Seq(Parts)]
    /\ delivered \subseteq Deliveries
    /\ spaces \subseteq Spaces
    /\ ctx \in [Parts -> Contexts]
    /\ lex \in [Parts -> Contexts]
    /\ aborted \in BOOLEAN
    /\ steps \in 0..MaxSteps

Init ==
    /\ parts = {<<>>}
    /\ claimed = {}
    /\ claim = [c \in Chans |-> <<>>]
    /\ pending = {}
    /\ uses = [c \in Chans |-> <<>>]
    /\ delivered = {}
    /\ spaces = {}
    /\ ctx = [p \in Parts |-> "default"]
    /\ lex = [p \in Parts |-> "default"]
    /\ aborted = FALSE
    /\ steps = 0

Waiting(p) == \E c \in Chans : <<p, c>> \in pending

Children(p) == {Append(p, 1), Append(p, 2)}

(* F26: a split replaces p by its two children, which inherit its context. *)
Split(p) ==
    /\ p \in parts /\ ~Waiting(p) /\ Len(p) < MaxDepth
    /\ parts' = (parts \ {p}) \cup Children(p)
    /\ ctx' = [ctx EXCEPT ![Append(p, 1)] = ctx[p], ![Append(p, 2)] = ctx[p]]
    /\ lex' = [lex EXCEPT ![Append(p, 1)] = lex[p], ![Append(p, 2)] = lex[p]]
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, aborted>>

(* use S { P } | Q: the body P runs in the first child with the context S,  *)
(* and Q in the second child with the context of p (wrap_body, eval_use).   *)
Use(p) ==
    /\ p \in parts /\ ~Waiting(p) /\ Len(p) < MaxDepth
    /\ parts' = (parts \ {p}) \cup Children(p)
    /\ ctx' = [ctx EXCEPT ![Append(p, 1)] = "S",
                          ![Append(p, 2)] = IF UseRestores THEN ctx[p] ELSE "S"]
    /\ lex' = [lex EXCEPT ![Append(p, 1)] = "S", ![Append(p, 2)] = lex[p]]
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, aborted>>

Finish(p) ==
    /\ p \in parts /\ ~Waiting(p)
    /\ parts' = parts \ {p}
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted>>

Admissible(p, c) == c \notin claimed \/ IsPrefix(claim[c], p)

(* SQ-3: the check of a program job of p on c. *)
StartUse(p, c) ==
    /\ p \in parts /\ ~Waiting(p)
    /\ AtomicGuard => pending = {}
    /\ IF Admissible(p, c)
         THEN /\ pending' = pending \cup {<<p, c>>}
              /\ UNCHANGED aborted
         ELSE /\ aborted' = TRUE
              /\ UNCHANGED pending
    /\ UNCHANGED <<parts, claimed, claim, uses, delivered, spaces, ctx, lex>>

(* SQ-4: the job commits and stores its claim. *)
CommitUse(j) ==
    /\ j \in pending
    /\ pending' = pending \ {j}
    /\ claimed' = claimed \cup {j[2]}
    /\ claim' = [claim EXCEPT ![j[2]] = j[1]]
    /\ uses' = [uses EXCEPT ![j[2]] = Append(@, j[1])]
    /\ UNCHANGED <<parts, delivered, spaces, ctx, lex, aborted>>

(* SQ-4: the job fails after its check: the deploy aborts, and no claim     *)
(* changes.                                                                 *)
FailUse(j) ==
    /\ j \in pending
    /\ pending' = pending \ {j}
    /\ aborted' = TRUE
    /\ UNCHANGED <<parts, claimed, claim, uses, delivered, spaces, ctx, lex>>

(* SQ-2: a plain send of the message ns.                                    *)
SendPlain(ns) ==
    /\ IF ScanEnabled /\ ns \cap SeqNames # {}
         THEN /\ aborted' = TRUE
              /\ UNCHANGED delivered
         ELSE /\ delivered' = delivered \cup {[kind |-> "plain", arg |-> "none", names |-> ns]}
              /\ UNCHANGED aborted
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, spaces, ctx, lex>>

(* SQ-2: a handle produce with the channel argument a (a name of the own    *)
(* space, exempt) and the message ns.                                       *)
SendHandle(a, ns) ==
    /\ IF ScanEnabled /\ ns \cap SeqNames # {}
         THEN /\ aborted' = TRUE
              /\ UNCHANGED delivered
         ELSE /\ delivered' = delivered \cup {[kind |-> "handle", arg |-> a, names |-> ns]}
              /\ UNCHANGED aborted
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, spaces, ctx, lex>>

(* QT-4 and QT-5: gate 3 of a factory call.                                 *)
Gate3(o, q, m) ==
    /\ o = "hashset" => q = "seq"
    /\ NativeOnlySeq => (q = "seq" => m = "native")

Create(o, q, m) ==
    /\ IF Gate3(o, q, m)
         THEN spaces' = spaces \cup {[outer |-> o, q |-> q, model |-> m]}
         ELSE UNCHANGED spaces
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, delivered, ctx, lex, aborted>>

Live == ~aborted /\ steps < MaxSteps

Next ==
    /\ Live
    /\ steps' = steps + 1
    /\ \/ \E p \in parts : Split(p) \/ Use(p) \/ Finish(p)
       \/ \E p \in parts, c \in Chans : StartUse(p, c)
       \/ \E j \in pending : CommitUse(j) \/ FailUse(j)
       \/ \E ns \in PlainMessages : SendPlain(ns)
       \/ \E a \in HandleArguments, ns \in HandleMessages : SendHandle(a, ns)
       \/ \E o \in Outers, q \in Qualifiers, m \in Models : Create(o, q, m)

Spec == Init /\ [][Next]_vars

----------------------------------------------------------------------------
(* Invariants *)

SeqExclusive ==
    \A c \in Chans : \A i \in 1..(Len(uses[c]) - 1) : IsPrefix(uses[c][i], uses[c][i + 1])

ClaimTracksLastUse ==
    \A c \in Chans :
        /\ c \in claimed <=> Len(uses[c]) > 0
        /\ c \in claimed => claim[c] = uses[c][Len(uses[c])]

SeqNotSent == \A d \in delivered : d.names \cap SeqNames = {}

QualifierSupport ==
    \A s \in spaces : /\ s.outer = "hashset" => s.q = "seq"
                      /\ s.q = "seq" => s.model = "native"

UseBalance == \A p \in parts : ctx[p] = lex[p]
=============================================================================

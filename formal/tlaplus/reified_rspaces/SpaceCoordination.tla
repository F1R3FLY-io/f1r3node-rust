-------------------------- MODULE SpaceCoordination --------------------------
(***************************************************************************)
(* The coordination rules of P7 (design v3, W1 and W3): the claims of a    *)
(* seq space, the send rule of seq names, the qualifier gate, and the use  *)
(* context.                                                                *)
(*                                                                         *)
(* Changed by decision 91 (DR-132): evaluation races, as on dev, and the   *)
(* claims of a seq space use causal histories instead of the participant  *)
(* paths of the removed scheduler. The model of decision 73 is in git     *)
(* history (git show 6bf10a721:formal/tlaplus/reified_rspaces/             *)
(* SpaceCoordination.tla).                                                 *)
(*                                                                         *)
(* Participants form a tree. A split replaces a participant by two         *)
(* children (F26). A participant that waits for a job does not split. Each *)
(* participant carries a history: the uses of seq channels that happened   *)
(* before its next step. A split gives both children the history of the    *)
(* parent. A COMM gives the receiver the history of the sender, so the     *)
(* body of a fire knows every use before each participant of the COMM.     *)
(* A seq space has the channels x and y. A program job of participant p    *)
(* uses one channel c: it checks the claim of c, and later it commits or   *)
(* fails. The check admits the use when c has no claim or the claim (the   *)
(* last use of c) is in the history of p (QF-15, decision 91). A refused   *)
(* check aborts the deploy (SeqConcurrent). A commit makes the use the     *)
(* claim of c, and the reply of the job gives p the use. A failed job      *)
(* aborts the deploy and changes no claim. The stripes of a channel        *)
(* serialize its uses between the check and the commit, because one space  *)
(* transaction holds them for both (the atomic guard). An internal job (a  *)
(* re-issue or a settle job) uses no channel, so the model has no step for *)
(* it (A3).                                                                *)
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
(* Changed by decision 90 (C6, step D): the code refuses every factory     *)
(* call outside native execution before every gate (ensure_available).     *)
(* The model keeps the execution model, so it admits every creation of the *)
(* code and more, and its invariants hold for the code.                    *)
(*                                                                         *)
(* The book of the evaluation (decision 83): the creation job of a seq     *)
(* space records the space before the factory replies, and a seq name      *)
(* exists only after the reply. So a send that holds a seq name reads a    *)
(* book with the space, and the scan sees it.                              *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   SeqCausalChain     for each channel, each admitted use happened       *)
(*                      before the next one                                *)
(*   HistIsCausalPast   the history of each participant is the set of the  *)
(*                      uses in its causal past, computed independently    *)
(*                      from the event log (program order and COMMs)       *)
(*   NoFalseRefusal     the history of a refused use misses the last use   *)
(*                      of its channel. With HistIsCausalPast, the         *)
(*                      refused use is then concurrent with that use.      *)
(*                      A wrong history breaks only HistIsCausalPast.      *)
(*   ClaimTracksLastUse the claim of a channel is its last admitted use    *)
(*   SeqNotSent         no delivered message holds a seq name              *)
(*   QualifierSupport   a HashSet space is a seq space, and a seq space    *)
(*                      exists only in native execution                    *)
(*   UseBalance         the context of each participant is its lexical     *)
(*                      context                                            *)
(* Reachability witness: NoTwoUseChain (a chain of two uses of one channel *)
(* is admitted, so SeqCausalChain does not hold vacuously).                *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   ClaimsNeedContext = TRUE ClaimsNeedContextUnsafe: the code before     *)
(*                         decision 91, where a program use needs the      *)
(*                         operation order that racing evaluation never    *)
(*                         sets -> NoFalseRefusal                          *)
(*   MergeOnComm = FALSE   NoMergeUnsafe: a COMM does not give the         *)
(*                         receiver the sender's history                   *)
(*                         -> HistIsCausalPast                             *)
(*   InheritOnSplit = FALSE NoInheritUnsafe: the children of a split start *)
(*                         with no history -> HistIsCausalPast             *)
(*   AtomicGuard = FALSE   SeqGuardToctouUnsafe: two uses of one channel   *)
(*                         check one claim before either commits           *)
(*                         -> SeqCausalChain                               *)
(*   ScanEnabled = FALSE   SeqScanSkippedUnsafe: route_send does not scan  *)
(*                         -> SeqNotSent                                   *)
(*   NativeOnlySeq = FALSE QualifierSupportUnsafe: a factory creates a seq *)
(*                         space outside native execution                  *)
(*                         -> QualifierSupport                             *)
(*   UseRestores = FALSE   UseBalanceUnsafe: the use context leaks into    *)
(*                         the rest of the process -> UseBalance           *)
(* Retired by decision 90 (DR-132): RecordInWindowUnsafe and               *)
(* ScanViewPerWindow, because racing evaluation has no scheduling windows. *)
(*                                                                         *)
(* Code (paths under rholang/src/rust/interpreter):                        *)
(*   claims        spaces/ephemeral.rs (SeqClaims::open, claim, store)     *)
(*   histories     reified/seq.rs (the causal histories of decision 91),   *)
(*                 reduce.rs (spawn_in_dispatch_scope keeps the history of *)
(*                 the parent task), dispatch.rs (the fired body runs      *)
(*                 under the union of its participants' histories)         *)
(*   the uses      spaces/generic.rs (Cx::claim and the staging            *)
(*                 primitives), spaces/join.rs (own_claims)                *)
(*   check, commit spaces/generic.rs (run_plan, run_with): the claims open *)
(*                 with the plan and are stored after a successful commit  *)
(*   atomic guard  the stripes of a space transaction                      *)
(*                 (rspace++/src/rspace/space_store_handle.rs, begin)      *)
(*   internal jobs reified/reducer_hooks.rs (the re-issue runs with        *)
(*                 OpOrigin::Internal); SpaceOp::Settle is internal by its *)
(*                 variant                                                 *)
(*   the scan      reified/seq.rs (check_send, scan),                      *)
(*                 reified/reducer_hooks.rs (route_send)                   *)
(*   gate 3        spaces/factory/constructor.rs (validate)                *)
(*   availability  reified/runtime.rs (ensure_available), called by        *)
(*                 reified/factory.rs (FactoryProcess::call) before gate 1 *)
(*   use context   reified/scope.rs (wrap_body),                           *)
(*                 reified/reducer_hooks.rs (eval_use)                     *)
(*   the book      spaces/generic.rs (the creation job records a seq space *)
(*                 after its commit), spaces/ephemeral.rs (record_created) *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - A participant path is a sequence over {1, 2}: the index of each     *)
(*     split child. It names a participant and its lineage. The claims do  *)
(*     not read it.                                                        *)
(*   - A COMM step lets participant p receive a message that participant q *)
(*     sends in the same step.                                             *)
(*   - A message is the set of its names: the seq names nx and ny of the   *)
(*     space, and the ordinary name o.                                     *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* TRUE: the stripes of a channel serialize its uses between their check
    \* and their commit.
    AtomicGuard,
    \* TRUE: route_send scans the message of every send.
    ScanEnabled,
    \* TRUE: no factory call creates a seq space outside native execution.
    \* Changed by decision 90 (C6, step D): the code refuses every factory call
    \* outside native execution before every gate (ensure_available). Before, TRUE
    \* meant that gate 3 refuses seq in legacy execution.
    NativeOnlySeq,
    \* TRUE: the use block keeps the context of the rest of the process.
    UseRestores,
    \* Changed by decision 90 (DR-132): RecordInJob is retired with the
    \* scheduling windows.
    \* RecordInJob,
    \* Added by decision 91. TRUE: the code before decision 91, where a
    \* program use needs an operation order.
    ClaimsNeedContext,
    \* Added by decision 91. TRUE: a COMM gives the receiver the history of
    \* the sender.
    MergeOnComm,
    \* Added by decision 91. TRUE: both children of a split get the history
    \* of the parent.
    InheritOnSplit,
    \* The depth bound of the participant tree.
    MaxDepth,
    \* The number of steps of one run.
    MaxSteps,
    \* Added by decision 91: the number of uses of one run.
    MaxUses

ASSUME AtomicGuard \in BOOLEAN /\ ScanEnabled \in BOOLEAN
ASSUME NativeOnlySeq \in BOOLEAN /\ UseRestores \in BOOLEAN
ASSUME ClaimsNeedContext \in BOOLEAN /\ MergeOnComm \in BOOLEAN /\ InheritOnSplit \in BOOLEAN
ASSUME MaxDepth \in 1..3 /\ MaxSteps \in 1..8 /\ MaxUses \in 1..4

Chans == {"x", "y"}
SeqNames == {"nx", "ny"}
Names == SeqNames \cup {"o"}
Parts == UNION {[1..n -> {1, 2}] : n \in 0..MaxDepth}
Contexts == {"default", "S"}
UseIds == 1..MaxUses
\* No use (the claim of a channel without one, or a refusal without one).
None == 0

PlainMessages == {{"o"}, {"nx"}, {"o", "ny"}}
HandleArguments == {"nx"}
HandleMessages == {{}, {"o"}, {"ny"}}
Deliveries == [kind : {"plain", "handle"}, arg : Names \cup {"none"}, names : SUBSET Names]

Outers == {"hashmap", "hashset"}
Qualifiers == {"default", "seq"}
Models == {"legacy", "native"}
Spaces == [outer : Outers, q : Qualifiers, model : Models]

\* An event of the log: a committed use at its participant, or a COMM whose
\* receiver is `at` and whose sender is `from`.
Events == [kind : {"use", "comm"}, at : Parts, from : Parts, id : UseIds \cup {None}]
\* A refusal: the participant, the channel, the last use of the channel at
\* the check, and the history of the participant at the check.
NoRefusal == [p |-> <<>>, c |-> "x", last |-> None, known |-> {}]

IsPrefix(a, b) == Len(a) <= Len(b) /\ SubSeq(b, 1, Len(a)) = a

VARIABLES
    parts,      \* the live participants
    claimed,    \* the channels with a claim
    claim,      \* the claim of each claimed channel: its last admitted use
    pending,    \* the jobs between their check and their commit: <<p, c, id>>
    uses,       \* the admitted uses of each channel, in commit order
    delivered,  \* the delivered messages
    spaces,     \* the created spaces
    ctx,        \* the context of each participant
    lex,        \* the lexical context of each participant
    aborted,    \* TRUE when the deploy aborted
    steps,      \* the number of steps so far
    book,       \* TRUE when the book of the evaluation holds the seq space
    created,    \* TRUE when a job created the seq space
    hist,       \* Added by decision 91: the history of each participant
    useHist,    \* Added by decision 91: the history of the participant of each use
    nextUse,    \* Added by decision 91: the id of the next use
    log,        \* Added by decision 91: the events, in step order
    refusal     \* Added by decision 91: the refusal that aborted the deploy

vars == <<parts, claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted, steps,
          book, created, hist, useHist, nextUse, log, refusal>>

TypeOK ==
    /\ parts \subseteq Parts
    /\ claimed \subseteq Chans
    /\ claim \in [Chans -> UseIds \cup {None}]
    /\ pending \subseteq (Parts \X Chans \X UseIds)
    /\ uses \in [Chans -> Seq(UseIds)]
    /\ delivered \subseteq Deliveries
    /\ spaces \subseteq Spaces
    /\ ctx \in [Parts -> Contexts]
    /\ lex \in [Parts -> Contexts]
    /\ aborted \in BOOLEAN
    /\ steps \in 0..MaxSteps
    /\ book \in BOOLEAN
    /\ created \in BOOLEAN
    /\ hist \in [Parts -> SUBSET UseIds]
    /\ useHist \in [UseIds -> SUBSET UseIds]
    /\ nextUse \in 1..(MaxUses + 1)
    /\ log \in Seq(Events)
    /\ refusal \in [p : Parts, c : Chans, last : UseIds \cup {None}, known : SUBSET UseIds]

Init ==
    /\ parts = {<<>>}
    /\ claimed = {}
    /\ claim = [c \in Chans |-> None]
    /\ pending = {}
    /\ uses = [c \in Chans |-> <<>>]
    /\ delivered = {}
    /\ spaces = {}
    /\ ctx = [p \in Parts |-> "default"]
    /\ lex = [p \in Parts |-> "default"]
    /\ aborted = FALSE
    /\ steps = 0
    /\ book = FALSE
    /\ created = FALSE
    /\ hist = [p \in Parts |-> {}]
    /\ useHist = [u \in UseIds |-> {}]
    /\ nextUse = 1
    /\ log = <<>>
    /\ refusal = NoRefusal

----------------------------------------------------------------------------
(* The causal past, computed from the event log alone (decision 91).       *)

\* Event i happened directly before event j: program order along the
\* lineage (the participant of i is j's or an ancestor of it), or the
\* sender's past of a COMM j.
DirectlyBefore(i, j) ==
    /\ i < j
    /\ \/ IsPrefix(log[i].at, log[j].at)
       \/ log[j].kind = "comm" /\ IsPrefix(log[i].at, log[j].from)

RECURSIVE Before(_)
\* The events that happened before event j.
Before(j) ==
    LET direct == {i \in 1..(j - 1) : DirectlyBefore(i, j)}
    IN direct \cup UNION {Before(i) : i \in direct}

\* The events that happened before the next step of participant p.
BeforeNow(p) ==
    LET direct == {i \in 1..Len(log) : IsPrefix(log[i].at, p)}
    IN direct \cup UNION {Before(i) : i \in direct}

\* The uses in the causal past of the next step of participant p.
TruePast(p) == {log[i].id : i \in {k \in BeforeNow(p) : log[k].kind = "use"}}

----------------------------------------------------------------------------

Waiting(p) == \E c \in Chans, u \in UseIds : <<p, c, u>> \in pending

Children(p) == {Append(p, 1), Append(p, 2)}

\* The history of a child of p after a split ("NoInheritUnsafe": none).
ChildHistory(p) == IF InheritOnSplit THEN hist[p] ELSE {}

(* F26: a split replaces p by its two children, which inherit its context  *)
(* and its history.                                                         *)
Split(p) ==
    /\ p \in parts /\ ~Waiting(p) /\ Len(p) < MaxDepth
    /\ parts' = (parts \ {p}) \cup Children(p)
    /\ ctx' = [ctx EXCEPT ![Append(p, 1)] = ctx[p], ![Append(p, 2)] = ctx[p]]
    /\ lex' = [lex EXCEPT ![Append(p, 1)] = lex[p], ![Append(p, 2)] = lex[p]]
    /\ hist' = [hist EXCEPT ![Append(p, 1)] = ChildHistory(p), ![Append(p, 2)] = ChildHistory(p)]
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, aborted, book, created,
                   useHist, nextUse, log, refusal>>

(* use S { P } | Q: the body P runs in the first child with the context S,  *)
(* and Q in the second child with the context of p (wrap_body, eval_use).   *)
Use(p) ==
    /\ p \in parts /\ ~Waiting(p) /\ Len(p) < MaxDepth
    /\ parts' = (parts \ {p}) \cup Children(p)
    /\ ctx' = [ctx EXCEPT ![Append(p, 1)] = "S",
                          ![Append(p, 2)] = IF UseRestores THEN ctx[p] ELSE "S"]
    /\ lex' = [lex EXCEPT ![Append(p, 1)] = "S", ![Append(p, 2)] = lex[p]]
    /\ hist' = [hist EXCEPT ![Append(p, 1)] = ChildHistory(p), ![Append(p, 2)] = ChildHistory(p)]
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, aborted, book, created,
                   useHist, nextUse, log, refusal>>

Finish(p) ==
    /\ p \in parts /\ ~Waiting(p)
    /\ parts' = parts \ {p}
    /\ UNCHANGED <<claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted, book,
                   created, hist, useHist, nextUse, log, refusal>>

(* Added by decision 91: a COMM. Participant p receives a message that      *)
(* participant q sends in this step, so p gets the history of q.            *)
Comm(p, q) ==
    /\ p \in parts /\ q \in parts /\ p /= q /\ ~Waiting(p) /\ ~Waiting(q)
    /\ Len(log) < MaxSteps
    /\ hist' = [hist EXCEPT ![p] = IF MergeOnComm THEN hist[p] \cup hist[q] ELSE hist[p]]
    /\ log' = Append(log, [kind |-> "comm", at |-> p, from |-> q, id |-> None])
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted,
                   book, created, useHist, nextUse, refusal>>

\* Changed by decision 91: the last use of c is in the history of p.
\* Admissible(p, c) == c \notin claimed \/ IsPrefix(claim[c], p)
Admissible(p, c) == c \notin claimed \/ claim[c] \in hist[p]

\* The atomic guard: no other use of c is between its check and its commit.
ChannelFree(c) == \A j \in pending : j[2] /= c

(* QF-15: the check of a program job of p on c ("ClaimsNeedContextUnsafe": *)
(* the code before decision 91 refuses every program use).                  *)
StartUse(p, c) ==
    /\ p \in parts /\ ~Waiting(p)
    /\ nextUse <= MaxUses
    /\ AtomicGuard => ChannelFree(c)
    /\ IF ~ClaimsNeedContext /\ Admissible(p, c)
         THEN /\ pending' = pending \cup {<<p, c, nextUse>>}
              /\ nextUse' = nextUse + 1
              /\ UNCHANGED <<aborted, refusal>>
         ELSE /\ aborted' = TRUE
              /\ refusal' = [p |-> p, c |-> c,
                             last |-> IF c \in claimed THEN claim[c] ELSE None,
                             known |-> hist[p]]
              /\ UNCHANGED <<pending, nextUse>>
    /\ UNCHANGED <<parts, claimed, claim, uses, delivered, spaces, ctx, lex, book, created, hist,
                   useHist, log>>

(* SQ-4: the job commits and stores its claim, and its reply gives p the    *)
(* use.                                                                     *)
CommitUse(j) ==
    /\ j \in pending
    /\ Len(log) < MaxSteps
    /\ pending' = pending \ {j}
    /\ claimed' = claimed \cup {j[2]}
    /\ claim' = [claim EXCEPT ![j[2]] = j[3]]
    /\ uses' = [uses EXCEPT ![j[2]] = Append(@, j[3])]
    /\ useHist' = [useHist EXCEPT ![j[3]] = hist[j[1]]]
    /\ hist' = [hist EXCEPT ![j[1]] = @ \cup {j[3]}]
    /\ log' = Append(log, [kind |-> "use", at |-> j[1], from |-> j[1], id |-> j[3]])
    /\ UNCHANGED <<parts, delivered, spaces, ctx, lex, aborted, book, created, nextUse, refusal>>

(* SQ-4: the job fails after its check: the deploy aborts, and no claim     *)
(* changes.                                                                 *)
FailUse(j) ==
    /\ j \in pending
    /\ pending' = pending \ {j}
    /\ aborted' = TRUE
    /\ UNCHANGED <<parts, claimed, claim, uses, delivered, spaces, ctx, lex, book, created, hist,
                   useHist, nextUse, log, refusal>>

(* SQ-2: a plain send of the message ns. A seq name exists only after the  *)
(* record, so a send that holds one reads a book with the space.            *)
SendPlain(ns) ==
    /\ ns \cap SeqNames # {} => book
    /\ IF ScanEnabled /\ book /\ ns \cap SeqNames # {}
         THEN /\ aborted' = TRUE
              /\ UNCHANGED delivered
         ELSE /\ delivered' = delivered \cup {[kind |-> "plain", arg |-> "none", names |-> ns]}
              /\ UNCHANGED aborted
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, spaces, ctx, lex, book, created, hist,
                   useHist, nextUse, log, refusal>>

(* SQ-2: a handle produce with the channel argument a (a name of the own    *)
(* space, exempt) and the message ns. The channel argument is a seq name,   *)
(* so the send follows the record.                                          *)
SendHandle(a, ns) ==
    /\ book
    /\ IF ScanEnabled /\ book /\ ns \cap SeqNames # {}
         THEN /\ aborted' = TRUE
              /\ UNCHANGED delivered
         ELSE /\ delivered' = delivered \cup {[kind |-> "handle", arg |-> a, names |-> ns]}
              /\ UNCHANGED aborted
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, spaces, ctx, lex, book, created, hist,
                   useHist, nextUse, log, refusal>>

(* QT-4 and QT-5: gate 3 of a factory call. Changed by decision 90 (C6,    *)
(* step D): the code also refuses every factory call outside native        *)
(* execution before every gate, so this gate over-approximates the code.   *)
Gate3(o, q, m) ==
    /\ o = "hashset" => q = "seq"
    /\ NativeOnlySeq => (q = "seq" => m = "native")

(* The creation job of a factory call. The job records a seq space before   *)
(* the factory replies (decision 83).                                       *)
Create(o, q, m) ==
    /\ IF Gate3(o, q, m)
         THEN spaces' = spaces \cup {[outer |-> o, q |-> q, model |-> m]}
         ELSE UNCHANGED spaces
    /\ created' = (created \/ (Gate3(o, q, m) /\ q = "seq"))
    /\ book' = (book \/ (Gate3(o, q, m) /\ q = "seq"))
    /\ UNCHANGED <<parts, claimed, claim, pending, uses, delivered, ctx, lex, aborted, hist,
                   useHist, nextUse, log, refusal>>

\* Changed by decision 90 (DR-132): the record in a window after the job
\* (RecordInJob = FALSE, before decision 83) is retired with the windows.
\* Record ==
\*     /\ ~RecordInJob /\ created /\ ~book
\*     /\ book' = TRUE
\*     /\ UNCHANGED <<parts, claimed, claim, pending, uses, delivered, spaces, ctx, lex, aborted,
\*                    created, seen>>

Live == ~aborted /\ steps < MaxSteps

Next ==
    /\ Live
    /\ steps' = steps + 1
    /\ \/ \E p \in parts : Split(p) \/ Use(p) \/ Finish(p)
       \/ \E p, q \in parts : Comm(p, q)
       \/ \E p \in parts, c \in Chans : StartUse(p, c)
       \/ \E j \in pending : CommitUse(j) \/ FailUse(j)
       \/ \E ns \in PlainMessages : SendPlain(ns)
       \/ \E a \in HandleArguments, ns \in HandleMessages : SendHandle(a, ns)
       \/ \E o \in Outers, q \in Qualifiers, m \in Models : Create(o, q, m)

Spec == Init /\ [][Next]_vars

----------------------------------------------------------------------------
(* Invariants *)

\* Changed by decision 91: the admitted uses of a channel form a causal
\* chain, not a prefix chain of participant paths.
\* SeqExclusive ==
\*     \A c \in Chans : \A i \in 1..(Len(uses[c]) - 1) : IsPrefix(uses[c][i], uses[c][i + 1])
SeqCausalChain ==
    \A c \in Chans : \A i \in 1..(Len(uses[c]) - 1) : uses[c][i] \in useHist[uses[c][i + 1]]

\* Added by decision 91: the history that the rules keep is the causal past
\* of the event log.
HistIsCausalPast == \A p \in parts : hist[p] = TruePast(p)

\* Added by decision 91: the history of a refused use misses the last use of
\* its channel. The invariant reads the history that the guard reads. With
\* HistIsCausalPast, that history is the causal past of the use, so the
\* refused use is concurrent with the last use, and the refusal is not
\* false. A wrong history breaks only HistIsCausalPast (NoMergeUnsafe,
\* NoInheritUnsafe).
NoFalseRefusal ==
    refusal /= NoRefusal => (refusal.last /= None /\ refusal.last \notin refusal.known)

ClaimTracksLastUse ==
    \A c \in Chans :
        /\ c \in claimed <=> Len(uses[c]) > 0
        /\ c \in claimed => claim[c] = uses[c][Len(uses[c])]

SeqNotSent == \A d \in delivered : d.names \cap SeqNames = {}

QualifierSupport ==
    \A s \in spaces : /\ s.outer = "hashset" => s.q = "seq"
                      /\ s.q = "seq" => s.model = "native"

UseBalance == \A p \in parts : ctx[p] = lex[p]

\* Changed by decision 90 (DR-132): retired with the scheduling windows.
\* ScanViewPerWindow == Cardinality(seen) <= 1

\* Added by decision 91: the reachability witness of a chain of two uses.
NoTwoUseChain == \A c \in Chans : Len(uses[c]) < 2
=============================================================================

---------------------- MODULE OfferedCandidateQuarantine ----------------------
(***************************************************************************)
(* DR-72: proposer liveness for offered-funded candidates.                 *)
(*                                                                         *)
(* A proposal selects a window of at most `cap` pending candidates in      *)
(* canonical order and checkpoints its head. A candidate-attributable      *)
(* failure quarantines that candidate: it leaves pending storage and       *)
(* enters a bounded FIFO status log. The proposal then tries the next      *)
(* alternate. With no alternate left, it builds an empty block when other  *)
(* work exists, and otherwise ends with NoNewDeploys. An infrastructure    *)
(* error ends the proposal without a quarantine. Between proposals a       *)
(* client may admit a candidate again; admission clears its status entry.  *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   window, batch                block_creator.rs selection loop          *)
(*                                (ordinary_cap; DR-134: no alternates)    *)
(*   Checkpoint                   create_inner checkpoint loop             *)
(*   quarantine, log              KeyValueDeployStorage::quarantine_envelope*)
(*                                and EnvelopeRejectionLog                 *)
(*   Admit                        add_envelope_if_absent (clears entry)    *)
(*   InfraError                   runtime.rs offered_candidate_rejection   *)
(*                                (infrastructure classes pass through)    *)
(*                                                                         *)
(* DR-134 (DR-65, O-6): a proposal attempts its whole window as one batch.  *)
(* A failing member is quarantined and the rest of the batch runs again    *)
(* (DR-63); with no member left the proposal builds an empty block when    *)
(* other work exists, and otherwise ends with NoNewDeploys. There are no    *)
(* alternates. A finished proposal includes exactly the fundable members of *)
(* its window (FundableWindowIncluded, which replaces FirstFundableIncluded).*)
(*                                                                         *)
(* Mutation selects one negative control. Each control must violate the    *)
(* named invariant in its *Unsafe.cfg:                                     *)
(*   "escape"      the proposer returns the candidate error                *)
(*                 -> NoCandidateErrorEscapes                              *)
(*   "overremove"  quarantine drops every pending candidate                *)
(*                 -> RemovedOnlyByInclusionOrQuarantine                   *)
(*   "unbounded"   the attempt sequence ignores ordinary_cap               *)
(*                 -> AttemptsBounded                                      *)
(*   "noevict"     the status log never evicts -> LogBounded               *)
(*   "skiphead"    selection skips the canonical head                      *)
(*                 -> FundableWindowIncluded                               *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS NumCandidates, MaxCap, MaxLogCapacity, MaxProposals, Mutation

Order == [i \in 1..NumCandidates |-> "c" \o ToString(i)]
Candidates == {Order[i] : i \in 1..NumCandidates}
Empty == "empty"
None == "none"
Outcomes == {"idle", "running", "created", "nonew", "infra", "candidate_error"}

ASSUME /\ NumCandidates \in Nat \ {0}
       /\ Empty \notin Candidates
       /\ None \notin Candidates
       /\ MaxCap \in Nat \ {0}
       /\ MaxLogCapacity \in Nat \ {0}
       /\ MaxProposals \in Nat \ {0}
       /\ Mutation \in {"none", "escape", "overremove", "unbounded",
                        "noevict", "skiphead"}

VARIABLES
  fundable,     \* candidates whose checkpoint succeeds; fixed per behavior
  cap,          \* ordinary_cap, the per-proposal window bound; fixed
  logCapacity,  \* status log capacity; fixed per behavior
  otherWork,    \* empty-block, slashing, or recovered-slash work exists
  pending,      \* candidates in pending storage
  log,          \* status log: quarantined identities in FIFO order
  history,      \* every quarantine decision in order (auxiliary)
  included,     \* candidates included in some created block (auxiliary)
  window,       \* canonical window of the current proposal (auxiliary)
  \* DR-134 (DR-65): the proposal includes a batch, and a batch has no
  \* alternates.
  \* chosen,       \* candidate included by the current proposal, or None
  \* attempt,      \* current attempt: a candidate, Empty, or None
  \* alternates,   \* remaining alternates of the current proposal
  chosen,       \* members included by the current proposal (a set)
  attempt,      \* current attempt: "batch", "empty", or "none"
  batch,        \* members of the current batch attempt, in canonical order
  attempts,     \* checkpoint executions in the current proposal
  outcome,      \* result of the current or last proposal
  proposals     \* proposals started so far

\* DR-134 (DR-65): `batch` replaces `alternates`.
vars == <<fundable, cap, logCapacity, otherWork, pending, log, history,
          included, window, chosen, attempt, batch, attempts, outcome,
          proposals>>

Min(a, b) == IF a <= b THEN a ELSE b

SeqRange(s) == {s[i] : i \in 1..Len(s)}

Last(s) == s[Len(s)]

PendingOrder == SelectSeq(Order, LAMBDA c : c \in pending)

FirstFundable(s) ==
  s[CHOOSE i \in 1..Len(s) :
      /\ s[i] \in fundable
      /\ \A j \in 1..(i - 1) : s[j] \notin fundable]

\* DR-134 (DR-65): the first member of a batch that fails alone; the
\* decision pass stops there.
FirstUnfundable(s) ==
  s[CHOOSE i \in 1..Len(s) :
      /\ s[i] \notin fundable
      /\ \A j \in 1..(i - 1) : s[j] \in fundable]

LogAppend(entry) ==
  LET appended == Append(log, entry)
  IN IF Mutation # "noevict" /\ Len(appended) > logCapacity
       THEN Tail(appended)
       ELSE appended

LogWithout(c) == SelectSeq(log, LAMBDA x : x # c)

Init ==
  /\ fundable \in SUBSET Candidates
  /\ cap \in 1..MaxCap
  /\ logCapacity \in 1..MaxLogCapacity
  /\ otherWork = FALSE
  /\ pending = Candidates
  /\ log = <<>>
  /\ history = <<>>
  /\ included = {}
  /\ window = <<>>
  /\ chosen = {}
  /\ attempt = "none"
  /\ batch = <<>>
  /\ attempts = 0
  /\ outcome = "idle"
  /\ proposals = 0

Admit(c) ==
  /\ outcome # "running"
  /\ c \notin pending
  /\ c \notin included
  /\ pending' = pending \cup {c}
  /\ log' = LogWithout(c)
  /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, history, included,
                 window, chosen, attempt, batch, attempts, outcome,
                 proposals>>

\* DR-134 (DR-65, O-6): the former single-candidate actions, kept for
\* reference.
\* StartProposal ==
\*   /\ outcome # "running"
\*   /\ outcome # "candidate_error"
\*   /\ proposals < MaxProposals
\*   /\ \E work \in BOOLEAN :
\*        LET ordered == PendingOrder
\*            w == SubSeq(ordered, 1, Min(cap, Len(ordered)))
\*            sequence == CASE Mutation = "unbounded" -> ordered
\*                          [] Mutation = "skiphead" /\ Len(w) >= 2 -> Tail(w)
\*                          [] OTHER -> w
\*        IN /\ otherWork' = work
\*           /\ window' = w
\*           /\ chosen' = None
\*           /\ attempts' = 0
\*           /\ proposals' = proposals + 1
\*           /\ IF sequence # <<>>
\*                THEN /\ attempt' = Head(sequence)
\*                     /\ alternates' = Tail(sequence)
\*                     /\ outcome' = "running"
\*                ELSE IF work
\*                       THEN /\ attempt' = Empty
\*                            /\ alternates' = <<>>
\*                            /\ outcome' = "running"
\*                       ELSE /\ attempt' = None
\*                            /\ alternates' = <<>>
\*                            /\ outcome' = "nonew"
\*   /\ UNCHANGED <<fundable, cap, logCapacity, pending, log, history, included>>
\*
\* Quarantine ==
\*   /\ pending' = IF Mutation = "overremove" THEN {} ELSE pending \ {attempt}
\*   /\ log' = LogAppend(attempt)
\*   /\ history' = Append(history, attempt)
\*   /\ UNCHANGED <<included, chosen>>
\*   /\ IF alternates # <<>>
\*        THEN /\ attempt' = Head(alternates)
\*             /\ alternates' = Tail(alternates)
\*             /\ outcome' = "running"
\*        ELSE IF otherWork
\*               THEN /\ attempt' = Empty
\*                    /\ alternates' = <<>>
\*                    /\ outcome' = "running"
\*               ELSE /\ attempt' = None
\*                    /\ alternates' = <<>>
\*                    /\ outcome' = "nonew"
\*
\* Checkpoint ==
\*   /\ outcome = "running"
\*   /\ attempts' = attempts + 1
\*   /\ CASE attempt = Empty ->
\*             /\ outcome' = "created"
\*             /\ attempt' = None
\*             /\ UNCHANGED <<pending, log, history, included, chosen, alternates>>
\*        [] attempt \in fundable ->
\*             /\ outcome' = "created"
\*             /\ included' = included \cup {attempt}
\*             /\ chosen' = attempt
\*             /\ pending' = pending \ {attempt}
\*             /\ attempt' = None
\*             /\ alternates' = <<>>
\*             /\ UNCHANGED <<log, history>>
\*        [] OTHER ->
\*             IF Mutation = "escape"
\*               THEN /\ outcome' = "candidate_error"
\*                    /\ attempt' = None
\*                    /\ alternates' = <<>>
\*                    /\ UNCHANGED <<pending, log, history, included, chosen>>
\*               ELSE Quarantine
\*   /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, window, proposals>>
\*
\* InfraError ==
\*   /\ outcome = "running"
\*   /\ outcome' = "infra"
\*   /\ attempt' = None
\*   /\ alternates' = <<>>
\*   /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, pending, log, history,
\*                  included, window, chosen, attempts, proposals>>
\*

StartProposal ==
  /\ outcome # "running"
  /\ outcome # "candidate_error"
  /\ proposals < MaxProposals
  /\ \E work \in BOOLEAN :
       LET ordered == PendingOrder
           w == SubSeq(ordered, 1, Min(cap, Len(ordered)))
           sequence == CASE Mutation = "unbounded" -> ordered
                         [] Mutation = "skiphead" /\ Len(w) >= 2 -> Tail(w)
                         [] OTHER -> w
       IN /\ otherWork' = work
          /\ window' = w
          /\ chosen' = {}
          /\ attempts' = 0
          /\ proposals' = proposals + 1
          /\ IF sequence # <<>>
               THEN /\ attempt' = "batch"
                    /\ batch' = sequence
                    /\ outcome' = "running"
               ELSE IF work
                      THEN /\ attempt' = "empty"
                           /\ batch' = <<>>
                           /\ outcome' = "running"
                      ELSE /\ attempt' = "none"
                           /\ batch' = <<>>
                           /\ outcome' = "nonew"
  /\ UNCHANGED <<fundable, cap, logCapacity, pending, log, history, included>>

\* A failing member is quarantined, and the rest of the batch runs again.
Quarantine ==
  LET failed == FirstUnfundable(batch)
      rest == SelectSeq(batch, LAMBDA x : x # failed)
  IN /\ pending' = IF Mutation = "overremove" THEN {} ELSE pending \ {failed}
     /\ log' = LogAppend(failed)
     /\ history' = Append(history, failed)
     /\ UNCHANGED <<included, chosen>>
     /\ IF rest # <<>>
          THEN /\ attempt' = "batch"
               /\ batch' = rest
               /\ outcome' = "running"
          ELSE IF otherWork
                 THEN /\ attempt' = "empty"
                      /\ batch' = <<>>
                      /\ outcome' = "running"
                 ELSE /\ attempt' = "none"
                      /\ batch' = <<>>
                      /\ outcome' = "nonew"

Checkpoint ==
  /\ outcome = "running"
  /\ attempts' = attempts + 1
  /\ CASE attempt = "empty" ->
            /\ outcome' = "created"
            /\ attempt' = "none"
            /\ UNCHANGED <<pending, log, history, included, chosen, batch>>
       [] attempt = "batch" /\ SeqRange(batch) \subseteq fundable ->
            /\ outcome' = "created"
            /\ included' = included \cup SeqRange(batch)
            /\ chosen' = SeqRange(batch)
            /\ pending' = pending \ SeqRange(batch)
            /\ attempt' = "none"
            /\ batch' = <<>>
            /\ UNCHANGED <<log, history>>
       [] OTHER ->
            IF Mutation = "escape"
              THEN /\ outcome' = "candidate_error"
                   /\ attempt' = "none"
                   /\ batch' = <<>>
                   /\ UNCHANGED <<pending, log, history, included, chosen>>
              ELSE Quarantine
  /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, window, proposals>>

InfraError ==
  /\ outcome = "running"
  /\ outcome' = "infra"
  /\ attempt' = "none"
  /\ batch' = <<>>
  /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, pending, log, history,
                 included, window, chosen, attempts, proposals>>

Next ==
  \/ StartProposal
  \/ Checkpoint
  \/ InfraError
  \/ \E c \in Candidates : Admit(c)

Spec == Init /\ [][Next]_vars /\ WF_vars(Checkpoint)

-----------------------------------------------------------------------------

TypeOK ==
  /\ fundable \subseteq Candidates
  /\ cap \in 1..MaxCap
  /\ logCapacity \in 1..MaxLogCapacity
  /\ otherWork \in BOOLEAN
  /\ pending \subseteq Candidates
  /\ log \in Seq(Candidates)
  /\ history \in Seq(Candidates)
  /\ included \subseteq Candidates
  /\ window \in Seq(Candidates)
  \* DR-134 (DR-65): a set of included members and a batch attempt.
  \* /\ chosen \in Candidates \cup {None}
  \* /\ attempt \in Candidates \cup {Empty, None}
  \* /\ alternates \in Seq(Candidates)
  /\ chosen \subseteq Candidates
  /\ attempt \in {"batch", "empty", "none"}
  /\ batch \in Seq(Candidates)
  /\ attempts \in Nat
  /\ outcome \in Outcomes
  /\ proposals \in 0..MaxProposals

\* A candidate failure never escapes create_inner as an error.
NoCandidateErrorEscapes == outcome # "candidate_error"

\* Pending storage loses a candidate only by inclusion or by quarantine.
RemovedOnlyByInclusionOrQuarantine ==
  \A c \in Candidates \ pending : c \in included \/ c \in SeqRange(history)

\* A quarantined candidate never enters a block, and only fundable
\* candidates do.
QuarantinedNeverIncluded ==
  /\ included \cap SeqRange(history) = {}
  /\ included \subseteq fundable

\* One proposal checkpoints at most the window plus one empty attempt.
AttemptsBounded == attempts <= cap + 1

\* The status log is bounded.
LogBounded == Len(log) <= logCapacity

\* The status log holds each identity at most once.
LogDistinct ==
  \A i, j \in 1..Len(log) : i # j => log[i] # log[j]

\* Admission clears the status entry of a pending candidate.
PendingNotLogged == \A c \in pending : c \notin SeqRange(log)

\* The status log is a subsequence of the quarantine history.
LogOrderFollowsHistory ==
  \E f \in [1..Len(log) -> 1..Len(history)] :
    /\ \A i \in 1..Len(log) : log[i] = history[f[i]]
    /\ \A i, j \in 1..Len(log) : i < j => f[i] < f[j]

\* A finished proposal includes the first fundable candidate of its window.
\* With no fundable candidate it includes nothing, and it builds an empty
\* block exactly when other work exists.
\* DR-134 (DR-65): replaced by FundableWindowIncluded, kept for reference.
\* FirstFundableIncluded ==
\*   outcome \in {"created", "nonew"} =>
\*     IF \E i \in 1..Len(window) : window[i] \in fundable
\*       THEN chosen = FirstFundable(window)
\*       ELSE /\ chosen = None
\*            /\ (outcome = "created") = otherWork

\* DR-134 (DR-65): a finished proposal includes exactly the fundable members
\* of its window. With no fundable member it includes nothing, and it builds
\* an empty block exactly when other work exists.
FundableWindowIncluded ==
  outcome \in {"created", "nonew"} =>
    IF \E i \in 1..Len(window) : window[i] \in fundable
      THEN chosen = SeqRange(window) \cap fundable
      ELSE /\ chosen = {}
           /\ (outcome = "created") = otherWork

\* A quarantine appends its identity and evicts only the oldest entry.
QuarantineAppendsAndEvictsOldest ==
  [][history' # history =>
       log' = IF Len(log) + 1 > logCapacity
                THEN Tail(Append(log, Last(history')))
                ELSE Append(log, Last(history'))]_vars

\* Every proposal terminates.
ProposalsTerminate == [](outcome = "running" => <>(outcome # "running"))

=============================================================================

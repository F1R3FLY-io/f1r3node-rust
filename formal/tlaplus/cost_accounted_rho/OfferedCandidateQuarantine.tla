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
(*   window, attempt, alternates  block_creator.rs selection loop          *)
(*                                (offered_alternates, ordinary_cap)       *)
(*   Checkpoint                   create_inner checkpoint loop             *)
(*   quarantine, log              KeyValueDeployStorage::quarantine_envelope*)
(*                                and EnvelopeRejectionLog                 *)
(*   Admit                        add_envelope_if_absent (clears entry)    *)
(*   InfraError                   runtime.rs offered_candidate_rejection   *)
(*                                (infrastructure classes pass through)    *)
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
(*                 -> FirstFundableIncluded                                *)
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
  chosen,       \* candidate included by the current proposal, or None
  attempt,      \* current attempt: a candidate, Empty, or None
  alternates,   \* remaining alternates of the current proposal
  attempts,     \* checkpoint executions in the current proposal
  outcome,      \* result of the current or last proposal
  proposals     \* proposals started so far

vars == <<fundable, cap, logCapacity, otherWork, pending, log, history,
          included, window, chosen, attempt, alternates, attempts, outcome,
          proposals>>

Min(a, b) == IF a <= b THEN a ELSE b

SeqRange(s) == {s[i] : i \in 1..Len(s)}

Last(s) == s[Len(s)]

PendingOrder == SelectSeq(Order, LAMBDA c : c \in pending)

FirstFundable(s) ==
  s[CHOOSE i \in 1..Len(s) :
      /\ s[i] \in fundable
      /\ \A j \in 1..(i - 1) : s[j] \notin fundable]

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
  /\ chosen = None
  /\ attempt = None
  /\ alternates = <<>>
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
                 window, chosen, attempt, alternates, attempts, outcome,
                 proposals>>

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
          /\ chosen' = None
          /\ attempts' = 0
          /\ proposals' = proposals + 1
          /\ IF sequence # <<>>
               THEN /\ attempt' = Head(sequence)
                    /\ alternates' = Tail(sequence)
                    /\ outcome' = "running"
               ELSE IF work
                      THEN /\ attempt' = Empty
                           /\ alternates' = <<>>
                           /\ outcome' = "running"
                      ELSE /\ attempt' = None
                           /\ alternates' = <<>>
                           /\ outcome' = "nonew"
  /\ UNCHANGED <<fundable, cap, logCapacity, pending, log, history, included>>

Quarantine ==
  /\ pending' = IF Mutation = "overremove" THEN {} ELSE pending \ {attempt}
  /\ log' = LogAppend(attempt)
  /\ history' = Append(history, attempt)
  /\ UNCHANGED <<included, chosen>>
  /\ IF alternates # <<>>
       THEN /\ attempt' = Head(alternates)
            /\ alternates' = Tail(alternates)
            /\ outcome' = "running"
       ELSE IF otherWork
              THEN /\ attempt' = Empty
                   /\ alternates' = <<>>
                   /\ outcome' = "running"
              ELSE /\ attempt' = None
                   /\ alternates' = <<>>
                   /\ outcome' = "nonew"

Checkpoint ==
  /\ outcome = "running"
  /\ attempts' = attempts + 1
  /\ CASE attempt = Empty ->
            /\ outcome' = "created"
            /\ attempt' = None
            /\ UNCHANGED <<pending, log, history, included, chosen, alternates>>
       [] attempt \in fundable ->
            /\ outcome' = "created"
            /\ included' = included \cup {attempt}
            /\ chosen' = attempt
            /\ pending' = pending \ {attempt}
            /\ attempt' = None
            /\ alternates' = <<>>
            /\ UNCHANGED <<log, history>>
       [] OTHER ->
            IF Mutation = "escape"
              THEN /\ outcome' = "candidate_error"
                   /\ attempt' = None
                   /\ alternates' = <<>>
                   /\ UNCHANGED <<pending, log, history, included, chosen>>
              ELSE Quarantine
  /\ UNCHANGED <<fundable, cap, logCapacity, otherWork, window, proposals>>

InfraError ==
  /\ outcome = "running"
  /\ outcome' = "infra"
  /\ attempt' = None
  /\ alternates' = <<>>
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
  /\ chosen \in Candidates \cup {None}
  /\ attempt \in Candidates \cup {Empty, None}
  /\ alternates \in Seq(Candidates)
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
FirstFundableIncluded ==
  outcome \in {"created", "nonew"} =>
    IF \E i \in 1..Len(window) : window[i] \in fundable
      THEN chosen = FirstFundable(window)
      ELSE /\ chosen = None
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

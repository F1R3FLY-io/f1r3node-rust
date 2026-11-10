-------------------------- MODULE ScopeTrappedGC --------------------------
(***************************************************************************)
(* Bounded safety model for the scope-trapped unforgeable garbage          *)
(* collector shipped in PR #984. Mechanizes the H1 invariant of            *)
(* `docs/rholang/garbage-collection.md` on finite, concrete Rholang-like   *)
(* programs: if the classifier `IsScopeTrapped(x, P)` accepts, then in     *)
(* every reachable state (process + rspace) the fresh unforgeable `N`      *)
(* that `new x in P` mints appears only in channel positions — never in   *)
(* a Send payload, a pattern, an expression, or a parked Receive body.    *)
(*                                                                         *)
(* Three defect classes have registered pre-fix negative controls:         *)
(*                                                                         *)
(*   1. Payload leak — the classifier weakened to accept `x` in            *)
(*      `Send.data`. Any trace that sends `x` on a non-trapped channel    *)
(*      produces a parked datum whose payload contains `N`; an external   *)
(*      receiver can bind it, violating Inv_N_NeverInPayload.             *)
(*                                                                         *)
(*   2. Receive-body mention, on a non-trapped channel — the classifier   *)
(*      weakened to accept `x` inside any Receive body. A receive parked  *)
(*      on a *forgeable* channel can be fired by an external deploy; its  *)
(*      body (which textually contains `N`) then emits a fresh send or    *)
(*      receive on `N` from outside the closure, violating                 *)
(*      Inv_N_NeverInReceiveBody.                                         *)
(*                                                                         *)
(*   3. Receive-body mention, on a receive whose source is `x` itself —   *)
(*      the subtler case that was the hole in the original prose proof.   *)
(*      A persistent receive on `x` embedded in its own body can emit     *)
(*      sends on `x` when fired from an inside-closure send, and the      *)
(*      body can contain a nested receive on a non-trapped channel        *)
(*      which carries `N` onward. The strict Rust H1 rejects this case;   *)
(*      the model's third knob exposes it as a regression.                *)
(*                                                                         *)
(* Code the model abstracts:                                               *)
(*                                                                         *)
(*   Model operator       Rust code                                        *)
(*   -----------------    ---------------------------------------------    *)
(*   IsScopeTrapped       rholang/src/rust/interpreter/garbage.rs::        *)
(*                          is_scope_trapped (the `check_par` walk)        *)
(*   AllocFresh (New)     rholang/src/rust/interpreter/reduce.rs::         *)
(*                          eval_new (rand.next() -> GPrivate id)          *)
(*   ParkSend / ParkRecv  rspace++/src/rspace/rspace/ops_consume.rs,       *)
(*                          ops_produce.rs (locked_consume/_produce)       *)
(*   Comm                 rspace++/src/rspace/rspace/ops_produce.rs::      *)
(*                          process_match_found                            *)
(*   FireParked           rspace++/src/rspace/rspace/ops_consume.rs:179    *)
(*                          (remove_matched_datum_and_join)                *)
(*   Sweep (soundness)    rspace++/src/rspace/rspace/ops_sweep.rs::        *)
(*                          locked_sweep                                   *)
(*                                                                         *)
(* The model is deliberately a Rholang-flavoured skeleton, not a full     *)
(* process-calculus mechanization: that scope goes to Rocq (Phase 2 of    *)
(* `docs/rholang/garbage-collection-formalization-plan.md`). What this    *)
(* module adds is a concrete counterexample oracle and three negative    *)
(* controls — enough to catch the "forgot a reduction case" bug class    *)
(* that holed the prose proof.                                           *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    N,                      \* model value: the fresh unforgeable allocated
                            \*   for the outer `new x in P`
    ForgeableChannels,      \* model values: channels accessible to any
                            \*   deploy (`@"public"`, `@1`, system URN-
                            \*   injected names). None of these is trapped.
    Payloads,               \* model values: opaque payload tokens that
                            \*   are not N (used in Send.data positions
                            \*   when x itself is not sent)
    MaxTermDepth,           \* bound on the AST depth of the generator;
                            \*   depth 2 is enough for the payload and
                            \*   body-mention regressions
    MaxReductionSteps,      \* bound on the reduction depth TLC explores
    H1_RejectsPayload,      \* TRUE in the post-fix: payload use of x
                            \*   defeats trappedness. FALSE in the
                            \*   payload_leak pre-fix control.
    H1_RejectsReceiveBody   \* TRUE in the post-fix: any occurrence of
                            \*   x inside a Receive.body defeats
                            \*   trappedness. FALSE in the body_mention
                            \*   pre-fix control. Covers the reduction
                            \*   rule the original prose proof forgot
                            \*   (firing a parked receive on a non-
                            \*   trapped channel re-emits its body into
                            \*   the live process).

ASSUME N \notin ForgeableChannels
ASSUME N \notin Payloads
ASSUME ForgeableChannels \intersect Payloads = {}
ASSUME MaxReductionSteps >= 1
ASSUME MaxTermDepth >= 1

----------------------------------------------------------------------------
(***************************************************************************)
(* Syntax                                                                  *)
(*                                                                         *)
(* A term is one of:                                                       *)
(*   Nil                              — empty process                      *)
(*   Send(chan, payload)              — send a payload on chan             *)
(*   Receive(chan, body)              — single-bind receive, body runs on  *)
(*                                       fire, is parked on chan otherwise *)
(*   Par(left, right)                 — parallel composition               *)
(*   New(body)                        — allocates a fresh unforgeable for  *)
(*                                       the single bound name X and      *)
(*                                       substitutes it in body. The      *)
(*                                       initial state has one outer      *)
(*                                       New wrapping the program under   *)
(*                                       test.                             *)
(*                                                                         *)
(* A chan or payload token is one of:                                      *)
(*   XName   — the bound name of the outer New (modelled as a literal;    *)
(*              AllocFresh substitutes it with N)                          *)
(*   NName   — the fresh unforgeable that AllocFresh minted               *)
(*   f       — a ForgeableChannel                                          *)
(*   p       — a Payload                                                   *)
(*                                                                         *)
(* No nested `new`. One binder is enough to exhibit all three defects      *)
(* without exploding the state space; the Rocq phase does the multi-      *)
(* binder generalization.                                                  *)
(***************************************************************************)

\* Full name universe — includes N, used by Subst's codomain and by the
\* `program` type after AllocFresh has fired.
Names == {"XName"} \cup {N} \cup ForgeableChannels \cup Payloads

\* Names a hand-written source program may textually reference. The
\* still-to-be-allocated fresh unforgeable `N` is EXCLUDED: nothing in
\* the source program can mention `N` before AllocFresh substitutes it
\* in — Rholang's semantics give the deploy no way to type a `GPrivate`
\* literal. Letting the generator emit `N` would model a deploy that
\* "already knows" the fresh name, which is exactly what unforgeability
\* forbids.
SourceNames == {"XName"} \cup ForgeableChannels \cup Payloads

\* Bounded-depth term generator for initial source programs. The
\* universe of bodies is enumerated up to depth 2, which is enough to
\* exhibit all three defect classes without exploding the state space.
RECURSIVE Term(_)
Term(depth) ==
    IF depth = 0 THEN {[kind |-> "Nil"]}
    ELSE
        LET sub == Term(depth - 1) IN
        {[kind |-> "Nil"]}
        \cup { [kind |-> "Send", chan |-> c, payload |-> p] :
               c \in SourceNames, p \in SourceNames }
        \cup { [kind |-> "Receive", chan |-> c, body |-> b] :
               c \in SourceNames, b \in sub }
        \cup { [kind |-> "Par", left |-> l, right |-> r] :
               l \in sub, r \in sub }

\* Bodies the outer New can wrap, bounded by MaxTermDepth. No inner
\* New — one binder is enough to witness the three defects; multi-
\* binder generalization is a Rocq obligation
\* (`docs/rholang/garbage-collection-formalization-plan.md` item 2.8
\* follow-up).
BodyUniverse == Term(MaxTermDepth)

----------------------------------------------------------------------------
(***************************************************************************)
(* The classifier IsScopeTrapped — the model's mirror of                   *)
(* `is_scope_trapped` in rholang/src/rust/interpreter/garbage.rs.          *)
(*                                                                         *)
(* Returns TRUE iff every occurrence of `x` in `P` sits in a safe          *)
(* position per the Rust implementation, subject to the H1_* knobs that    *)
(* weaken specific rejection clauses for the pre-fix regression            *)
(* controls.                                                                *)
(***************************************************************************)

RECURSIVE OccursInChanPosition(_, _)
RECURSIVE OccursInPayloadPosition(_, _)
RECURSIVE OccursInReceiveBody(_, _)

\* Is x used as a channel somewhere in P? (Send.chan or Receive.chan.)
OccursInChanPosition(x, P) ==
    CASE P.kind = "Nil"     -> FALSE
      [] P.kind = "Send"    -> P.chan = x
      [] P.kind = "Receive" -> P.chan = x
                               \/ OccursInChanPosition(x, P.body)
      [] P.kind = "Par"     -> OccursInChanPosition(x, P.left)
                               \/ OccursInChanPosition(x, P.right)
      [] P.kind = "New"     -> OccursInChanPosition(x, P.body)

\* Is x used as a payload somewhere in P? (Send.payload.)
OccursInPayloadPosition(x, P) ==
    CASE P.kind = "Nil"     -> FALSE
      [] P.kind = "Send"    -> P.payload = x
      [] P.kind = "Receive" -> OccursInPayloadPosition(x, P.body)
      [] P.kind = "Par"     -> OccursInPayloadPosition(x, P.left)
                               \/ OccursInPayloadPosition(x, P.right)
      [] P.kind = "New"     -> OccursInPayloadPosition(x, P.body)

\* Does x appear ANYWHERE inside a Receive body in P?
OccursInReceiveBody(x, P) ==
    CASE P.kind = "Nil"     -> FALSE
      [] P.kind = "Send"    -> FALSE
      [] P.kind = "Receive" -> OccursInChanPosition(x, P.body)
                               \/ OccursInPayloadPosition(x, P.body)
                               \/ OccursInReceiveBody(x, P.body)
      [] P.kind = "Par"     -> OccursInReceiveBody(x, P.left)
                               \/ OccursInReceiveBody(x, P.right)
      [] P.kind = "New"     -> OccursInReceiveBody(x, P.body)

IsScopeTrapped(x, P) ==
    /\ \* Payload rejection: x may not appear in `Send.data`. Toggle via
       \* H1_RejectsPayload.
       (H1_RejectsPayload => ~OccursInPayloadPosition(x, P))
    /\ \* Full receive-body rejection: x may not appear anywhere inside
       \* a `Receive.body`. Toggle via H1_RejectsReceiveBody.
       (H1_RejectsReceiveBody => ~OccursInReceiveBody(x, P))

----------------------------------------------------------------------------
(***************************************************************************)
(* Substitution — replace every occurrence of the bound name token with    *)
(* the freshly-minted unforgeable N.                                       *)
(***************************************************************************)

RECURSIVE Subst(_, _, _)
Subst(from, to, P) ==
    CASE P.kind = "Nil"     -> P
      [] P.kind = "Send"    -> [kind |-> "Send",
                                chan |-> IF P.chan = from THEN to ELSE P.chan,
                                payload |-> IF P.payload = from THEN to ELSE P.payload]
      [] P.kind = "Receive" -> [kind |-> "Receive",
                                chan |-> IF P.chan = from THEN to ELSE P.chan,
                                body |-> Subst(from, to, P.body)]
      [] P.kind = "Par"     -> [kind |-> "Par",
                                left |-> Subst(from, to, P.left),
                                right |-> Subst(from, to, P.right)]
      [] P.kind = "New"     -> \* Nested news aren't in the generator, but
                                \* keeping Subst total is cheap and useful.
                                [kind |-> "New",
                                 body |-> Subst(from, to, P.body)]

----------------------------------------------------------------------------
(***************************************************************************)
(* Operational semantics                                                   *)
(*                                                                         *)
(* State:                                                                  *)
(*   program   — current process being reduced                             *)
(*   parkData  — channel -> seq of payloads (parked sends)                 *)
(*   parkCont  — channel -> seq of receive bodies (parked continuations)   *)
(*   steps     — counter, bounded by MaxReductionSteps                     *)
(*   trapped   — whether the classifier accepted the INITIAL program;      *)
(*               fixed after Init.                                         *)
(*                                                                         *)
(* Reduction rules (one step each):                                        *)
(*                                                                         *)
(*   Alloc  — the outer `New(body)` becomes `body[XName <- N]`             *)
(*   PubS   — a top-level Par-exposed Send(c, p) parks on c                *)
(*   PubR   — a top-level Par-exposed Receive(c, body) parks on c          *)
(*   Comm   — a parked send on c matches a parked receive on c; the       *)
(*             receive's body is re-emitted into the live process (and    *)
(*             the send-payload binding is NOT modelled — our bodies are  *)
(*             pattern-free, the model's payload is just a value token)   *)
(*   ExtFire — a parked receive on a ForgeableChannel is fired by an      *)
(*             external deploy; its body is re-emitted. ExtFire is the    *)
(*             transition the holed prose proof forgot about.             *)
(*                                                                         *)
(* No scheduler fairness is required: the invariant we check is pure      *)
(* safety.                                                                 *)
(***************************************************************************)

VARIABLES program, parkData, parkCont, steps, trapped, newAllocated

vars == <<program, parkData, parkCont, steps, trapped, newAllocated>>

\* Starting shape: `new XName in body` for every body in BodyUniverse.
\* TLC enumerates over the set, so every reachable
\*   (initial-body, classifier-verdict, reduction-trace) triple is
\* explored. `newAllocated` tracks whether AllocFresh has fired, so the
\* outer `new` reduces exactly once.
Init ==
    /\ \E body \in BodyUniverse :
            /\ program = [kind |-> "New", body |-> body]
            /\ trapped = IsScopeTrapped("XName", body)
    /\ parkData = [c \in Names |-> <<>>]
    /\ parkCont = [c \in Names |-> <<>>]
    /\ steps = 0
    /\ newAllocated = FALSE

----------------------------------------------------------------------------
\* Decompose a Par-tree into the multiset of top-level leaves exposed to
\* the scheduler. In TLA+ we model this as the sequence of leaves via a
\* left-to-right walk; the scheduler picks one.

RECURSIVE Leaves(_)
Leaves(P) ==
    CASE P.kind = "Par" -> Leaves(P.left) \o Leaves(P.right)
      [] OTHER          -> << P >>

\* Replace the first occurrence of a specific leaf in a Par-tree with its
\* residue (Nil if fully consumed, or another term if the leaf partially
\* reduces).  We approximate by rebuilding program from the leaf sequence.
RECURSIVE RebuildFromLeaves(_)
RebuildFromLeaves(leafSeq) ==
    IF leafSeq = <<>> THEN [kind |-> "Nil"]
    ELSE IF Len(leafSeq) = 1 THEN Head(leafSeq)
    ELSE [kind |-> "Par",
          left |-> Head(leafSeq),
          right |-> RebuildFromLeaves(Tail(leafSeq))]

\* Remove the i-th leaf, leaving the rest as the residue program.
RemoveLeaf(i) ==
    LET ls == Leaves(program)
    IN RebuildFromLeaves([j \in 1..(Len(ls) - 1) |->
                            IF j < i THEN ls[j] ELSE ls[j + 1]])

----------------------------------------------------------------------------
\* Actions.

AllocFresh ==
    /\ steps < MaxReductionSteps
    /\ ~newAllocated
    /\ program.kind = "New"
    /\ program' = Subst("XName", N, program.body)
    /\ newAllocated' = TRUE
    /\ steps' = steps + 1
    /\ UNCHANGED <<parkData, parkCont, trapped>>

ParkOne ==
    /\ steps < MaxReductionSteps
    /\ newAllocated
    /\ Leaves(program) /= <<>>
    /\ \E i \in 1..Len(Leaves(program)) :
            LET L == Leaves(program)[i] IN
            \/ \* Park a Send
               /\ L.kind = "Send"
               /\ parkData' = [parkData EXCEPT ![L.chan] = Append(@, L.payload)]
               /\ program' = RemoveLeaf(i)
               /\ steps' = steps + 1
               /\ UNCHANGED <<parkCont, trapped, newAllocated>>
            \/ \* Park a Receive
               /\ L.kind = "Receive"
               /\ parkCont' = [parkCont EXCEPT ![L.chan] = Append(@, L.body)]
               /\ program' = RemoveLeaf(i)
               /\ steps' = steps + 1
               /\ UNCHANGED <<parkData, trapped, newAllocated>>

Comm ==
    /\ steps < MaxReductionSteps
    /\ newAllocated
    /\ \E c \in Names :
            /\ Len(parkData[c]) > 0
            /\ Len(parkCont[c]) > 0
            /\ LET body == Head(parkCont[c]) IN
               program' = IF program.kind = "Nil"
                          THEN body
                          ELSE [kind |-> "Par", left |-> program, right |-> body]
            /\ parkData' = [parkData EXCEPT ![c] = Tail(@)]
            /\ parkCont' = [parkCont EXCEPT ![c] = Tail(@)]
            /\ steps' = steps + 1
            /\ UNCHANGED <<trapped, newAllocated>>

\* External firing — the case Rule (B) covered: an external deploy
\* fires a parked receive on a forgeable channel by sending a payload.
\* The external payload token is not `N` (external deploys don't have
\* `N`); it's an arbitrary Payload. The receive's body is emitted into
\* the live process. This is how a scope-trapped `N` can leak if it
\* textually appears inside the body — the hole in the holed proof.
ExtFire ==
    /\ steps < MaxReductionSteps
    /\ newAllocated
    /\ \E c \in ForgeableChannels :
            /\ Len(parkCont[c]) > 0
            /\ LET body == Head(parkCont[c]) IN
               program' = IF program.kind = "Nil"
                          THEN body
                          ELSE [kind |-> "Par", left |-> program, right |-> body]
            /\ parkCont' = [parkCont EXCEPT ![c] = Tail(@)]
            /\ steps' = steps + 1
            /\ UNCHANGED <<parkData, trapped, newAllocated>>

Next == AllocFresh \/ ParkOne \/ Comm \/ ExtFire

Spec == Init /\ [][Next]_vars

----------------------------------------------------------------------------
(***************************************************************************)
(* Invariants                                                               *)
(*                                                                         *)
(* We only require the invariants to hold when the classifier accepted    *)
(* the initial program (trapped = TRUE). The pre-fix controls weaken the  *)
(* classifier, so the SET of programs for which trapped = TRUE grows to   *)
(* include programs that genuinely leak N.                                *)
(*                                                                         *)
(* Inv_N_NeverInLiveProgram — N is never in a live Send payload or any    *)
(*   Receive body position inside the current `program`.                  *)
(*                                                                         *)
(* Inv_N_NeverInParkedData — no parked datum has N as its payload.        *)
(*                                                                         *)
(* Inv_N_NeverInParkedBodyOnForgeable — no parked continuation on a       *)
(*   forgeable channel has N textually inside its body. This is the       *)
(*   invariant that the receive-body-on-x pre-fix violates: when the      *)
(*   classifier accepts "receive body on x" as safe, a receive on x can   *)
(*   itself emit an inner receive on a forgeable channel whose body       *)
(*   mentions N — and firing that outer receive moves the inner receive  *)
(*   onto a forgeable park slot, triggering this invariant.               *)
(***************************************************************************)

LiveNeverInPayload ==
    ~OccursInPayloadPosition(N, program)
    /\ ~OccursInReceiveBody(N, program)

Inv_N_NeverInLiveProgram ==
    trapped => LiveNeverInPayload

Inv_N_NeverInParkedData ==
    trapped => \A c \in Names :
                   \A i \in 1..Len(parkData[c]) : parkData[c][i] /= N

RECURSIVE BodyMentionsN(_)
BodyMentionsN(body) ==
    CASE body.kind = "Nil"     -> FALSE
      [] body.kind = "Send"    -> body.chan = N
                                  \/ body.payload = N
      [] body.kind = "Receive" -> body.chan = N
                                  \/ BodyMentionsN(body.body)
      [] body.kind = "Par"     -> BodyMentionsN(body.left)
                                  \/ BodyMentionsN(body.right)
      [] body.kind = "New"     -> BodyMentionsN(body.body)

Inv_N_NeverInParkedBodyOnForgeable ==
    trapped =>
        \A c \in ForgeableChannels :
            \A i \in 1..Len(parkCont[c]) : ~BodyMentionsN(parkCont[c][i])

\* Top-level invariant: conjunction of the three.
Inv_ScopeTrappedSafe ==
    /\ Inv_N_NeverInLiveProgram
    /\ Inv_N_NeverInParkedData
    /\ Inv_N_NeverInParkedBodyOnForgeable

============================================================================

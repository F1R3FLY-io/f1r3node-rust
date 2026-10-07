---------------------- MODULE CostEventCodecEpoch ----------------------
EXTENDS Naturals, Sequences

\* Finite field-order witness for the event/cold-store codec merge. Each B
\* token stands for the EXACT bytes of one positional serde field, including
\* its tag/count/payload; C expands a nested message in place. The old-field
\* tokens are deliberately shared by both modes. Rust differential tests
\* against the pre-merge 52217b94a encoder must establish each token's bytes;
\* this model checks routing and order, not the Rust implementation or hash.
\*
\* The epoch is an authenticated input from replay/activation context. It is
\* NEVER inferred by looking at a term or by trying one decoder then another.
Epochs == {"Legacy", "CostV1"}
AuthenticatedContexts == Epochs \cup {"Missing"}
Roots == {"par", "bind", "list", "continuation"}
EventKinds == {"Produce", "Consume"}
CostSites == {"root", "child", "set-key", "map-key", "map-value"}
Domain == "f1r3node:rspace-event-cost:v1"
KeyDomain == "f1r3node:rspace-channel-key-cost:v1"

B(token) == [kind |-> "B", token |-> token]
C(node) == [kind |-> "C", node |-> node]

ParProgram(withReceive) ==
    <<B("P.sends"), B("P.receives")>>
    \o (IF withReceive THEN <<C("receive")>> ELSE <<>>)
    \o <<B("P.news"), B("P.exprs"), B("P.matches"),
         B("P.unforgeables"), B("P.bundles"), B("P.connectives"),
         B("P.conditionals"), B("P.locally_free"),
         B("P.connective_used")>>

\* These are the pre-merge generated Rust struct positions, NOT protobuf tag
\* order. In particular TaggedContinuation was guard, tagged_cont.
LegacyProgram(node) ==
    CASE node = "par" -> ParProgram(TRUE)
      [] node = "plain-par" -> ParProgram(FALSE)
      [] node = "receive" ->
             <<B("R.binds"), C("bind"), B("R.body"),
               B("R.persistent"), B("R.peek"), B("R.bind_count"),
               B("R.locally_free"), B("R.connective_used"),
               B("R.condition")>>
      [] node = "bind" ->
             <<B("B.patterns"), C("plain-par"), B("B.source"),
               B("B.remainder"), B("B.free_count")>>
      [] node = "list" ->
             <<B("L.pars"), C("plain-par"), B("L.random_state")>>
      [] node = "continuation" ->
             <<B("K.guard"), C("plain-par"), B("K.tagged_cont"),
               C("par-with-random")>>
      [] node = "par-with-random" ->
             <<B("PW.body"), C("plain-par"), B("PW.random_state")>>

CostToken(field, present) == [field |-> field, present |-> present]

\* CostV1 retains all old fields in their old order. Par/Bind/List gain
\* suffixes, but the generated TaggedContinuation inserts cost_authority
\* BETWEEN guard and tagged_cont. A blanket append-only rule is false.
CostProgram(node, present) ==
    CASE node = "par" \/ node = "plain-par" ->
             LegacyProgram(node)
             \o <<B(CostToken("P.cost_signed_terms", present)),
                  B(CostToken("P.cost_stacks", present))>>
      [] node = "bind" ->
             LegacyProgram(node)
             \o <<B(CostToken("B.cost_signature", present))>>
      [] node = "list" ->
             LegacyProgram(node)
             \o <<B(CostToken("L.cost_authority", present)),
                  B(CostToken("L.cost_stack", present))>>
      [] node = "continuation" ->
             <<B("K.guard"), C("plain-par"),
               B(CostToken("K.cost_authority", present)),
               B("K.tagged_cont"), C("par-with-random")>>
      [] OTHER -> LegacyProgram(node)

Program(node, epoch, present) ==
    IF epoch = "Legacy" THEN LegacyProgram(node)
    ELSE CostProgram(node, present)

RECURSIVE RefNode(_, _, _), RefProgram(_, _, _)
RefNode(node, epoch, present) == RefProgram(Program(node, epoch, present), epoch, present)
RefProgram(program, epoch, present) ==
    IF Len(program) = 0 THEN <<>>
    ELSE IF Head(program).kind = "B" THEN
             <<Head(program).token>> \o RefProgram(Tail(program), epoch, present)
         ELSE RefNode(Head(program).node, epoch, present)
              \o RefProgram(Tail(program), epoch, present)

OldReference(root) == RefNode(root, "Legacy", FALSE)
Expected(root, epoch, present) ==
    RefNode(root, epoch, present)

\* The event hash itself covers bincode(Vec<Vec<u8>>), not merely a rhoapi
\* root. `Body` stands for one COMPLETE encoded datum/pattern/continuation
\* leg. Channel digests and patterns are already sorted by the caller. A
\* CostV1 domain is one extra first vector member, so stable and native
\* source builders must agree on the same vector arity and ordering.
Body(bytes) == [kind |-> "body", bytes |-> bytes]
LegacyEvent(kind, bytes) ==
    IF kind = "Produce" THEN <<"channel-digest", Body(bytes), "persist">>
    ELSE <<"channel-digest", "sorted-patterns", Body(bytes), "persist">>
EventPreimage(kind, epoch, bytes) ==
    (IF epoch = "CostV1" THEN <<Domain>> ELSE <<>>)
    \o LegacyEvent(kind, bytes)

\* Route A: preserve the existing ordinary channel key in BOTH epochs, and
\* domain-tag every cost-bearing channel. `costPresent` is a recursive fold of
\* the entire canonical Par, not merely its root fields. This finite witness
\* includes ordinary children, decoded EPathMap set/map keys and map values;
\* the Rust traversal must cover every concrete Par constructor. In particular
\* opaque key bytes may never be skipped or checked by stringification.
CostTree(sites) ==
    [cost |-> "root" \in sites,
     children |-> <<
       [cost |-> "child" \in sites, children |-> <<>>],
       [cost |-> FALSE, children |-> <<
         [cost |-> "set-key" \in sites, children |-> <<>>],
         [cost |-> "map-key" \in sites, children |-> <<>>],
         [cost |-> "map-value" \in sites, children |-> <<>>]
       >>]
     >>]
RECURSIVE RecursiveCostPresent(_)
RecursiveCostPresent(node) ==
    node.cost \/ \E i \in 1..Len(node.children): RecursiveCostPresent(node.children[i])
ChannelProjection(epoch, present) ==
    IF epoch = "Legacy" \/ ~present THEN OldReference("par")
    ELSE <<KeyDomain>> \o RefNode("par", "CostV1", TRUE)
LiveChannelProjection(epoch, present) == ChannelProjection(epoch, present)
ReplayChannelProjection(epoch, present) == ChannelProjection(epoch, present)

VARIABLES authenticatedEpoch, epoch, costSites, root, eventKind, costPresent, todo, output, status
vars == <<authenticatedEpoch, epoch, costSites, root, eventKind, costPresent, todo, output, status>>

Init ==
    /\ authenticatedEpoch \in AuthenticatedContexts
    /\ epoch = authenticatedEpoch
    /\ costSites \in SUBSET CostSites
    /\ root \in Roots
    /\ eventKind \in EventKinds
    /\ costPresent = RecursiveCostPresent(CostTree(costSites))
    /\ output = <<>>
    /\ IF epoch = "Missing" \/ (epoch = "Legacy" /\ costPresent)
       THEN /\ todo = <<>> /\ status = "rejected"
       ELSE /\ todo = Program(root, epoch, costPresent)
            /\ status = "running"

Emit ==
    /\ status = "running" /\ Len(todo) > 0 /\ Head(todo).kind = "B"
    /\ output' = Append(output, Head(todo).token)
    /\ todo' = Tail(todo)
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind, costPresent, status>>

Descend ==
    /\ status = "running" /\ Len(todo) > 0 /\ Head(todo).kind = "C"
    /\ todo' = Program(Head(todo).node, epoch, costPresent) \o Tail(todo)
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind, costPresent, output, status>>

Finish ==
    /\ status = "running" /\ Len(todo) = 0
    /\ status' = "accepted"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind, costPresent, todo, output>>

\* Accepted and rejected are terminal observations. Make their stutter
\* explicit so TLC's deadlock check distinguishes intended termination from
\* a missing decoder transition.
Stop ==
    /\ status \in {"accepted", "rejected"}
    /\ UNCHANGED vars

Next == Emit \/ Descend \/ Finish \/ Stop
Spec == Init /\ [][Next]_vars

Inv_AuthenticatedEpoch == epoch = authenticatedEpoch
\* A consensus session without the adopted-genesis policy is rejected before
\* emitting any event bytes. It may not fall back to the unversioned codec.
Inv_NoMissingPolicyFallback ==
    authenticatedEpoch = "Missing" => status = "rejected" /\ output = <<>>
Inv_Prefix == output = SubSeq(Expected(root, epoch, costPresent), 1, Len(output))
Inv_FailClosedLegacy ==
    epoch = "Legacy" /\ costPresent => status = "rejected" /\ output = <<>>
Inv_LegacyIdentity ==
    status = "accepted" /\ epoch = "Legacy" => output = OldReference(root)
Inv_CostDomain ==
    status = "accepted" /\ epoch = "CostV1" =>
        EventPreimage(eventKind, "CostV1", output)
            # EventPreimage(eventKind, "Legacy", output)
Inv_Complete ==
    status = "accepted" => output = Expected(root, epoch, costPresent)
Inv_LegacyEventIdentity ==
    status = "accepted" /\ epoch = "Legacy" =>
        EventPreimage(eventKind, epoch, output)
            = LegacyEvent(eventKind, OldReference(root))
Inv_CostEventDomain ==
    status = "accepted" /\ epoch = "CostV1" =>
        Head(EventPreimage(eventKind, epoch, output)) = Domain
Inv_ChannelProjectionAgreement ==
    LiveChannelProjection(epoch, costPresent)
        = ReplayChannelProjection(epoch, costPresent)
Inv_RecursiveCostPresence ==
    costPresent = RecursiveCostPresent(CostTree(costSites))
Inv_LegacyChannelIdentity ==
    status = "accepted" /\ epoch = "Legacy" =>
        ChannelProjection(epoch, costPresent) = OldReference("par")
Inv_StableOrdinaryChannelIdentity ==
    status = "accepted" /\ epoch = "CostV1" /\ ~costPresent =>
        ChannelProjection(epoch, costPresent) = OldReference("par")
Inv_CostChannelSeparation ==
    status = "accepted" /\ epoch = "CostV1" /\ costPresent =>
        /\ Head(ChannelProjection(epoch, costPresent)) = KeyDomain
        /\ ChannelProjection(epoch, costPresent) # OldReference("par")
=======================================================================

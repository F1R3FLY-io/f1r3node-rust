---------------------- MODULE CostEventCodecEpoch ----------------------
EXTENDS Naturals, Sequences

\* Finite field-order witness for the event/cold-store codec merge. Each B
\* token stands for the EXACT bytes of one positional serde field, including
\* its tag/count/payload; C expands a nested message in place. The old-field
\* tokens are deliberately shared by all three modes. Rust differential tests
\* against the v5 52217b94a and v6 37730e1 encoders must establish each
\* token's bytes; this model checks routing and order, not the Rust hash.
\*
\* The epoch is an authenticated input from replay/activation context. It is
\* NEVER inferred by looking at a term or by trying one decoder then another.
Epochs == {"Legacy", "CostV1", "CostV2"}
CostEpochs == {"CostV1", "CostV2"}
AuthenticatedContexts == Epochs \cup {"Missing"}
Roots == {"par", "bind", "list", "continuation"}
EventKinds == {"Produce", "Consume"}
CostSites == {"root", "child", "set-key", "map-key", "map-value"}
Domain == "f1r3node:rspace-event-cost:v1"
KeyDomain == "f1r3node:rspace-channel-key-cost:v1"
Routes == <<"play", "replay", "native", "cold">>
CandidateKinds == {"AllLegacy", "Offered"}

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

\* A CostStack is a positional bincode node, not a protobuf message. The v6
\* CostV1 node has ONLY its cells field, even for an empty parallel tail. The
\* v7 CostV2 node always emits the new tail-vector field (including its empty
\* count). This is a typed field-token program: it does not assert that the
\* two raw bincode byte languages are disjoint under a wrong decoder. The
\* authenticated epoch chooses the decoder before any positional bytes read.
StackProgram(epoch, branched) ==
    <<B("S.cells")>>
    \o (IF epoch = "CostV2"
        THEN <<B(CostToken("S.parallel_tail", branched))>>
        ELSE <<>>)
CostStackField(field, present, epoch, branched) ==
    IF epoch = "CostV2" /\ present
    THEN B([field |-> field, present |-> present,
            elementProgram |-> StackProgram(epoch, branched)])
    ELSE B(CostToken(field, present))
\* This is typed positional-field consumption, not an assertion about raw
\* byte-language disjointness. The cold route's authenticated epoch selects
\* which field program the decoder consumes; v6 never reads a tail count.
StackDecoderAccepts(epoch, branched, fields) ==
    fields = StackProgram(epoch, branched)

\* CostV1 retains all old fields in their old order. Par/Bind/List gain
\* suffixes, but the generated TaggedContinuation inserts cost_authority
\* BETWEEN guard and tagged_cont. A blanket append-only rule is false.
CostProgram(node, present, stackPresent, epoch, branched) ==
    CASE node = "par" \/ node = "plain-par" ->
             LegacyProgram(node)
             \o <<B(CostToken("P.cost_signed_terms", present)),
                  CostStackField("P.cost_stacks", stackPresent, epoch, branched)>>
      [] node = "bind" ->
             LegacyProgram(node)
             \o <<B(CostToken("B.cost_signature", present))>>
      [] node = "list" ->
             LegacyProgram(node)
             \o <<B(CostToken("L.cost_authority", present)),
                  CostStackField("L.cost_stack", stackPresent, epoch, branched)>>
      [] node = "continuation" ->
             <<B("K.guard"), C("plain-par"),
               B(CostToken("K.cost_authority", present)),
               B("K.tagged_cont"), C("par-with-random")>>
      [] OTHER -> LegacyProgram(node)

Program(node, epoch, present, stackPresent, branched) ==
    IF epoch = "Legacy" THEN LegacyProgram(node)
    ELSE CostProgram(node, present, stackPresent, epoch, branched)

RECURSIVE RefNode(_, _, _, _, _), RefProgram(_, _, _, _, _)
RefNode(node, epoch, present, stackPresent, branched) ==
    RefProgram(Program(node, epoch, present, stackPresent, branched),
               epoch, present, stackPresent, branched)
RefProgram(program, epoch, present, stackPresent, branched) ==
    IF Len(program) = 0 THEN <<>>
    ELSE IF Head(program).kind = "B" THEN
             <<Head(program).token>>
             \o RefProgram(Tail(program), epoch, present, stackPresent, branched)
         ELSE RefNode(Head(program).node, epoch, present, stackPresent, branched)
              \o RefProgram(Tail(program), epoch, present, stackPresent, branched)

OldReference(root) == RefNode(root, "Legacy", FALSE, FALSE, FALSE)
Expected(root, epoch, present, stackPresent, branched) ==
    RefNode(root, epoch, present, stackPresent, branched)

\* The event hash itself covers bincode(Vec<Vec<u8>>), not merely a rhoapi
\* root. `Body` stands for one COMPLETE encoded datum/pattern/continuation
\* leg. Channel digests and patterns are already sorted by the caller. A
\* Cost-domain marker is one extra first vector member in BOTH cost epochs;
\* this domain spelling is deliberately the v6 one. Stable and native
\* source builders must agree on the same vector arity and ordering.
Body(bytes) == [kind |-> "body", bytes |-> bytes]
LegacyEvent(kind, bytes) ==
    IF kind = "Produce" THEN <<"channel-digest", Body(bytes), "persist">>
    ELSE <<"channel-digest", "sorted-patterns", Body(bytes), "persist">>
EventPreimage(kind, epoch, bytes) ==
    (IF epoch \in CostEpochs THEN <<Domain>> ELSE <<>>)
    \o LegacyEvent(kind, bytes)

\* Route A: preserve the existing ordinary channel key in ALL epochs, and
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
ChannelProjection(epoch, present, stackPresent, branched) ==
    IF epoch = "Legacy" \/ ~present THEN OldReference("par")
    ELSE <<KeyDomain>> \o RefNode("par", epoch, TRUE, stackPresent, branched)
LiveChannelProjection(epoch, present, stackPresent, branched) ==
    ChannelProjection(epoch, present, stackPresent, branched)
ReplayChannelProjection(epoch, present, stackPresent, branched) ==
    ChannelProjection(epoch, present, stackPresent, branched)

\* A replay shortcut is valid only for the codec that produced its event log.
\* Parent/payload identity alone cannot distinguish v5, v6, and v7 logs.
ReplayCacheKey(parent, sender, sequence, payload, codec) ==
    <<parent, sender, sequence, payload, codec>>

VARIABLES authenticatedEpoch, epoch, costSites, root, eventKind, costPresent,
          stackPresent, branchPresent,
          candidateKind, routesBound, todo, output, status, rootVersion, logVersion
vars == <<authenticatedEpoch, epoch, costSites, root, eventKind, costPresent,
          stackPresent, branchPresent,
          candidateKind, routesBound, todo, output, status, rootVersion, logVersion>>

Init ==
    /\ authenticatedEpoch \in AuthenticatedContexts
    /\ epoch = authenticatedEpoch
    /\ costSites \in SUBSET CostSites
    /\ root \in Roots
    /\ eventKind \in EventKinds
    /\ candidateKind \in CandidateKinds
    /\ costPresent = RecursiveCostPresent(CostTree(costSites))
    /\ stackPresent \in BOOLEAN
    /\ branchPresent \in BOOLEAN
    /\ (stackPresent => costPresent)
    /\ (branchPresent => stackPresent)
    /\ routesBound = <<>>
    /\ output = <<>>
    /\ todo = <<>>
    /\ status = "routing"
    /\ rootVersion = 0
    /\ logVersion = 0

RejectMissing ==
    /\ status = "routing" /\ epoch = "Missing"
    /\ status' = "rejected"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output,
                   rootVersion, logVersion>>

RejectLegacyCost ==
    /\ status = "routing" /\ epoch = "Legacy" /\ costPresent
    /\ status' = "rejected"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output,
                   rootVersion, logVersion>>

RejectCostV1Branch ==
    /\ status = "routing" /\ epoch = "CostV1" /\ branchPresent
    /\ status' = "rejected"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output,
                   rootVersion, logVersion>>

BindRoute ==
    /\ status = "routing" /\ epoch \in Epochs
    /\ ~(epoch = "Legacy" /\ costPresent)
    /\ ~(epoch = "CostV1" /\ branchPresent)
    /\ Len(routesBound) < Len(Routes)
    /\ routesBound' = Append(routesBound,
          [route |-> Routes[Len(routesBound) + 1], epoch |-> epoch])
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   todo, output, status,
                   rootVersion, logVersion>>

StartEncoding ==
    /\ status = "routing" /\ Len(routesBound) = Len(Routes)
    /\ todo' = Program(root, epoch, costPresent, stackPresent, branchPresent)
    /\ status' = "running"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, output,
                   rootVersion, logVersion>>

Emit ==
    /\ status = "running" /\ Len(todo) > 0 /\ Head(todo).kind = "B"
    /\ output' = Append(output, Head(todo).token)
    /\ todo' = Tail(todo)
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, status,
                   rootVersion, logVersion>>

Descend ==
    /\ status = "running" /\ Len(todo) > 0 /\ Head(todo).kind = "C"
    /\ todo' = Program(Head(todo).node, epoch, costPresent,
                       stackPresent, branchPresent)
                \o Tail(todo)
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, output, status,
                   rootVersion, logVersion>>

Finish ==
    /\ status = "running" /\ Len(todo) = 0
    /\ status' = "prepared"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output,
                   rootVersion, logVersion>>

Commit ==
    /\ status = "prepared"
    /\ status' = "accepted"
    /\ rootVersion' = 1
    /\ logVersion' = 1
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output>>

Abort ==
    /\ status \in {"routing", "running", "prepared"}
    /\ epoch \in Epochs
    /\ ~(epoch = "Legacy" /\ costPresent)
    /\ ~(epoch = "CostV1" /\ branchPresent)
    /\ status' = "rejected"
    /\ UNCHANGED <<authenticatedEpoch, epoch, costSites, root, eventKind,
                   costPresent, stackPresent, branchPresent, candidateKind,
                   routesBound, todo, output,
                   rootVersion, logVersion>>

\* Accepted and rejected are terminal observations. Make their stutter
\* explicit so TLC's deadlock check distinguishes intended termination from
\* a missing decoder transition.
Stop ==
    /\ status \in {"accepted", "rejected"}
    /\ UNCHANGED vars

Next == RejectMissing \/ RejectLegacyCost \/ RejectCostV1Branch
        \/ BindRoute \/ StartEncoding
        \/ Emit \/ Descend \/ Finish \/ Commit \/ Abort \/ Stop
Spec == Init /\ [][Next]_vars

Inv_AuthenticatedEpoch == epoch = authenticatedEpoch
\* A consensus session without the adopted-genesis policy is rejected before
\* emitting any event bytes. It may not fall back to the unversioned codec.
Inv_NoMissingPolicyFallback ==
    authenticatedEpoch = "Missing" =>
        /\ status \in {"routing", "rejected"}
        /\ routesBound = <<>> /\ output = <<>>
Inv_Prefix ==
    output = SubSeq(Expected(root, epoch, costPresent,
                             stackPresent, branchPresent), 1, Len(output))
Inv_FailClosedLegacy ==
    epoch = "Legacy" /\ costPresent =>
        /\ status \in {"routing", "rejected"}
        /\ routesBound = <<>> /\ output = <<>>
Inv_FailClosedCostV1Branch ==
    epoch = "CostV1" /\ branchPresent =>
        /\ status \in {"routing", "rejected"}
        /\ routesBound = <<>> /\ output = <<>>
Inv_CostV1StackOldLayout ==
    StackProgram("CostV1", branchPresent) = <<B("S.cells")>>
Inv_CostV2StackTailLayout ==
    StackProgram("CostV2", branchPresent) =
        <<B("S.cells"), B(CostToken("S.parallel_tail", branchPresent))>>
Inv_LegacyIdentity ==
    status = "accepted" /\ epoch = "Legacy" => output = OldReference(root)
Inv_CostDomain ==
    status = "accepted" /\ epoch \in CostEpochs =>
        EventPreimage(eventKind, epoch, output)
            # EventPreimage(eventKind, "Legacy", output)
Inv_Complete ==
    status = "accepted" =>
        output = Expected(root, epoch, costPresent, stackPresent, branchPresent)
Inv_LegacyEventIdentity ==
    status = "accepted" /\ epoch = "Legacy" =>
        EventPreimage(eventKind, epoch, output)
            = LegacyEvent(eventKind, OldReference(root))
Inv_CostEventDomain ==
    status = "accepted" /\ epoch \in CostEpochs =>
        Head(EventPreimage(eventKind, epoch, output)) = Domain
Inv_ChannelProjectionAgreement ==
    Len(routesBound) >= 2 =>
        LiveChannelProjection(routesBound[1].epoch, costPresent,
                              stackPresent, branchPresent)
            = ReplayChannelProjection(routesBound[2].epoch, costPresent,
                                      stackPresent, branchPresent)
Inv_AllRoutesAuthenticated ==
    \A i \in 1..Len(routesBound):
        /\ routesBound[i].route = Routes[i]
        /\ routesBound[i].epoch = authenticatedEpoch
Inv_AllLegacyFastPath ==
    candidateKind = "AllLegacy" /\ Len(routesBound) >= 2 =>
        /\ routesBound[1].epoch = authenticatedEpoch
        /\ routesBound[2].epoch = authenticatedEpoch
Inv_NativeColdProjectionAgreement ==
    Len(routesBound) = Len(Routes) =>
        /\ ChannelProjection(routesBound[3].epoch, costPresent,
                             stackPresent, branchPresent)
             = ChannelProjection(routesBound[4].epoch, costPresent,
                                 stackPresent, branchPresent)
        /\ routesBound[3].epoch = routesBound[1].epoch
Inv_AuthenticatedStackDecoder ==
    Len(routesBound) = Len(Routes) =>
        StackDecoderAccepts(routesBound[4].epoch, branchPresent,
                            StackProgram(epoch, branchPresent))
Inv_BoundBeforeEncoding ==
    status \in {"running", "prepared", "accepted"} =>
        Len(routesBound) = Len(Routes)
Inv_AtomicPublication ==
    /\ rootVersion = logVersion
    /\ (status = "accepted" <=> rootVersion = 1)
Inv_NoMutationOnRejection ==
    status = "rejected" => rootVersion = 0 /\ logVersion = 0
Inv_RecursiveCostPresence ==
    costPresent = RecursiveCostPresent(CostTree(costSites))
Inv_StackPresenceConsistent ==
    /\ (stackPresent => costPresent)
    /\ (branchPresent => stackPresent)
Inv_LegacyChannelIdentity ==
    status = "accepted" /\ epoch = "Legacy" =>
        ChannelProjection(epoch, costPresent, stackPresent, branchPresent)
            = OldReference("par")
Inv_StableOrdinaryChannelIdentity ==
    status = "accepted" /\ epoch \in CostEpochs /\ ~costPresent =>
        ChannelProjection(epoch, costPresent, stackPresent, branchPresent)
            = OldReference("par")
Inv_CostChannelSeparation ==
    status = "accepted" /\ epoch \in CostEpochs /\ costPresent =>
        /\ Head(ChannelProjection(epoch, costPresent,
                                  stackPresent, branchPresent)) = KeyDomain
        /\ ChannelProjection(epoch, costPresent, stackPresent, branchPresent)
             # OldReference("par")
Inv_ReplayCacheEpochSeparation ==
    epoch \in Epochs =>
        \A other \in Epochs \ {epoch}:
            ReplayCacheKey("parent", "sender", 1, "payload", epoch)
                # ReplayCacheKey("parent", "sender", 1, "payload", other)
=======================================================================

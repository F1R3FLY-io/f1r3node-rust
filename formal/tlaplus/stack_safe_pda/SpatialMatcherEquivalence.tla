---------------------- MODULE SpatialMatcherEquivalence ----------------------
EXTENDS Integers, Naturals, Sequences, FiniteSets, TLC

(******************************************************************************
Finite operational cross-check for the stateful matcher algebra mechanized in
Rocq's SpatialMatcher.v.  The source semantics is recursive.  The implementation
model is a post-order PDA with explicit program counter and value stack.

The corpus includes success/refusal, two values for the same free-variable
level (so conflicting bindings are reachable), sequencing/conjunction,
ordered retrying disjunction, and negation.  Every disjunction arm is isolated:
success publishes only the verdict and restores the original binding snapshot,
just as failure does before retry.  Negation likewise restores its snapshot.
******************************************************************************)

LeafLabels == {"ExactOk", "ExactFail", "Bind0A", "Bind0B", "Bind1A"}
ControlLabels == {"Sequence", "Conjunction", "Disjunction", "Negation"}

BindingAtoms == {<<0, 0>>, <<0, 1>>, <<1, 0>>}
Result(ok, bindings) == [ok |-> ok, bindings |-> bindings]
Fail == Result(FALSE, {})
Results == {[ok |-> ok, bindings |-> bindings]
              : ok \in BOOLEAN, bindings \in SUBSET BindingAtoms}

LeafResult(label) ==
    CASE label = "ExactOk"   -> Result(TRUE, {})
      [] label = "ExactFail" -> Fail
      [] label = "Bind0A"    -> Result(TRUE, {<<0, 0>>})
      [] label = "Bind0B"    -> Result(TRUE, {<<0, 1>>})
      [] label = "Bind1A"    -> Result(TRUE, {<<1, 0>>})

HasConflict(bindings) ==
    \E left, right \in bindings :
       left[1] = right[1] /\ left[2] # right[2]

UnionBindings(children) ==
    UNION {children[index].bindings : index \in 1..Len(children)}

AllSuccessful(children) ==
    \A index \in 1..Len(children) : children[index].ok

RECURSIVE IsolatedFirstSuccess(_, _)
IsolatedFirstSuccess(children, index) ==
    IF index > Len(children)
    THEN Fail
    ELSE IF children[index].ok
         THEN Result(TRUE, {})
         ELSE IsolatedFirstSuccess(children, index + 1)

Apply(label, children) ==
    IF label \in LeafLabels
    THEN LeafResult(label)
    ELSE IF label \in {"Sequence", "Conjunction"}
         THEN LET bindings == UnionBindings(children)
              IN IF AllSuccessful(children) /\ ~HasConflict(bindings)
                 THEN Result(TRUE, bindings)
                 ELSE Fail
         ELSE IF label = "Disjunction"
              THEN IsolatedFirstSuccess(children, 1)
              ELSE \* Negation: successful child means refusal; failed child
                   \* means success with the original (empty) binding delta.
                   IF Len(children) = 0 \/ ~children[1].ok
                   THEN Result(TRUE, {})
                   ELSE Fail

LeafTrees == {[label |-> label, children |-> <<>>] : label \in LeafLabels}

ChildSequences(elements) ==
    {<<>>}
    \union {<<first>> : first \in elements}
    \union {<<first, second>> : first \in elements, second \in elements}

ControlTrees(elements) ==
    {[label |-> label, children |-> children]
       : label \in ControlLabels, children \in ChildSequences(elements)}

DepthOneTrees == ControlTrees(LeafTrees)
Tree(label, children) == [label |-> label, children |-> children]
NestedWitnesses == {
    Tree("Conjunction",
         <<Tree("Bind0A", <<>>),
           Tree("Disjunction", <<Tree("ExactFail", <<>>), Tree("Bind1A", <<>>)>>)>>),
    Tree("Disjunction",
         <<Tree("ExactFail", <<>>),
           Tree("Conjunction", <<Tree("Bind0A", <<>>), Tree("Bind0B", <<>>)>>)>>),
    Tree("Negation",
         <<Tree("Conjunction", <<Tree("Bind0A", <<>>), Tree("ExactFail", <<>>)>>)>>)
}
SourceTrees == LeafTrees \union DepthOneTrees \union NestedWitnesses

RECURSIVE Compile(_), CompileChildren(_, _), RecursiveMatch(_), RecursiveChildren(_, _)

CompileChildren(children, index) ==
    IF index > Len(children)
    THEN <<>>
    ELSE Compile(children[index]) \o CompileChildren(children, index + 1)

Compile(tree) ==
    CompileChildren(tree.children, 1)
      \o <<[label |-> tree.label, arity |-> Len(tree.children)]>>

RecursiveChildren(children, index) ==
    IF index > Len(children)
    THEN <<>>
    ELSE Append(RecursiveChildren(children, index + 1), RecursiveMatch(children[index]))

RecursiveMatch(tree) ==
    Apply(tree.label,
          [index \in 1..Len(tree.children) |-> RecursiveMatch(tree.children[index])])

VARIABLES source, program, expected, pc, values
vars == <<source, program, expected, pc, values>>

Init ==
    /\ source \in SourceTrees
    /\ program = Compile(source)
    /\ expected = RecursiveMatch(source)
    /\ pc = 1
    /\ values = <<>>

ReduceStep ==
    /\ pc <= Len(program)
    /\ LET instruction == program[pc]
           arity == instruction.arity
           retained == SubSeq(values, 1, Len(values) - arity)
           children == SubSeq(values, Len(values) - arity + 1, Len(values))
       IN /\ Len(values) >= arity
          /\ values' = Append(retained, Apply(instruction.label, children))
    /\ pc' = pc + 1
    /\ UNCHANGED <<source, program, expected>>

Done == pc = Len(program) + 1

Next ==
    ReduceStep
    \/ /\ Done
       /\ UNCHANGED vars

TypeInvariant ==
    /\ source \in SourceTrees
    /\ program = Compile(source)
    /\ expected = RecursiveMatch(source)
    /\ pc \in 1..(Len(program) + 1)
    /\ values \in Seq(Results)

NoInstructionUnderflow ==
    pc <= Len(program) => Len(values) >= program[pc].arity

CompletedEquivalent == Done => values = <<expected>>

FailedBranchesCarryNoBindings ==
    \A index \in 1..Len(values) :
       ~values[index].ok => values[index].bindings = {}

(******************************************************************************
Finite cross-check for the encoded Expr outer-envelope router.  This is
deliberately independent of protobuf parsing: evidence is either absent or a
proven outer tag.  The downstream result includes Invalid so the model checks
that insufficient evidence is delegated rather than collapsed to no-match.
******************************************************************************)

ExprTags == {"Tag10", "Tag11"}
NoEvidence == "NoEvidence"
TagEvidence == ExprTags \union {NoEvidence}
RouteOutcomes == {"Accepted", "Rejected", "Invalid"}

ReferenceOuterRoute(targetEvidence, patternEvidence, downstream) ==
    IF targetEvidence \in ExprTags
       /\ patternEvidence \in ExprTags
       /\ targetEvidence # patternEvidence
    THEN "Rejected"
    ELSE downstream

ImplementationOuterDecision(targetEvidence, patternEvidence) ==
    IF targetEvidence \in ExprTags
       /\ patternEvidence \in ExprTags
       /\ targetEvidence # patternEvidence
    THEN "RejectProvenDifferent"
    ELSE "Delegate"

ImplementationOuterRoute(targetEvidence, patternEvidence, downstream) ==
    IF ImplementationOuterDecision(targetEvidence, patternEvidence)
         = "RejectProvenDifferent"
    THEN "Rejected"
    ELSE downstream

OuterEnvelopeRoutingEquivalent ==
    \A targetEvidence \in TagEvidence,
       patternEvidence \in TagEvidence,
       downstream \in RouteOutcomes :
       ReferenceOuterRoute(targetEvidence, patternEvidence, downstream)
         = ImplementationOuterRoute(targetEvidence, patternEvidence, downstream)

SameOrUnprovedDelegatesExactly ==
    \A targetEvidence \in TagEvidence,
       patternEvidence \in TagEvidence,
       downstream \in RouteOutcomes :
       (targetEvidence = NoEvidence
        \/ patternEvidence = NoEvidence
        \/ targetEvidence = patternEvidence)
       => ImplementationOuterRoute(targetEvidence, patternEvidence, downstream)
            = downstream

OnlyProvenDifferentMayShortCircuit ==
    \A targetEvidence \in TagEvidence,
       patternEvidence \in TagEvidence :
       (ImplementationOuterDecision(targetEvidence, patternEvidence)
          = "RejectProvenDifferent")
       <=> (targetEvidence \in ExprTags
            /\ patternEvidence \in ExprTags
            /\ targetEvidence # patternEvidence)

DelegatedInvalidIsNotCollapsedAbsentDifferentTags ==
    \A targetEvidence \in TagEvidence,
       patternEvidence \in TagEvidence :
       ~(targetEvidence \in ExprTags
         /\ patternEvidence \in ExprTags
         /\ targetEvidence # patternEvidence)
       => ImplementationOuterRoute(targetEvidence, patternEvidence, "Invalid")
            = "Invalid"

\* Mutation reproducing the removed rule: a proven pattern plus unproved
\* target deep parser was collapsed to semantic rejection.
CollapsedDeepFailureRoute(targetDeep, patternDeep, downstream) ==
    IF patternDeep \in ExprTags /\ targetDeep = NoEvidence
    THEN "Rejected"
    ELSE ImplementationOuterRoute(targetDeep, patternDeep, downstream)

CollapsedDeepFailureMutationIsDetected ==
    \E tag \in ExprTags :
       /\ ImplementationOuterRoute(tag, tag, "Invalid") = "Invalid"
       /\ CollapsedDeepFailureRoute(NoEvidence, tag, "Invalid") = "Rejected"

(******************************************************************************
Finite refinement model for the byte-native authoritative Connective route.

The actual protobuf recognizer remains an executable boundary.  "Parsed"
means that the cursor supplied sound evidence for a complete canonical shape;
"Unproved" delegates; and "Malformed" is rejected by canonical ParBag
admission before the encoded matcher job can be constructed.  Parsed trees run
through the same explicit instruction/value-stack PDA checked above.  The
finite nested witnesses exercise AND/OR/NOT frame composition and conflicting
bindings rather than checking only terminal connectives.
******************************************************************************)

ConnectiveAccepted(bindings) ==
    [status |-> "Accepted", bindings |-> bindings]
ConnectiveRejected == [status |-> "Rejected", bindings |-> {}]
ConnectiveInvalid == [status |-> "Invalid", bindings |-> {}]
ConnectiveRouteOutcomes ==
    {ConnectiveRejected, ConnectiveInvalid}
      \union {ConnectiveAccepted(bindings) : bindings \in SUBSET BindingAtoms}

ToConnectiveOutcome(result) ==
    IF result.ok
    THEN ConnectiveAccepted(result.bindings)
    ELSE ConnectiveRejected

RECURSIVE RunCompiled(_, _)
RunCompiled(programToRun, stack) ==
    IF Len(programToRun) = 0
    THEN stack
    ELSE LET instruction == Head(programToRun)
             arity == instruction.arity
             retained == SubSeq(stack, 1, Len(stack) - arity)
             children == SubSeq(stack, Len(stack) - arity + 1, Len(stack))
         IN IF Len(stack) < arity
            THEN <<>>
            ELSE RunCompiled(
                    Tail(programToRun),
                    Append(retained, Apply(instruction.label, children)))

CompiledMatch(tree) == Head(RunCompiled(Compile(tree), <<>>))

NoTree == Tree("NoTree", <<>>)
EncodedConnectiveTrees == SourceTrees

ParsedConnectiveCases ==
    {[evidence |-> "Parsed",
      parsed |-> tree,
      decoded |-> tree,
      downstream |-> ConnectiveInvalid]
       : tree \in EncodedConnectiveTrees}

UnprovedConnectiveCases ==
    {[evidence |-> "Unproved",
      parsed |-> NoTree,
      decoded |-> NoTree,
      downstream |-> downstream]
       : downstream \in ConnectiveRouteOutcomes}

MalformedConnectiveCase ==
    [evidence |-> "Malformed",
     parsed |-> NoTree,
     decoded |-> NoTree,
     downstream |-> ConnectiveInvalid]

EncodedConnectiveCases ==
    ParsedConnectiveCases
      \union UnprovedConnectiveCases
      \union {MalformedConnectiveCase}

ReferenceConnectiveRoute(case) ==
    CASE case.evidence = "Parsed" -> ToConnectiveOutcome(RecursiveMatch(case.decoded))
      [] case.evidence = "Unproved" -> case.downstream
      [] OTHER -> ConnectiveInvalid

ImplementationConnectiveRoute(case) ==
    CASE case.evidence = "Parsed" -> ToConnectiveOutcome(CompiledMatch(case.parsed))
      [] case.evidence = "Unproved" -> case.downstream
      [] OTHER -> ConnectiveInvalid

EncodedParserSoundOnCanonicalCorpus ==
    \A case \in ParsedConnectiveCases : case.parsed = case.decoded

EncodedConnectiveRoutingEquivalent ==
    \A case \in EncodedConnectiveCases :
       ImplementationConnectiveRoute(case) = ReferenceConnectiveRoute(case)

EncodedUnprovedDelegatesExactly ==
    \A case \in UnprovedConnectiveCases :
       ImplementationConnectiveRoute(case) = case.downstream

MalformedNeverEntersEncodedMachine ==
    ImplementationConnectiveRoute(MalformedConnectiveCase) = ConnectiveInvalid

SuccessfulDisjunctionCarriesNoBindings ==
    \A children \in ChildSequences(LeafTrees) :
       LET results == [index \in 1..Len(children) |-> RecursiveMatch(children[index])]
           disjunction == IsolatedFirstSuccess(results, 1)
       IN disjunction.ok => disjunction.bindings = {}

AndFailureRestoresWholeFoldSnapshot ==
    \A children \in ChildSequences(LeafTrees) :
       LET result == Apply("Conjunction",
                     [index \in 1..Len(children) |-> RecursiveMatch(children[index])])
       IN ~result.ok => result.bindings = {}

NegationAlwaysRestoresSnapshot ==
    \A child \in LeafTrees :
       Apply("Negation", <<RecursiveMatch(child)>>).bindings = {}

RECURSIVE CursorVisit(_, _)
CursorVisit(children, next) ==
    IF next > Len(children)
    THEN <<>>
    ELSE <<children[next]>> \o CursorVisit(children, next + 1)

EncodedCursorEnumeratesChildrenOnceInOrder ==
    \A children \in ChildSequences(LeafTrees) : CursorVisit(children, 1) = children

RECURSIVE LeakyFirstSuccess(_, _)
LeakyFirstSuccess(children, index) ==
    IF index > Len(children)
    THEN Fail
    ELSE IF children[index].ok
         THEN children[index]
         ELSE LeakyFirstSuccess(children, index + 1)

LeakySuccessfulOrMutationIsDetected ==
    LET binding == LeafResult("Bind0A")
    IN /\ IsolatedFirstSuccess(<<binding>>, 1) = Result(TRUE, {})
       /\ LeakyFirstSuccess(<<binding>>, 1) = binding
       /\ binding # Result(TRUE, {})

PerChildRestoredAnd(children) ==
    IF Len(children) = 0
    THEN Result(TRUE, {})
    ELSE IF AllSuccessful(children)
         THEN children[Len(children)]
         ELSE Fail

PerChildAndRestoreMutationIsDetected ==
    LET children == <<LeafResult("Bind0A"), LeafResult("Bind1A")>>
    IN Apply("Conjunction", children) # PerChildRestoredAnd(children)

NotBindingLeakMutation(child) ==
    IF child.ok
    THEN Result(FALSE, child.bindings)
    ELSE Result(TRUE, child.bindings)

NotBindingLeakMutationIsDetected ==
    LET binding == LeafResult("Bind0A")
    IN Apply("Negation", <<binding>>) # NotBindingLeakMutation(binding)

SkippedCursor(children) ==
    IF Len(children) < 2 THEN children ELSE SubSeq(children, 2, Len(children))
DuplicatedCursor(children) ==
    IF Len(children) = 0 THEN children ELSE <<children[1]>> \o children
RECURSIVE ReversedCursor(_)
ReversedCursor(children) ==
    IF Len(children) = 0
    THEN <<>>
    ELSE ReversedCursor(Tail(children)) \o <<Head(children)>>

CursorMutationsAreDetected ==
    LET witness == <<Tree("ExactOk", <<>>), Tree("ExactFail", <<>>)>>
    IN /\ CursorVisit(witness, 1) = witness
       /\ SkippedCursor(witness) # witness
       /\ DuplicatedCursor(witness) # witness
       /\ ReversedCursor(witness) # witness

ParserNoneAsRejectionMutation(case) ==
    IF case.evidence = "Unproved" THEN ConnectiveRejected
    ELSE ImplementationConnectiveRoute(case)

ParserNoneAsRejectionMutationIsDetected ==
    \E case \in UnprovedConnectiveCases :
       /\ case.downstream \in {ConnectiveAccepted({}), ConnectiveInvalid}
       /\ ParserNoneAsRejectionMutation(case) # ImplementationConnectiveRoute(case)

ShallowTagOnlyMalformedMutation(case) ==
    IF case.evidence = "Malformed" THEN ConnectiveAccepted({})
    ELSE ImplementationConnectiveRoute(case)

ShallowTagOnlyMalformedMutationIsDetected ==
    ShallowTagOnlyMalformedMutation(MalformedConnectiveCase)
      # ImplementationConnectiveRoute(MalformedConnectiveCase)

PredicateFamilies == {"Bool", "Int", "String", "Uri", "ByteArray"}
TargetFamilies == PredicateFamilies \union {"Compound"}
PredicatePayloads == BOOLEAN

PredicateRoute(target, requested, payload) ==
    IF target = requested THEN ConnectiveAccepted({}) ELSE ConnectiveRejected

PredicateFamilyMappingIsExactAndPayloadIndependent ==
    \A target \in TargetFamilies, requested \in PredicateFamilies,
       leftPayload \in PredicatePayloads, rightPayload \in PredicatePayloads :
       /\ (PredicateRoute(target, requested, leftPayload) = ConnectiveAccepted({})
             <=> target = requested)
       /\ PredicateRoute(target, requested, leftPayload)
            = PredicateRoute(target, requested, rightPayload)

PayloadSensitivePredicateMutation(target, requested, payload) ==
    IF payload /\ target = requested THEN ConnectiveAccepted({}) ELSE ConnectiveRejected

PredicatePayloadMutationIsDetected ==
    \E family \in PredicateFamilies :
       PredicateRoute(family, family, FALSE)
         # PayloadSensitivePredicateMutation(family, family, FALSE)

TerminalConnectiveRoute(kind) == ConnectiveRejected

VarRefAndAbsentReject ==
    /\ TerminalConnectiveRoute("VarRef") = ConnectiveRejected
    /\ TerminalConnectiveRoute("Absent") = ConnectiveRejected

AbsentAsEmptyAndMutationIsDetected ==
    ConnectiveRejected # ConnectiveAccepted({})

(******************************************************************************
Connective metadata and target-domain dispatch.  A singleton Connective
protobuf envelope is only an active pattern when the enclosing Par also
proves connective_used.  Inactive or unproved child summaries delegate to
concrete resident Par matching.  At a root, that metadata is necessary but not
sufficient for the direct encoded matcher: bypassing ParCount/sub_pars also
requires evidence that the direct and resident routes receive the same target
domain.  Candidate compilation remains a conservative filter and uses the
root summary gate independently.
******************************************************************************)

ConnectiveSummaryEvidence == {"Active", "Inactive", "Unproved"}
ChildShapes == {"SingletonConnective", "Ordinary"}

ChildMetadataDispatch(summary, shape, nested, resident) ==
    IF shape = "SingletonConnective" /\ summary = "Active"
    THEN nested
    ELSE resident

ActiveChildDispatchCases ==
    {[summary |-> "Active",
      shape |-> "SingletonConnective",
      nested |-> outcome,
      resident |-> outcome]
       : outcome \in ConnectiveRouteOutcomes}

DelegatedChildDispatchCases ==
    {[summary |-> summary,
      shape |-> shape,
      nested |-> nested,
      resident |-> resident]
       : summary \in {"Inactive", "Unproved"},
         shape \in ChildShapes,
         nested \in ConnectiveRouteOutcomes,
         resident \in ConnectiveRouteOutcomes}
    \union
    {[summary |-> "Active",
      shape |-> "Ordinary",
      nested |-> nested,
      resident |-> resident]
       : nested \in ConnectiveRouteOutcomes,
         resident \in ConnectiveRouteOutcomes}

ChildMetadataDispatchCases ==
    ActiveChildDispatchCases \union DelegatedChildDispatchCases

ChildMetadataDispatchPreservesResidentAuthority ==
    \A case \in ChildMetadataDispatchCases :
       ChildMetadataDispatch(case.summary, case.shape, case.nested, case.resident)
         = case.resident

FalseSummarySingletonDelegatesToConcreteMatch ==
    \A nested \in ConnectiveRouteOutcomes, resident \in ConnectiveRouteOutcomes :
       ChildMetadataDispatch("Inactive", "SingletonConnective", nested, resident)
         = resident

UnprovedSummarySingletonDelegatesToConcreteMatch ==
    \A nested \in ConnectiveRouteOutcomes, resident \in ConnectiveRouteOutcomes :
       ChildMetadataDispatch("Unproved", "SingletonConnective", nested, resident)
         = resident

SingletonOnlyChildDispatch(summary, shape, nested, resident) ==
    IF shape = "SingletonConnective" THEN nested ELSE resident

FalseSummaryChildDispatchMutationIsDetected ==
    LET nested == ConnectiveAccepted({})
        resident == ConnectiveRejected
    IN /\ ChildMetadataDispatch(
              "Inactive", "SingletonConnective", nested, resident) = resident
       /\ SingletonOnlyChildDispatch(
              "Inactive", "SingletonConnective", nested, resident) = nested
       /\ nested # resident

UnprovedSummaryChildDispatchMutationIsDetected ==
    LET nested == ConnectiveAccepted({})
        resident == ConnectiveInvalid
    IN /\ ChildMetadataDispatch(
              "Unproved", "SingletonConnective", nested, resident) = resident
       /\ SingletonOnlyChildDispatch(
              "Unproved", "SingletonConnective", nested, resident) = nested
       /\ nested # resident

RootMatchDispatch(connectiveUsed, directDomainSafe, encoded, resident) ==
    IF connectiveUsed /\ directDomainSafe THEN encoded ELSE resident

SafeActiveRootDispatchCases ==
    {[connectiveUsed |-> TRUE,
      directDomainSafe |-> TRUE,
      encoded |-> outcome,
      resident |-> outcome]
       : outcome \in ConnectiveRouteOutcomes}

DelegatedRootDispatchCases ==
    {[connectiveUsed |-> FALSE,
      directDomainSafe |-> directDomainSafe,
      encoded |-> encoded,
      resident |-> resident]
       : directDomainSafe \in BOOLEAN,
         encoded \in ConnectiveRouteOutcomes,
         resident \in ConnectiveRouteOutcomes}
    \union
    {[connectiveUsed |-> TRUE,
      directDomainSafe |-> FALSE,
      encoded |-> encoded,
      resident |-> resident]
       : encoded \in ConnectiveRouteOutcomes,
         resident \in ConnectiveRouteOutcomes}

RootDispatchCases == SafeActiveRootDispatchCases \union DelegatedRootDispatchCases

RootMetadataDispatchPreservesResidentAuthority ==
    \A case \in RootDispatchCases :
       RootMatchDispatch(
         case.connectiveUsed, case.directDomainSafe, case.encoded, case.resident)
         = case.resident

FalseSummaryRootDelegatesToConcreteMatch ==
    \A directDomainSafe \in BOOLEAN,
       encoded \in ConnectiveRouteOutcomes, resident \in ConnectiveRouteOutcomes :
       RootMatchDispatch(FALSE, directDomainSafe, encoded, resident) = resident

UnsafeActiveRootDelegatesToCandidateSelectingMatch ==
    \A encoded \in ConnectiveRouteOutcomes, resident \in ConnectiveRouteOutcomes :
       RootMatchDispatch(TRUE, FALSE, encoded, resident) = resident

SafeActiveRootEntersEncodedMatch ==
    \A encoded \in ConnectiveRouteOutcomes :
       RootMatchDispatch(TRUE, TRUE, encoded, encoded) = encoded

RootCandidateDecision(connectiveUsed) ==
    IF connectiveUsed THEN "CompileConnective" ELSE "DelegateConcrete"

FalseSummaryRootDoesNotCompileConnectiveCandidate ==
    RootCandidateDecision(FALSE) = "DelegateConcrete"

RootCandidateMetadataMutation(connectiveUsed) == "CompileConnective"

FalseSummaryRootCandidateMutationIsDetected ==
    /\ RootCandidateDecision(FALSE) = "DelegateConcrete"
    /\ RootCandidateMetadataMutation(FALSE) = "CompileConnective"

SummaryOnlyRootMatchMutation(connectiveUsed, encoded, resident) ==
    IF connectiveUsed THEN encoded ELSE resident

FalseSummaryRootMatchMutationIsDetected ==
    LET encoded == ConnectiveAccepted({})
        resident == ConnectiveRejected
    IN /\ RootMatchDispatch(FALSE, TRUE, encoded, resident) = resident
       /\ SummaryOnlyRootMatchMutation(FALSE, encoded, resident) = resident
       /\ encoded # resident

UnsafeActiveRootMatchMutationIsDetected ==
    LET encoded == ConnectiveAccepted({})
        resident == ConnectiveRejected
    IN /\ RootMatchDispatch(TRUE, FALSE, encoded, resident) = resident
       /\ SummaryOnlyRootMatchMutation(TRUE, encoded, resident) = encoded
       /\ encoded # resident

(******************************************************************************
Exact scalar equality.  The finite decoded identity is deliberately the
canonical scalar byte identity itself: injectivity makes both equal and
unequal admitted bytes conclusive, while NoScalar is lack of proof and must
delegate without inspecting a shallow tag.
******************************************************************************)

CanonicalScalars == {
    "Bool0", "Bool1", "Int0", "Int1", "String0", "String1",
    "Uri0", "Uri1", "Bytes0", "Bytes1", "Double0", "Double1",
    "BigInt0", "BigInt1", "BigRat0", "BigRat1", "Fixed0", "Fixed1"
}
NoScalar == "NoScalar"
ScalarEvidence == CanonicalScalars \union {NoScalar}
DecodedScalarIdentity(scalar) == scalar

ExactScalarRoute(target, pattern, downstream) ==
    IF target \in CanonicalScalars /\ pattern \in CanonicalScalars
    THEN IF target = pattern THEN "Accepted" ELSE "Rejected"
    ELSE downstream

DecodedScalarAuthority(target, pattern, downstream) ==
    IF target \in CanonicalScalars /\ pattern \in CanonicalScalars
    THEN IF DecodedScalarIdentity(target) = DecodedScalarIdentity(pattern)
         THEN "Accepted"
         ELSE "Rejected"
    ELSE downstream

CanonicalScalarByteEqualityIsDecodedEquality ==
    \A target \in CanonicalScalars, pattern \in CanonicalScalars :
       (target = pattern)
         <=> (DecodedScalarIdentity(target) = DecodedScalarIdentity(pattern))

ExactScalarRoutePreservesAuthority ==
    \A target \in ScalarEvidence, pattern \in ScalarEvidence,
       downstream \in RouteOutcomes :
       ExactScalarRoute(target, pattern, downstream)
         = DecodedScalarAuthority(target, pattern, downstream)

UnprovedScalarBytesDelegateExactly ==
    \A target \in ScalarEvidence, pattern \in ScalarEvidence,
       downstream \in RouteOutcomes :
       (target = NoScalar \/ pattern = NoScalar)
         => ExactScalarRoute(target, pattern, downstream) = downstream

UnequalScalarDelegationMutation(target, pattern, downstream) ==
    IF target \in CanonicalScalars /\ pattern \in CanonicalScalars
    THEN IF target = pattern THEN "Accepted" ELSE downstream
    ELSE downstream

ExactScalarMismatchMutationIsDetected ==
    /\ ExactScalarRoute("Int0", "Int1", "Accepted") = "Rejected"
    /\ UnequalScalarDelegationMutation("Int0", "Int1", "Accepted") = "Accepted"

UnprovedScalarRejectionMutation(target, pattern, downstream) ==
    IF target = NoScalar \/ pattern = NoScalar
    THEN "Rejected"
    ELSE ExactScalarRoute(target, pattern, downstream)

UnprovedScalarRejectionMutationIsDetected ==
    /\ ExactScalarRoute(NoScalar, "Int0", "Invalid") = "Invalid"
    /\ UnprovedScalarRejectionMutation(NoScalar, "Int0", "Invalid") = "Rejected"

Spec == Init /\ [][Next]_vars

=============================================================================

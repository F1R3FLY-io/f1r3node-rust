---------------------- MODULE SpatialMatcherEquivalence ----------------------
EXTENDS Integers, Naturals, Sequences, FiniteSets, TLC

(******************************************************************************
Finite operational cross-check for the stateful matcher algebra mechanized in
Rocq's SpatialMatcher.v.  The source semantics is recursive.  The implementation
model is a post-order PDA with explicit program counter and value stack.

The corpus includes success/refusal, two values for the same free-variable
level (so conflicting bindings are reachable), sequencing/conjunction,
ordered retrying disjunction, and negation.  Bindings produced by a failed
branch never enter the result: disjunction selects the first successful child
and negation always returns either failure or the original empty delta.
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

RECURSIVE FirstSuccess(_, _)
FirstSuccess(children, index) ==
    IF index > Len(children)
    THEN Fail
    ELSE IF children[index].ok
         THEN children[index]
         ELSE FirstSuccess(children, index + 1)

Apply(label, children) ==
    IF label \in LeafLabels
    THEN LeafResult(label)
    ELSE IF label \in {"Sequence", "Conjunction"}
         THEN LET bindings == UnionBindings(children)
              IN IF AllSuccessful(children) /\ ~HasConflict(bindings)
                 THEN Result(TRUE, bindings)
                 ELSE Fail
         ELSE IF label = "Disjunction"
              THEN FirstSuccess(children, 1)
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
SourceTrees == LeafTrees \union DepthOneTrees

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

Spec == Init /\ [][Next]_vars

=============================================================================

---------------------------- MODULE PDAEquivalence ----------------------------
EXTENDS Integers, Naturals, Sequences, TLC

(******************************************************************************
This finite model independently checks the operational invariant used by the
generated Rust pushdown automata.  Each source tree is compiled to post-order
Reduce instructions.  The machine consumes child results from an explicit
value stack and must finish with exactly the recursive fold of the source.

The configured universe contains every ordered binary tree over two labels to
depth two (422 roots).  Rocq proves the theorem for arbitrary finite trees;
TLC exhaustively checks the concrete transition relation and catches mistakes
in stack orientation, arity handling, program-counter movement, or completion.
******************************************************************************)

Labels == {0, 1}

LeafTrees == {[label |-> label, children |-> <<>>] : label \in Labels}

ChildSequences(elements) ==
    {<<>>}
    \union {<<first>> : first \in elements}
    \union {<<first, second>> : first \in elements, second \in elements}

TreesFrom(elements) ==
    {[label |-> label, children |-> children]
        : label \in Labels, children \in ChildSequences(elements)}

DepthOneTrees == TreesFrom(LeafTrees)
DepthTwoTrees == TreesFrom(DepthOneTrees)
SourceTrees == LeafTrees \union DepthOneTrees \union DepthTwoTrees

RECURSIVE Compile(_), RecursiveFold(_), CompileChildren(_, _)

CompileChildren(children, index) ==
    IF index > Len(children)
    THEN <<>>
    ELSE Compile(children[index]) \o CompileChildren(children, index + 1)

Compile(tree) ==
    CompileChildren(tree.children, 1)
      \o <<[label |-> tree.label, arity |-> Len(tree.children)]>>

RecursiveFold(tree) ==
    [label |-> tree.label,
     children |-> [index \in 1..Len(tree.children) |->
                      RecursiveFold(tree.children[index])]]

VARIABLES source, program, expected, pc, values
vars == <<source, program, expected, pc, values>>

Init ==
    /\ source \in SourceTrees
    /\ program = Compile(source)
    /\ expected = RecursiveFold(source)
    /\ pc = 1
    /\ values = <<>>

ReduceStep ==
    /\ pc \leq Len(program)
    /\ LET instruction == program[pc]
           arity == instruction.arity
           retained == SubSeq(values, 1, Len(values) - arity)
           children == SubSeq(values, Len(values) - arity + 1, Len(values))
           result == [label |-> instruction.label, children |-> children]
       IN /\ Len(values) \geq arity
          /\ values' = Append(retained, result)
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
    /\ expected = RecursiveFold(source)
    /\ pc \in 1..(Len(program) + 1)
    /\ values \in Seq({RecursiveFold(tree) : tree \in SourceTrees})

NoInstructionUnderflow ==
    pc \leq Len(program) => Len(values) \geq program[pc].arity

CompletedEquivalent ==
    Done => values = <<expected>>

Spec == Init /\ [][Next]_vars

=============================================================================

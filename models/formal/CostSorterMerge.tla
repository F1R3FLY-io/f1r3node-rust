------------------------ MODULE CostSorterMerge ------------------------
EXTENDS Integers, Sequences, FiniteSets

\* A finite witness for the new mutually recursive cost-sorter cut. Each
\* sequence is in source visit order, not protobuf field order. Compound
\* children are already flattened, as descend_cost_signature does before
\* scheduling them. In Rust, each item is a borrowed node/continuation.
\* Scope exception: target F1R3Lang EPathMap/EZipper use a compressed trie,
\* not the source Vec<Par>. Their deterministic key-decode/re-file meter
\* tariff and canonical trie order are checked by Rust differential/trace
\* tests; this finite cut-order witness does not claim charge equivalence for
\* those target-only storage modes.
Root == "par"
Kids(id) ==
    CASE id = "par" -> <<"receive", "signed", "stack">>
      [] id = "receive" -> <<"bind", "body", "condition">>
      [] id = "bind" -> <<"pattern", "source", "bind-sig">>
      [] id = "bind-sig" -> <<"quote-par">>
      [] id = "quote-par" -> <<"quote-stack">>
      [] id = "quote-stack" -> <<"quote-inner">>
      [] id = "quote-inner" -> <<"quote-leaf-par">>
      [] id = "signed" -> <<"signed-body", "signed-sig">>
      [] id = "signed-sig" -> <<"name-par">>
      [] id = "stack" -> <<"compound", "stack-leaf">>
      [] id = "compound" -> <<"unit", "bound">>
      [] OTHER -> <<>>

RECURSIVE Ref(_), RefKids(_)
Ref(id) == RefKids(Kids(id)) \o <<id>>
RefKids(kids) ==
    IF Len(kids) = 0 THEN <<>>
    ELSE Ref(Head(kids)) \o RefKids(Tail(kids))
Expected == Ref(Root)

Desc(id, ix) == [kind |-> "D", node |-> id, index |-> ix]
Comb(id, ix) == [kind |-> "C", node |-> id, index |-> ix]
Value(id, ix) == [node |-> id, index |-> ix]
\* Work is LIFO at the right. Reverse source order when pushing children.
ReverseDescents(id) ==
    [i \in 1..Len(Kids(id)) |->
        Desc(Kids(id)[Len(Kids(id)) - i + 1], Len(Kids(id)) - i)]

RECURSIVE WorkArity(_)
WorkArity(w) ==
    IF Len(w) = 0 THEN 0
    ELSE (IF Head(w).kind = "C" THEN Len(Kids(Head(w).node)) ELSE 0)
         + WorkArity(Tail(w))
Pending(w, kind) == Cardinality({i \in 1..Len(w) : w[i].kind = kind})
Top(w) == w[Len(w)]
WithoutTop(w) == SubSeq(w, 1, Len(w) - 1)

VARIABLES work, vals, seen, status
vars == <<work, vals, seen, status>>
Init ==
    /\ work = <<Desc(Root, 0)>>
    /\ vals = <<>>
    /\ seen = <<>>
    /\ status = "running"
Descend ==
    /\ status = "running" /\ Len(work) > 0 /\ Top(work).kind = "D"
    /\ work' = WithoutTop(work) \o <<Comb(Top(work).node, Top(work).index)>>
               \o ReverseDescents(Top(work).node)
    /\ UNCHANGED <<vals, seen, status>>
Ready ==
    LET n == Len(Kids(Top(work).node))
    IN /\ Len(vals) >= n
       /\ \A i \in 1..n : vals[Len(vals) - n + i].index = i - 1
Combine ==
    /\ status = "running" /\ Len(work) > 0 /\ Top(work).kind = "C"
    /\ Ready
    /\ LET n == Len(Kids(Top(work).node))
           item == Top(work)
       IN /\ vals' = SubSeq(vals, 1, Len(vals) - n)
                       \o <<Value(item.node, item.index)>>
          /\ seen' = Append(seen, item.node)
    /\ work' = WithoutTop(work)
    /\ UNCHANGED status
Finish ==
    /\ status = "running" /\ Len(work) = 0
    /\ status' = "finished"
    /\ UNCHANGED <<work, vals, seen>>
Next == Descend \/ Combine \/ Finish
Spec == Init /\ [][Next]_vars

\* Exactly the shared drive's deficit invariant, plus the source-index and
\* postorder facts whose violation can change signed canonical bytes.
Inv_Deficit ==
    status = "running" =>
        Len(vals) + Pending(work, "D") + Pending(work, "C")
        = 1 + WorkArity(work)
Inv_Ready ==
    status = "running" /\ Len(work) > 0 /\ Top(work).kind = "C"
        => Ready
Inv_PostorderPrefix == seen = SubSeq(Expected, 1, Len(seen))
Inv_Final ==
    status = "finished" =>
        /\ vals = <<Value(Root, 0)>>
        /\ seen = Expected
=========================================================================

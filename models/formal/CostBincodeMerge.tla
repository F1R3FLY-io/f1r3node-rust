------------------------ MODULE CostBincodeMerge ------------------------
EXTENDS Naturals, Sequences

Roots == <<"par", "float", "authority-id", "principal-id", "continuation", "list">>

Kind(id) ==
    CASE id = "par" \/ id = "subpar" -> "Par"
      [] id = "receive" -> "Receive"
      [] id = "bind" -> "Bind"
      [] id = "signed" -> "Signed"
      [] id = "stack" -> "Stack"
      [] id = "quote" -> "Quote"
      [] id = "compound-sig" -> "CompoundSig"
      [] id = "compound" -> "Compound"
      [] id = "ground" -> "Ground"
      [] id = "continuation" -> "Continuation"
      [] id = "list" -> "List"
      [] id = "authority" -> "Authority"
      [] id = "region" -> "Region"
      [] id = "float" -> "Float"
      [] id = "authority-id" -> "AuthorityId"
      [] id = "principal-id" -> "PrincipalId"

Fields(kind) ==
    CASE kind = "Par" ->
             <<"sends", "receives", "news", "exprs", "matches",
               "unforgeables", "bundles", "connectives", "conditionals",
               "locally_free", "connective_used", "cost_signed_terms",
               "cost_stacks">>
      [] kind = "Receive" ->
             <<"binds", "body", "persistent", "peek", "bind_count",
               "locally_free", "connective_used", "condition">>
      [] kind = "Bind" ->
             <<"patterns", "source", "remainder", "free_count",
               "cost_signature">>
      [] kind = "Continuation" ->
             <<"guard", "cost_authority", "tagged_cont">>
      [] kind = "List" ->
             <<"pars", "random_state", "cost_authority", "cost_stack">>
      [] kind = "Signed" -> <<"body", "signature">>
      [] kind = "Stack" -> <<"cells">>
      [] kind = "Authority" -> <<"regions">>
      [] kind = "Region" -> <<"instance_id", "signature">>
      [] kind = "Compound" -> <<"elements">>
      [] kind = "Ground" -> <<"option_tag", "variant_index", "ground">>
      [] kind = "Quote" -> <<"option_tag", "variant_index", "quote">>
      [] kind = "CompoundSig" -> <<"option_tag", "variant_index", "compound">>
      [] kind = "Float" -> <<"option_tag", "variant_index", "bits">>
      [] kind = "AuthorityId" -> <<"option_tag", "variant_index", "id">>
      [] kind = "PrincipalId" ->
             <<"option_tag", "variant_index", "key_family", "public_key">>

Kids(id, field) ==
    CASE id = "par" /\ field = "receives" -> <<"receive">>
      [] id = "par" /\ field = "cost_signed_terms" -> <<"signed">>
      [] id = "par" /\ field = "cost_stacks" -> <<"stack">>
      [] id = "receive" /\ field = "binds" -> <<"bind">>
      [] id = "bind" /\ field = "cost_signature" -> <<"ground">>
      [] id = "signed" /\ field = "body" -> <<"subpar">>
      [] id = "signed" /\ field = "signature" -> <<"quote">>
      [] id = "quote" /\ field = "quote" -> <<"subpar">>
      [] id = "stack" /\ field = "cells" -> <<"compound-sig">>
      [] id = "compound-sig" /\ field = "compound" -> <<"compound">>
      [] id = "compound" /\ field = "elements" -> <<"ground", "ground">>
      [] id = "continuation" /\ field = "cost_authority" -> <<"authority">>
      [] id = "list" /\ field = "cost_authority" -> <<"authority">>
      [] id = "list" /\ field = "cost_stack" -> <<"stack">>
      [] id = "authority" /\ field = "regions" -> <<"region">>
      [] id = "region" /\ field = "signature" -> <<"ground">>
      [] OTHER -> <<>>

Task(id, field) == [node |-> id, field |-> field]
FieldTasks(id) ==
    [i \in 1..Len(Fields(Kind(id))) |-> Task(id, Fields(Kind(id))[i])]

RECURSIVE ChildTasks(_), RefNode(_), RefFields(_, _), RefChildren(_)
ChildTasks(ids) ==
    IF Len(ids) = 0 THEN <<>>
    ELSE FieldTasks(Head(ids)) \o ChildTasks(Tail(ids))
RefNode(id) == RefFields(id, 1)
RefFields(id, index) ==
    IF index > Len(Fields(Kind(id))) THEN <<>>
    ELSE LET field == Fields(Kind(id))[index]
         IN <<Task(id, field)>> \o RefChildren(Kids(id, field)) \o
            RefFields(id, index + 1)
RefChildren(ids) ==
    IF Len(ids) = 0 THEN <<>>
    ELSE RefNode(Head(ids)) \o RefChildren(Tail(ids))

Expected == RefChildren(Roots)
Input(cut) == SubSeq(Expected, 1, cut)

VARIABLES todo, cursor, seen, status, cut
vars == <<todo, cursor, seen, status, cut>>

Init ==
    /\ cut \in 0..Len(Expected)
    /\ todo = ChildTasks(Roots)
    /\ cursor = 1
    /\ seen = <<>>
    /\ status = "running"
Read ==
    /\ status = "running"
    /\ Len(todo) > 0
    /\ cursor <= Len(Input(cut))
    /\ Head(todo) = Input(cut)[cursor]
    /\ seen' = Append(seen, Head(todo))
    /\ todo' = ChildTasks(Kids(Head(todo).node, Head(todo).field)) \o Tail(todo)
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<status, cut>>
Reject ==
    /\ status = "running"
    /\ Len(todo) > 0
    /\ IF cursor > Len(Input(cut)) THEN TRUE
       ELSE Head(todo) # Input(cut)[cursor]
    /\ status' = "rejected"
    /\ UNCHANGED <<todo, cursor, seen, cut>>
Accept ==
    /\ status = "running"
    /\ Len(todo) = 0
    /\ status' = "accepted"
    /\ UNCHANGED <<todo, cursor, seen, cut>>
Next == Read \/ Reject \/ Accept
Spec == Init /\ [][Next]_vars

Inv_Prefix == seen = SubSeq(Expected, 1, Len(seen))
Inv_NoDefault == status = "accepted" => seen = Expected
Inv_TruncationRejects == status = "accepted" => cut = Len(Expected)
Inv_AllFieldsConsumed == status = "accepted" => cursor = Len(Expected) + 1
==========================================================================

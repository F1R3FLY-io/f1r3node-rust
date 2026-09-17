-------------------- MODULE FeeCursorTransport --------------------
EXTENDS Naturals, Sequences

CONSTANT WrapCursor
VARIABLES cursor, fee, stage, transport, delivered, charged

vars == <<cursor, fee, stage, transport, delivered, charged>>

Init == /\ cursor \in {"Nil", "ValidCursor"}
        /\ fee \in 0..1
        /\ stage = "encode"
        /\ transport = <<>>
        /\ delivered = "none"
        /\ charged = 0

Encode == /\ stage = "encode"
          /\ transport' = IF WrapCursor THEN <<cursor>> ELSE cursor
          /\ stage' = "inject"
          /\ UNCHANGED <<cursor, fee, delivered, charged>>

Inject == /\ stage = "inject"
          /\ stage' = IF transport = "Nil" THEN "injection-error" ELSE "deliver"
          /\ UNCHANGED <<cursor, fee, transport, delivered, charged>>

Deliver == /\ stage = "deliver"
           /\ delivered' = IF WrapCursor THEN transport[1] ELSE transport
           /\ stage' = "guard"
           /\ UNCHANGED <<cursor, fee, transport, charged>>

Guard == /\ stage = "guard"
         /\ stage' = IF delivered = "Nil" /\ fee > 0 THEN "rejected" ELSE "settled"
         /\ charged' = IF delivered = "Nil" /\ fee > 0 THEN 0 ELSE fee
         /\ UNCHANGED <<cursor, fee, transport, delivered>>

Next == Encode \/ Inject \/ Deliver \/ Guard
Spec == Init /\ [][Next]_vars

ValidInputCanReachVault == stage # "injection-error"
TransportPreservesCursor == stage \in {"guard", "rejected", "settled"} => delivered = cursor
MissingCursorNeverPaysFee == cursor = "Nil" => charged = 0
ZeroFeeIsNotRejected == fee = 0 => stage # "rejected"
GuardPreservesFee == stage = "settled" => charged = fee

=====================================================================

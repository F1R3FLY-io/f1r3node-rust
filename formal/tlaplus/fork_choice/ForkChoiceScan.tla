----------------------------- MODULE ForkChoiceScan -----------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxTip,
    Cap,
    NodeLocalTop

VARIABLES
    certified,
    topA,
    topB

vars == <<certified, topA, topB>>

LegacyDepthProjection(top, messages) ==
    { message \in messages : message + Cap > top }

Projection(top, messages) ==
    IF NodeLocalTop = 0
    THEN messages
    ELSE LegacyDepthProjection(top, messages)

Init ==
    /\ certified = {}
    /\ topA = 0
    /\ topB = 0

Step ==
    \E messages \in SUBSET (1..MaxTip) :
      \E receiverTopA \in 0..MaxTip :
        \E receiverTopB \in 0..MaxTip :
          /\ certified' = messages
          /\ topA' = receiverTopA
          /\ topB' = receiverTopB

Next == Step

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ certified \subseteq (1..MaxTip)
    /\ topA \in 0..MaxTip
    /\ topB \in 0..MaxTip

Inv_LcaDeterministic ==
    Projection(topA, certified) = Projection(topB, certified)

Inv_AllCertifiedMessagesRetained ==
    /\ Projection(topA, certified) = certified
    /\ Projection(topB, certified) = certified

Inv_ReceiverTopIrrelevant ==
    \A arbitraryTop \in 0..MaxTip :
      /\ Projection(topA, certified) = Projection(arbitraryTop, certified)
      /\ Projection(topB, certified) = Projection(arbitraryTop, certified)

===============================================================================

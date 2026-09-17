-------------------- MODULE ProtocolActivationCoherence --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    UseActiveProtocolForBase,
    RejectMixedScope,
    ValidateVersionEncoding

LegacyProtocol == 1
ExactProtocol == 2
StateEffectProtocol == 3
ActiveProtocol == StateEffectProtocol
FloorProtocol == LegacyProtocol

BaseSource == "base-source"
DuplicateBlock == "duplicate-block"
FreshBlock == "fresh-block"
LegacyBlock == "legacy-block"
MalformedCurrentBlock == "malformed-current-block"

Blocks == {DuplicateBlock, FreshBlock, LegacyBlock, MalformedCurrentBlock}
Sources == {BaseSource} \union Blocks
Signatures == {"A", "B", "C", "D"}

BlockProtocol(block) ==
    IF block = LegacyBlock THEN LegacyProtocol ELSE ActiveProtocol

HasProvenance(block) == block \in {DuplicateBlock, FreshBlock}
HasSpecifiedReason(block) == block \in {DuplicateBlock, FreshBlock}

EncodingMatches(block) ==
    IF BlockProtocol(block) >= ExactProtocol
    THEN HasProvenance(block) /\ HasSpecifiedReason(block)
    ELSE ~HasProvenance(block) /\ ~HasSpecifiedReason(block)

Signature(source) ==
    CASE source = BaseSource -> "A"
      [] source = DuplicateBlock -> "A"
      [] source = FreshBlock -> "B"
      [] source = LegacyBlock -> "C"
      [] source = MalformedCurrentBlock -> "D"

ScopeProtocolAdmissible(block) ==
    IF RejectMixedScope
    THEN BlockProtocol(block) = ActiveProtocol
    ELSE TRUE

ScopeEncodingAdmissible(block) ==
    IF ValidateVersionEncoding
    THEN EncodingMatches(block)
    ELSE TRUE

AdmissibleBlocks ==
    {block \in Blocks :
        ScopeProtocolAdmissible(block) /\ ScopeEncodingAdmissible(block)}

BaseReceiptVisible ==
    IF UseActiveProtocolForBase
    THEN ActiveProtocol >= ExactProtocol
    ELSE FloorProtocol >= ExactProtocol

BaseSignatures == IF BaseReceiptVisible THEN {"A"} ELSE {}

AcceptedBlocks ==
    {block \in AdmissibleBlocks : Signature(block) \notin BaseSignatures}

VARIABLES phase, selectedBlocks, materializedSources, rejectedBlocks

vars == <<phase, selectedBlocks, materializedSources, rejectedBlocks>>

Init ==
    /\ phase = "ready"
    /\ selectedBlocks = {}
    /\ materializedSources = {BaseSource}
    /\ rejectedBlocks = {}

Merge ==
    /\ phase = "ready"
    /\ phase' = "merged"
    /\ selectedBlocks' = AcceptedBlocks
    /\ materializedSources' = {BaseSource} \union AcceptedBlocks
    /\ rejectedBlocks' = Blocks \ AcceptedBlocks

Next == Merge

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ready", "merged"}
    /\ selectedBlocks \subseteq Blocks
    /\ materializedSources \subseteq Sources
    /\ rejectedBlocks \subseteq Blocks

Inv_AtMostOneEffectPerSignature ==
    \A signature \in Signatures :
        Cardinality(
            {source \in materializedSources : Signature(source) = signature}
        ) <= 1

Inv_ActiveScopeVersionHomogeneous ==
    \A block \in selectedBlocks : BlockProtocol(block) = ActiveProtocol

Inv_EncodingMatchesVersion ==
    \A block \in selectedBlocks : EncodingMatches(block)

Inv_LegacyFloorExactActivationComposes ==
    phase = "merged" =>
        /\ BaseSource \in materializedSources
        /\ DuplicateBlock \notin selectedBlocks
        /\ FreshBlock \in selectedBlocks
        /\ LegacyBlock \notin selectedBlocks
        /\ MalformedCurrentBlock \notin selectedBlocks
=============================================================================

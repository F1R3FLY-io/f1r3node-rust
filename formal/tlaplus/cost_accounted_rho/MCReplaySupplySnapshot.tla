-------------------------- MODULE MCReplaySupplySnapshot --------------------------
EXTENDS ReplaySupplySnapshot

\* @type: Seq(Int);
CostsDef == <<3, 3, 3>>
\* @type: Seq(Int);
RecordedEventsDef == <<101, 102>>
\* @type: Seq(Int);
RootsDef == <<1001, 1002, 1003>>
\* @type: Str;
CertifiedProposerDef == "validator-A"

=============================================================================

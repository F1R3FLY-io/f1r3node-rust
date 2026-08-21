-------------------------- MODULE MCMergeAggregateAgreement --------------------------
EXTENDS MergeAggregateAgreement

ContributionOrdersDef ==
  {<<10, -1>>, <<-1, 10>>, <<4, -5, 1>>, <<1, -5, 4>>}

UnsafeContributionOrdersDef == {<<10, -1>>, <<-1, 10>>}

=============================================================================

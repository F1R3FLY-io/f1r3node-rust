# Economic and activation policy ratification

## Approved decisions

The selected release mode is new-economy fresh-genesis activation.
Imported-state activation and in-place upgrades are outside this release.
Required legacy and activated replay remain release requirements.

The [economic failure policy](economic-failure-policy-decisions.md) defines charge, refund, prepaid, exposure, and conversion behavior.
The [activation policy](activation-migration-policy-decisions.md) defines compatibility, historical replay, and recovery requirements.

## Economic boundaries

Preserve compatible prepaid acquisition terms and captured refund destinations.
Distinguish maximum retained charges from separately authorized temporary exposure.
Retain only authorized billable work and the separate fee for classified user failures.
Platform, certificate, and unclassified failures prevent publication of candidate charges, including mixed-error results.

Support separately committed conversion and atomic quote-backed funding as distinct signed compositions.
Atomic funding releases unused input in its original asset and custody.
Preserve captured quote terms, provider capacity, and approved allocation fairness.
Do not introduce persistent deployment escrow or independently spendable intermediate balances.

## Activation boundaries

Bind new consent to exact schedules and explicit compatibility entries.
Preserve requested historical execution rules and their accepted effects.
Authenticate receipts through existing accepted block commitments, with a verifiable receipt binding.
Do not introduce a new receipt-signing authority or Casper activation mechanism.

The excluded migration modes remain documented requirements for a separately approved release.
These policies introduce no new Casper activation mechanism.

## Evidence and remaining work

Policy ratification does not establish runtime implementation or formal correctness.
The planned contracts, proofs, property tests, concurrency tests, native integration, and release gates remain required.

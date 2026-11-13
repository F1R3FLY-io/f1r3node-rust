# Justification history

This area checks the [justification history claim](../../../docs/claims/justification-history.md).

The property covers any arrival order. It is unbounded by specification. The model instance uses two validator keys, sequence numbers 0 through 2, Boolean witness validity and availability, and two receiver views. Key 0 also represents the candidate sender. It checks both orders of receiver advancement and candidate validation. These are model-checking bounds, not protocol bounds.

The predecessor view represents the block named by the candidate's signed creator justification. A receiver advance changes its local latest-message view. Each comparison retains the invalid-witness exemption. Missing evidence defers the comparison. An earlier demonstrated regression can reject before a later missing witness is examined.

Run from this directory with `java` and `tla2tools.jar` available:

```sh
java -cp "$TLA_TOOLS_JAR" tlc2.TLC -workers 1 -config MC_JustificationHistory.cfg JustificationHistory.tla
java -cp "$TLA_TOOLS_JAR" tlc2.TLC -workers 1 -config MC_JustificationHistory_local_latest_unsafe.cfg JustificationHistory.tla
```

The clean configuration must exit 0. The unsafe configuration must exit 12 on `HistoryRelative`. The unsafe switch uses the receiver's advanced view as the comparison baseline. It reproduces a rejection of an otherwise valid historical candidate with complete witnesses.

The construction tier is [BugFixSelfRegression.v](../../rocq/slashing/theories/BugFixSelfRegression.v). Its added theorems prove selection independence from receiver latest messages, signed-predecessor selection, exact detection of a valid regressive witness, and the invalid-witness exemption. They quantify over unbounded natural numbers and lists. The existing slashing project imports this module into `MainTheorem`. Build that project and run `coqchk` on `Slashing.MainTheorem`.

The Rust binding tests are `justification_regressions_use_the_signed_predecessor_during_catch_up` and `justification_regressions_defer_when_predecessor_evidence_is_missing` in [validate_test.rs](../../../casper/tests/batch2/validate_test.rs). The first checks a historical candidate before and after later messages arrive while its body is absent. It also checks a real regression. The second checks missing predecessor evidence and rejection after that evidence arrives.

Trusted assumptions include immutable hash-addressed block metadata, correct creator-justification lookup, and the validation caller's earlier structural and sequence checks. The model abstracts hash identity, signatures, storage failures, and replay. The Rust tests check examples of the binding. They do not prove it for all DAGs. Maintainer review of the scope and binding is pending. No bounded-by-design classification, waiver, or signed acceptance is claimed.

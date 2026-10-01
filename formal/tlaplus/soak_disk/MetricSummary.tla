---- MODULE MetricSummary ----
EXTENDS Naturals, FiniteSets

CONSTANT CompleteFormatter
VARIABLES phase, collected, published

vars == <<phase, collected, published>>
Required == 1..27
LegacyFormatter == 1..7

Init == /\ phase = "scraping"
        /\ collected = {}
        /\ published = {}

Scrape == /\ phase = "scraping"
          /\ collected' = Required
          /\ phase' = "formatting"
          /\ UNCHANGED published

Publish == /\ phase = "formatting"
           /\ published' = collected \cap
                (IF CompleteFormatter THEN Required ELSE LegacyFormatter)
           /\ phase' = "retained"
           /\ UNCHANGED collected

Next == Scrape \/ Publish
Spec == Init /\ [][Next]_vars

TypeOK == /\ phase \in {"scraping", "formatting", "retained"}
          /\ collected \subseteq Required
          /\ published \subseteq Required

CollectedRequiredPublished == phase = "retained" => collected \subseteq published
====

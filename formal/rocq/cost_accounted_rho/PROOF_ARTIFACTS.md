# Cost-Accounted Rho Proof Build Artifacts

Rocq proof certificates are build products for the cost-accounted rho formal model and are intentionally not tracked. The verification script regenerates them from source before checking headline theorem closure.

- Source commit used for the migration: `d9dd1b8c335e73b386b6b56768587d8e384f6403`
- Rocq version used during migration: `9.1.0`
- Logical namespace: `-Q theories CostAccountedRho`
- Proof entry point: `CostAccountedRho.UseCaseAdequacy`
- λ "R1-only" cost instance (DR-25): `CostAccountedRho.CAUntypedLambda` (core: R1-only, funded run-bound, unconditional funded SN including the Ω halting seam, erasure to pure β) and `CostAccountedRho.CAUntypedLambdaCI` (`Lambda_ciGSLT`, a second object under `CostCI`); 16 headline theorems queried for axiom-free closure by the verification script

Run `scripts/check-cost-accounted-rho-proofs.sh` from the repository root to rebuild the local certificates, run `rocqchk`, and query the assumptions of the headline theorems. Local `*.vo`, `*.vos`, `*.vok`, `.aux`, `.glob`, dependency, and cache files remain ignored.

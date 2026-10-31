# A-6: authority audit of patch/P2-integration against P10 and P9

- **Branch audited:** `patch/P2-integration` at `3b5a7e90a`.
- **Design:** stall-loop design revision 8, §4.6, §4.7, §6.1 (P9, P10) and §6.9. Paper: `cost-accounting-as-monad/continued-gslt-cost-v2.tex`, §§3–6 and §8 (capabilities).
- **Auditor:** lane A1. Lane A1 wrote none of the native REV code.
- **Scope:** findings only. This document contains no fixes. Each finding becomes a fix or a decision after review.

## Method and limits

The audit is a code reading of the native REV layer (`rholang/src/rust/interpreter/accounting/rev_stack/*`), its system-process entry points (`system_processes.rs`), the reducer (`reduce.rs`: `eval_new`, `check_region_signature`, `eval_cost_stack`, `eval_kind_stacks`), the normalizer environment (`models/src/rust/normalizer_env.rs`), cosignatures (`casper_message.rs`), the gate (`casper/src/rust/v6/gate.rs`, `executor.rs`) and settlement (`casper/src/rust/v6/settle.rs`), plus the Rholang callers `PoSV6.rhox` and `SystemVaultV6.rho`.

No leak test was run. Each finding gives a test plan: the setup, the action and the assertion that fails if the leak is real. A finding marked **confirmed in code** follows from the code paths quoted. A finding marked **to verify** depends on a reachability fact that the test must establish first.

Severity:

- **Critical:** a term that holds no surface can create REV, or move another party's REV to itself.
- **High:** another party's REV moves without a seal that this party applied, or authority survives outside the envelope that granted it.
- **Medium:** authority comes from a surface that the design does not name, or a check depends on context, but the effect is bounded.
- **Low:** a hardening or consistency issue with no direct value effect.

## Summary

| # | Severity | Mechanism | Finding |
|---|---|---|---|
| F1 | Critical (to verify) | `rho:rev:genesis`, genesis-only URN filter | The filter checks only `new` binders. `rho:internal:registry_lookup` serves any URN in `urn_map`, which may include the genesis process. The genesis handler has no guard of its own, and a mint raises the recorded supply, so conservation still holds. |
| F2 | High | REV stack literals (`eval_cost_stack`) | The paying wallet is the running deployment's deployer (execution context). A stack literal in a persistent contract that another party triggers pays from that party. |
| F3 | High | Kind stack literals (`eval_kind_stacks`) | The same as F2, for token kinds. |
| F4 | High | `rho:system:cosignerId`, cosignatures | A cosignature covers the deploy data but not the deployer, the envelope signature or the other cosignatures. It can be lifted into another envelope with another deployer, and the cosigner's authority goes with it. |
| F5 | High | `rho:rev:bond` `"bond"` | No signer check. Any term that holds a party's `GDeployerId` can bond that party's REV, in any later deployment. |
| F6 | Medium | `rho:rev:wallet` `"transfer"` | Authority is the payer's `GDeployerId` value plus an execution-context check (the payer must sign the running envelope). No seal on the transfer redex is checked. |
| F7 | Medium | `rho:rev:bond` `"release"`, `"slash"` | Keyed on the system auth token, a fieldless singleton, and not on the PoS contract's own unforgeable names as §6.9 requires. The URN itself is reachable by any term. |
| F8 | Medium | Genesis-only filter state | The filter shares the fs-native flag. Every place that disables fs filtering also exposes `rho:rev:genesis`. One of those places (reporting replay) keys on `!with_cost_accounting`, not on "this is genesis". |
| F9 | Medium (P9) | Kind stacks bypass the journal | Kind deposits and transfers write state directly during the run. REV moves are journaled and settled. A second kind follows a different settlement path. |
| F10 | Medium (P9) | Conservation | Supply and the audit are REV only. No per-kind supply exists, and kind mints are not counted. The settlement conservation check sums all touched owners into one total, across kinds. |
| F11 | Low (P9) | `rho:rev:wallet` commands | `transfer`, `balance` and reservations exist only for REV. Kinds have `kindBalance` and stack literals only. |
| F12 | Low | `GateInput.named` | `named_signatures` treats a term that fails to normalize as naming nothing. It is safe only because such a deploy fails at run time. |
| F13 | Low | Hook authority, compute wallet | `replace_rev_hook` and `spend_compute` take a `signers` list and are not wired yet. When they are wired, the list must come from verified seals in the term, not from deploy data. |

What holds (no finding): the native journal channel, the supply channel and the hook channels are fixed `GPrivate` names, which no term can write in source. `install_genesis_hooks` refuses a second installation. Deploy parameters are ground values only (no unforgeables). The test-only `rho:test:deployerId:make` and `sys:test:authToken:make` are registered only in test runtimes. `check_region_signature` and the gate's `ForeignSignature` refusal verify that a seal named in the term belongs to a signer, which is the pattern P10 asks for.

## Findings

### F1 — Critical (to verify): the genesis mint may be reachable through the registry lookup

**Surface the design names:** genesis itself. The process must be unreachable from any later term.

**What grants authority today:** reachability of the URN. `rev_genesis` (`system_processes.rs:1190`) and `rev_stack::genesis::handle` check nothing about the caller:

- `"mint"` deposits on any listed address and raises the genesis supply by the same amount (`genesis.rs`, `mint`). The settlement and audit conservation checks compare holdings with that supply, so they still pass after a mint.
- `"bonds"` mints into bond wallets. It refuses only a validator that already holds a bond.
- `"hooks"` refuses a second installation (no finding).

**Reachable without the surface?** The only barrier is in `eval_new` (`reduce.rs:1535`). It refuses `new x(`rho:rev:genesis`)` while `filter_fs_native_urns` is set. Two observations:

1. `rho:internal:registry_lookup` is registered as a system process (`rho_runtime.rs:966`). Its handler (`system_processes.rs:1339`) serves any URN found in `urn_map` by string. It applies no genesis-only or fs-native filter. If `urn_map` contains `rho:rev:genesis` (it is registered as a `Definition` like the other system processes) and a user term can bind `rho:internal:registry_lookup`, the filter is bypassed.
2. The same handler would also serve the `rho:io:fs:native:*` URNs, which the same filter is meant to hide.

**Context instead of seal:** yes. Phase (genesis or not) is the only guard, and phase is held in a runtime flag.

**Test plan:** in a v6 shard after genesis, run a user deployment that binds the registry-lookup URN, asks it for the genesis URN, and sends a `"mint"` request for its own address.
- *Expected (no leak):* the run fails, and the deployer's balance and the genesis supply are unchanged.
- *Leak:* the balance and the supply both rise, and conservation still passes.

Add a second case for `"bonds"`, and a third that asks the lookup for an fs-native URN.

### F2 — High: REV stack literals pay from the running deployer

**Surface the design names:** a stack literal `s :: S` moves one unit from "the writer's wallet" (Decision 33). Under P10, the writer must be named in the term.

**What grants authority today:** `eval_cost_stack` (`reduce.rs`) takes `writer = deploy_data.deployer_id`, the deployer of whichever deployment evaluates the term. The term names only the payees.

**Reachable without the surface?** Yes. A stack literal inside a persistent contract runs in the deployment that triggers the contract. That deployment's deployer pays, though it applied no seal and wrote no stack.

**Context instead of seal:** yes. This is the "whoever is running this deployment" case that §4.7 rules out.

**Test plan:**
1. Block 1: party M installs a persistent contract on a public channel. Its body is a stack literal whose cells name M's wallet.
2. Block 2: party A sends one message on that channel. A's term has no stack literal and no signed region.

- *Expected:* A's wallet loses nothing beyond A's own charges.
- *Leak:* A's wallet loses one unit per cell, and M's wallet gains them.

### F3 — High: kind stack literals transfer from the running deployer

The same as F2, for token kinds. `eval_kind_stacks` (`reduce.rs:1774`) takes `writer = deploy_data.deployer_id` for a non-issuer kind stack and debits `kind_owner(kind, writer)`.

The issuer path (a mint) takes its authority from the kind's unforgeable name in the signature. That path is consistent with P10.

**Test plan:** as F2, with M's contract holding a transfer stack of a kind that A owns units of. The leak shows as A's units of the kind moving to M.

### F4 — High: a cosignature is not bound to its envelope

**Surface the design names:** a cosigner name confers authority "only because the cosignature over the envelope's bytes is verified" (§6.9).

**What grants authority today:** `Cosignature::sign` and `verify` (`casper_message.rs:1376-1410`) sign and verify `DeployData::to_message()`. That message is the term, timestamp, phlo price and limit, valid-after block, shard, expiration and parameters (`_to_proto`, `casper_message.rs:1512`). It leaves out:

- the deployer's public key, which is not part of `DeployData`
- the envelope signature
- the other cosignatures.

So a cosignature is a seal over the data, not over the envelope.

**Reachable without the surface?** Yes. A party that sees a cosigned envelope can build a second envelope with the same data and the same cosignature, signed as deployer by itself. In that envelope:

- the cosigner verifies (`verified_cosigners`)
- `rho:system:cosignerId:0` resolves to the cosigner's `GDeployerId`
- the cosigner counts as a signer for the wallet check (F6), for `check_region_signature` and for the gate.

Deduplication is by envelope signature, which differs, so both envelopes can run inside the validity window.

**Context instead of seal:** the seal is verified, but over a message that does not identify the envelope.

**Test plan:** A and B produce an envelope from A, cosigned by B, whose term transfers from B's wallet to C. M takes B's cosignature, builds an envelope with the same data and signs it as deployer.
- *Expected:* the gate or the runtime refuses M's envelope with respect to B.
- *Leak:* both envelopes are admitted and B pays twice.

### F5 — High: `rho:rev:bond` `"bond"` has no signer check

**Surface the design names:** "bonding draws on the validator's own wallet under its seal" (§6.9).

**What grants authority today:** `bond::handle` `"bond"` (`bond.rs`) accepts any `GDeployerId` in the term and debits that party's wallet into that party's bond wallet. Unlike `rho:rev:wallet` (F6), it does not check that the party signs the running envelope. The process is reachable by URN from any term, not only through `PoSV6`'s `bond` contract.

**Reachable without the surface?** A `GDeployerId` reaches a term only through `rho:system:deployerId`, `rho:system:cosignerId:<i>`, `rho:deploy:data` (which returns the running deployer's id), or by being passed as data. Once a party has passed its id to any contract, for example to `PoSV6.bond` or `SystemVaultV6.transfer`, a stored copy can bond that party's REV in a later deployment that the party does not sign.

The REV goes to the party's own bond, not to the attacker. Release needs the system auth token. So the effect is locked funds and a forced bond, not theft.

**Context instead of seal:** no seal is checked at all. Possession of the id value is the whole authority.

**Test plan:**
1. Block 1: A calls a contract of M's with A's deployer id, and M's contract stores it.
2. Block 2: M's own deployment reads the stored id and calls `rho:rev:bond` `"bond"` with it.

- *Expected:* the bond is refused.
- *Leak:* A's wallet falls and A's bond wallet rises in a block A did not sign.

### F6 — Medium: wallet transfers combine a capability value with an execution-context check

**Surface the design names:** the payer's seal on the deployment (or every component's seal, for a compound payer). §6.9 records that the "any deployer id" leak was closed in P3-02.

**What grants authority today:** `native::handle` `"transfer"` requires:

1. a `GDeployerId` (or a tuple of them) for the payer, in the term
2. `admissible(from, signers)`, where `signers` is the running envelope's deployer and verified cosigners (`system_processes.rs:1149`).

Neither checks that the transfer redex lies inside a region sealed by the payer.

**Consequence:** inside a deployment that A signs, any code that holds A's deployer id can move A's REV anywhere. That includes a third-party persistent contract to which A's term passes the id, for example to authenticate. The second condition comes from execution context, not from a seal in the term.

The design must choose between two readings:

- **Capability reading:** the id is itself the capability. Then the signer check is an ambient restriction.
- **Seal reading:** the redex must be sealed by the payer. Then possession of the id is not enough, and the check must look at the seal around the redex.

**Test plan:**
1. Block 1: M installs a contract that receives a deployer id and transfers from it to M.
2. Block 2: A's deployment sends its own deployer id to that contract and does nothing else.

The reading the design chooses decides which outcome counts as correct. Today A pays M.

### F7 — Medium: bond release and slashing are keyed on the system auth token

**Surface the design names:** release and slashing "must be reachable only through the PoS contract's own unforgeable names, not by any term that can name the process" (§6.9).

**What grants authority today:** `"release"` and `"slash"` (`bond.rs`) require `RhoSysAuthToken` in the term. `GSysAuthToken` has no fields, so all tokens are equal. Any holder of a token can release or slash any validator's bond: every system deploy, and any contract a system deploy passes the token to. The `rho:rev:bond` URN is reachable by any term. Only the token gates these two commands.

No path for a user term to obtain the token was found. It is injected only into system-deploy environments, and `PoS.rhox` and `PoSV6.rhox` check it without storing it. The finding is the deviation from the named surface, and the fact that a singleton token stands in for a per-contract capability.

**Test plan:**
- A user deployment calls `"release"` and `"slash"` with no token, and with every token-shaped value it can obtain. Expected: both are refused.
- A system deploy other than the close-block and slash deploys calls `"release"` for an arbitrary validator. Under the design, this must be refused. Today it would succeed.

### F8 — Medium: the genesis-only filter rides on the fs-native flag

`is_genesis_only_urn` is checked inside the `filter_fs_native_urns` branch of `eval_new`. That flag is turned off in three places:

| Place | Condition |
|---|---|
| `runtime.rs:419` | `play_deploys_for_genesis` |
| `replay_runtime.rs:126` | genesis replay (`is_genesis`) |
| `reporting_casper.rs:176` | `!with_cost_accounting` |

The third place keys on cost accounting, not on genesis. A reporting replay of a non-genesis block without cost accounting would let its deploys bind `rho:rev:genesis`. That replay serves reports, not consensus, so the effect is a wrong report.

The flag is also one `AtomicBool` shared by every use of a reducer. Any future exemption for fs-native URNs exempts the REV mint too.

**Test plan:** run the reporting path for a non-genesis block, with cost accounting off, on a deploy that binds `rho:rev:genesis`. Expected: refused.

### F9 — Medium (P9): token kinds take a separate, unjournaled path

`eval_kind_stacks` deposits and transfers directly on the hot store during the run. The REV paths journal their moves when the journal is open and settle them after the merge (`journaled_transfer`, `journaled_transfers`, `settle::kept_transfers`).

So for a kind:

- moves are not subject to reservations (`held`)
- moves are not dropped when the envelope's merge verdict is "not kept"
- moves do not appear in the settlement's transfer list.

This is the P2-21 problem (native writes and the v6 merge) for kinds. It is also the code-path difference that P9 forbids. `JournalEntry` has no kind field. Kinds are distinguished only through `kind_owner` hashing of the owner, which the journal could carry, but the kind path does not use the journal.

**Test plan:** a block with two envelopes that conflict and are merged in block order, where the conflicted (not kept) envelope moves units of a kind.
- *Expected:* the not-kept envelope's kind move is absent from the settled state, as a REV move would be.
- *Leak:* the kind move persists.

### F10 — Medium (P9): conservation is REV only, and the settlement check sums across kinds

- `genesis_supply`, `audit` and `Audit::conserved` know one supply, the REV genesis supply. Kind mints (`eval_kind_stacks`, issuer path) raise kind balances without recording any supply, so no per-kind conservation check exists. Decision 46 requires one per kind.
- `settle_block` checks `held_by(touched)` before and after settlement as one `u128` over all touched owners. If a kind owner were among them, a conversion of REV units into units of a kind would leave the total unchanged and pass. Today kind owners are not touched, because of F9, so the check covers REV only. That is a P9 gap in itself.

**Test plan:** after a block that mints and moves units of a kind, a per-kind audit must report the kind's supply equal to the sum of its holdings. Today no such audit exists, so the test cannot be written without new code. That absence is the finding.

### F11 — Low (P9): the wallet process is REV-shaped

`rho:rev:wallet` offers `find`, `balance`, `transfer` and reservation-aware balances for REV, and only `kindBalance` for kinds. A second kind cannot use `transfer`, reservations or `visible_balance`, and moves only through stack literals (F9).

**Test plan:** none needed. The command list shows the difference.

### F12 — Low: an unnormalizable term names nothing at the gate

`executor::named_signatures` returns an empty list when the term does not normalize (`unwrap_or_default`). The gate then treats the deployer as the only payer. This is safe today only because such a deploy fails at run time before any signed region runs. It is a fail-open default in a check that P10 relies on.

**Test plan:** a deploy whose term fails normalization must be refused or charged to the deployer with no other wallet touched. Assert that no wallet other than the deployer's changes.

### F13 — Low: hook replacement and compute-wallet spending are not wired

`replace_rev_hook` and `spend_compute` (`hooks.rs`, `compute.rs`) check a `signers` list against the genesis hook authority. No system process calls them on this branch. When they are wired, the list must come from verified seals in the term, for example a compound signature over the hook authority's addresses (§6.8). If it comes from the running envelope's signers, the same execution-context pattern as F6 follows.

**Test plan:** when they are wired, a deployment signed by enough hook-authority members, but whose replacing redex is not sealed by them, must be refused.

## Kind-specific code on the payment path (P9 list)

A second token kind would need new or different code at these places:

1. `eval_kind_stacks`: its own deposit and transfer path, with no journal or reservation (F9).
2. `settle.rs`: settlement applies journal transfers and REV charges. Kinds have no settled moves (F9).
3. `genesis.rs` and `settle::audit`: one REV supply and one conservation check. No per-kind supply (F10).
4. `rho:rev:wallet`: `transfer`, `balance` and reservations for REV only (F11).
5. `reserve.rs` and the gate's commitments: commitments are in REV, which is correct for charges (`phloPrice` is REV per phlo). But no reservation exists for kind moves, so a kind's available balance cannot account for in-flight moves.

Charges staying in REV is policy (§4.6), not a P9 finding.

## Open questions for review

1. **F6:** under P10, is a `GDeployerId` a capability that its holder may delegate by passing it, or must every draw on a wallet sit inside a region sealed by the wallet's owner? The answer decides F2, F3, F5 and F6 together.
2. **F4:** should a cosignature cover the envelope (the deployer key and the deploy signature, or the hash of the whole signed envelope less the cosignatures)? Binding to the deployer key is enough to stop reuse under another deployer. Binding to the deploy data alone is not.
3. **F7:** should release and slashing move behind PoS-held unforgeable names, as §6.9 says, with the system auth token kept only for the system deploys' own calls into PoS?

# A-6: authority audit of patch/P2-integration and patch/P3-04 against P10 and P9, with the A-7 re-audit of the fixes

- **Branch audited:** `patch/P2-integration` at `3b5a7e90a` (revision 1). `patch/P3-04` at `45bfb2246`, which includes P3-07b and the `# P` seal rule (revision 2, below). `patch/FX-1` at `8886c8c9a` and `patch/FX-4` at `0fbf80c85` (revision 3, action A-7). `patch/FX-2` at `4aed4d852` and `patch/FX-3` at `df0db3146` (revision 4). `patch/FX-3` at `d3c5d383a`, `patch/FX-5` at `c0d48af7f`, `patch/FX-2-refunds` at `38c8ca21c` and `patch/FX-7` at `d1c1a5df0` (revision 5, action A-8). `patch/FX-6` at `ead0663ef`, route tests (revision 6). `patch/FX-8b` at `1c281b7a0` (revision 7). `patch/FX-11` at `1b2d387e1` (revision 8).
- **Design:** stall-loop design revision 8, §4.6, §4.7, §6.1 (P9, P10) and §6.9. Paper: `cost-accounting-as-monad/continued-gslt-cost-v2.tex`, §§3–6 and §8 (capabilities).
- **Auditor:** lane A1. Lane A1 wrote none of the native REV code.
- **Scope:** revision 1 gives findings only. Revision 2 adds the four suspects that lanes B and C reported, each with a severity and a proposed fix, and audits the code that P3-04 adds. Each proposed fix becomes a patch or a decision after review.
- **Ruling used in revision 2:** open question 1 is answered with the seal reading (b). Every draw on a wallet must sit inside a region that the wallet's owner sealed.

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

## Revision 2: the reported suspects and patch/P3-04

- **Code audited:** `patch/P3-04` at `45bfb2246`. This is `patch/P2-integration` plus P2-21's canonical journal order (`33b46e430`), lane C's P3-07b (`9bf528ed5`), and lane B's P3-04 (`afbbf1834`, merged in `45bfb2246`).
- **Files read:** `reduce.rs` (`eval_inner`, `eval_receive`, `check_region_signature`, `eval_cost_signed_term`, `pay_stacks`, `mint_stacks`, `stack_payees`), `rev_stack/{process,region,kind,compute,hooks,native,bond,journal}.rs`, `casper/src/rust/v6/{gate,settle,hooks}.rs`, and the P3-04 and P3-07b decision logs.
- **Method:** a code reading, as in revision 1. No leak test was run. The test plans are setups and assertions only.

### Status of F1 to F13 at `45bfb2246`

| # | Status | Note |
|---|---|---|
| F1 | Open | The genesis process, the registry lookup and the URN filter are unchanged. |
| F2 | Open | Restated as F14. `pay_stacks` keeps `writer = deploy_data.deployer_id`. |
| F3 | Superseded | `eval_kind_stacks` is removed. The same pattern is now in `pay_stacks` for a kind. See F15. |
| F4 | Open | The cosignature message is unchanged. F17 depends on it. |
| F5 | Open | `bond` now journals through `JournalEntry::transfer`. It still checks no signer. |
| F6 | Ruled | The seal reading (b) applies. The fix is in F14: `"transfer"` must find a region sealed by `from` around the redex. |
| F7 | Open | Unchanged. |
| F8 | Open | Unchanged. |
| F9 | Fixed | P3-07b journals kind moves, and settlement applies them per kind (P3-07b decisions 5, 9 and 10). |
| F10 | Partly fixed | Settlement now checks conservation per kind over the touched wallets (P3-07b decision 11, `settle.rs` `held_by_kind`). `settle::audit` is still REV only. The whole-state audit per kind is in `patch/P2-09`, which is not on this branch. |
| F11 | Open | `kindBalance` now uses the same code as `balance`. `"transfer"` and reservations are still REV only. |
| F12 | Open | Unchanged. |
| F13 | Open | Unchanged. F16 adds the spender that it did not cover. |

### Summary of revision 2

| # | Severity | Source | Finding | Needs a ruling |
|---|---|---|---|---|
| F14 | High (confirmed in code) | Lane C suspect 2 | A stack literal pays from the running deployer, in `eval_cost_stack` and in `pay_stacks`. | Yes: authority and charging |
| F15 | High (confirmed in code) | Lane C suspect 3 | The source of a kind transfer is the running deployer's kind wallet. | Yes: authority |
| F16 | High (latent: not wired) | Lane B suspect 1 | `ComputeSpender::InstalledHook` spends the compute wallet with no check. Its authority is "the hook is running". | No, while it stays unwired |
| F17 | Medium (High with F4) | Lane B and C suspect 4 | The runtime check compares a seal with the running deployment's signers. It does not verify that the owner applied the seal to this term. | Yes: consensus (the state holds the region) |
| F18 | Medium (latent until M-9) | P3-04 | The `# P` check compares the unsubstituted forms, but a cell that names `# P` deposits into the wallet of the substituted P. An open template and all its instances share one seal. | With M-9 |
| F19 | Low | P3-04 and P3-07b | The gate does not name a variable seal over a stack body. It assumes the variable is a kind. | No |
| F20 | Low (latent until M-9) | P3-04 | A `# P` body can read the runner's context. A self-sealed P that pays "the deployer" pays whoever fires it. | With M-9 |

F14, F15 and F17 have one root. The reducer has no record of which seal encloses the redex that it evaluates, so every check falls back to the running deployment. The proposed fixes give the reducer that record once (F14), and F15, F17 and the F6 fix use it.

### F14 — High: a stack literal pays from the running deployer (lane C suspect 2)

**Code:** `eval_inner` (`reduce.rs:327`) calls `eval_cost_stack` for every bare stack in a `Par`. `eval_cost_signed_term` (`reduce.rs:1819`) calls `pay_stacks` for a stack body whose signature names a kind transfer. Both reach `pay_stacks` (`reduce.rs:1712`), which takes `writer = WalletOwner::from_public_key(deploy_data.deployer_id)`.

**Why it breaks P10:** the writer is "whoever runs this deployment". A seal around the stack has no effect on the payer. `{% A %}[ stack ]` passes `check_region_signature` when A signs the running envelope, and then the stack pays from the deployer, not from A. A `# P` region pays from the deployer, not from the wallet of `# P`.

**Reachable:** yes. The gate does not name a bare stack, because `named_owners` visits only the cells of a stack (`region.rs`, `visit_par`). A persistent contract whose body is a bare stack passes the gate, and each deployment that fires it pays. This is the test plan of F2, unchanged.

**Proposed fix (seal reading b):**

1. Give the reducer a seal context: the owner named by the innermost enclosing region whose seal passed `check_region_signature`. `eval_cost_signed_term` sets it for the evaluation of its body. A `# P` region sets it to `process_owner(P)` after substitution (see F18).
2. `pay_stacks` takes the writer from the seal context. A stack with no enclosing wallet seal is refused. The deployer is no longer a default payer.
3. Keep the context across suspension. When an evaluation inside a region stores a continuation (a receive body, or the body of a persistent send), store it wrapped in the same seal. When the continuation fires, `eval_cost_signed_term` checks the seal again (F17 decides against what) and sets the context again.
4. `"transfer"` in `rho:rev:wallet` and `"bond"` in `rho:rev:bond` read the same context. Each refuses a draw on a wallet other than the context owner, or a component of a compound context owner. This closes F5 and F6 as the ruling requires.
5. The gate computes the paying wallets from the same rule, so that the gate's commitment and the run's draw name the same wallet. `named_owners` must then name the enclosing seal of every stack and every native draw. A stack outside every seal is a static refusal.

**Escalation:** the fix changes who pays and what the gate commits, so it touches charging and authority. The change of payer also changes the state root of any block that holds a stack literal. It needs a ruling before a patch.

**Test plan:** the F2 plan, plus two cases.
- `{% A %}[ stack ]` in an envelope that A and B sign, deployed by B. *Expected after the fix:* A's wallet pays. *Today:* B's wallet pays.
- A bare stack in a deployment. *Expected after the fix:* refused. *Today:* the deployer pays.

### F15 — High: the kind-transfer source is the running deployer's kind wallet (lane C suspect 3)

**Code:** `{% bundle0{*k} %}[ cells ]` reaches `pay_stacks(kind, ...)` through `eval_cost_signed_term`, and debits `(deployer, k)`. The signature names the kind, not the payer. The term has no place to name the payer.

**Why it breaks P10:** as F14. The authority to spend a kind balance comes from the running deployment. The mint path is correct. Its authority is the unforgeable name `k` in the signature (P3-07b decision 7).

**Reachable:** yes. As F14, with a kind that the firing deployer holds.

**Proposed fix:** take the source from the F14 seal context, and the kind from the stack's own signature. The form is `{% A %}[ {% bundle0{*k} %}[ cells ] ]`, which moves units of k from `(A, k)`. One alternative is a compound signature that joins the payer and the kind, but the compound signature already means a joint wallet (`WalletOwner::Compound`). A second meaning would need a new signature form. The nesting form needs no new syntax and reuses one rule for REV and every kind (P9).

**Test plan:** the F3 plan. Also a case where A holds units of k and B fires `{% A %}[ {% bundle0{*k} %}[ cells ] ]` in an envelope that only B signs. *Expected after the fix:* refused, because the A seal does not verify (F17). *Today:* B's units move.

### F16 — High (latent): `ComputeSpender::InstalledHook` spends because it is running (lane B suspect 1)

**Code:** `spend_compute` (`compute.rs`) checks the hook authority only for `ComputeSpender::Authority(signers)`. For `ComputeSpender::InstalledHook` it checks nothing and transfers from the compute wallet. At `45bfb2246` no system process calls `spend_compute`. Only the unit test in `hooks.rs:374` uses `InstalledHook`.

**Why it breaks P10:** the variant names no seal and no name. Its authority is a claim about execution context: "this code runs as the installed hook". When it is wired, every path that the hook's run reaches inherits the claim. This includes contracts that the hook calls and terms that the hook receives as data.

**Reachable:** not today. The finding is severe because a wiring patch would make it High at once, and the shape of the API invites that patch.

**Proposed fix:**

1. Remove `InstalledHook`. `spend_compute` takes the owner of the enclosing seal (the F14 context), not a spender variant.
2. Admit a spend in two cases only:
   - The seal owner is a compound of at least `threshold` hook-authority members. This is the §6.8 rule, with verified seals in place of a `signers` list.
   - The seal is `# H`, and H is the contract that the hook channel holds. The `# P` rule of P3-04 then gives the authority to H's own body and to nothing that H calls.
3. Apply the same rule to `replace_rev_hook`, which closes F13.

**Test plan:** after wiring, a hook body H that calls a contract C, where C asks for a compute spend. *Expected:* refused, because C is not inside `# H`. A second case: a deployment that installs a term equal to H but not as the hook. *Expected:* refused, because the hook channel does not hold it. The second case depends on how the hook channel names H, which the wiring patch decides.

### F17 — Medium (High with F4): the runtime check compares with deploy data (suspect 4)

**Code:** `check_region_signature` (`reduce.rs:1663`) builds `signers` from `deploy_data.deployer_id` and `deploy_data.cosigners` of the running deployment. For every seal component other than `# P`, it substitutes the signature and requires `admissible(owner, signers)`. `eval_receive` calls it for bind seals at the time the receive is evaluated (`reduce.rs:1313`). `eval_cost_signed_term` calls it each time a region is evaluated, which for a stored continuation is the time it fires.

**Why it breaks P10:** a seal `{% A %}` names A, which is correct. But the check does not verify that A sealed this term. It verifies that A signed whichever envelope runs now. Two consequences follow:
- A region that A sealed in one envelope is checked against the signers of the envelope that fires it later. Whether it runs depends on the runner. Authority can survive outside the envelope that granted it, if A's later envelope fires it.
- With F4, M can make B a signer of M's envelope by reusing B's cosignature over the same deploy data.

**Reachable:** the gate's static walk (`named_owners`) refuses an envelope whose term names a foreign seal at any depth, including inside continuations and quoted data. So a third party cannot introduce a region sealed by A through its own source. Two gaps remain: F19, and regions that arrive as data from an envelope that A did sign. The bound effect today is Medium. With F4 it is High.

**Proposed fix:**

1. Verify the seal at its origin, not at its use. The gate already proves, for every non-hash seal in an admitted term, that the owner signed that envelope. Make that proof the authority. Record in each sealed region the identity of the envelope that admitted it (the envelope signature). At run time, compare the seal with that envelope's verified signers, not with the running deployment's.
2. The record is part of the term, so it enters the tuple space and the state root. The normalizer must fill it from the deploy, and a term must not be able to forge or strip it. The design must say whether the record is a field of `CostSignedTerm` or a wrapper that only the reducer can build.
3. Fix F4 first. A seal that is verified at its origin is only as strong as the cosignature that admitted it. The cosignature must cover the deployer key and the deploy signature.

**Ruling needed:** may a region that A sealed in A's envelope run later, in an envelope that A did not sign? P3-04 answers yes for `# P` (the test `a_stored_hash_sealed_term_runs_for_any_deployment_that_fires_it`). For a key seal, the recommendation is also yes once step 1 is in place, because the authority is then A's verified seal on that exact term. The alternative is to refuse a key-sealed region outside its origin envelope. That alternative is the closest to today's practical effect, but it makes a persistent contract sealed by its owner impossible.

**Test plan:**
1. Block 1: A's envelope stores `for (_ <- @"pub") { {% A %}[ P ] }`.
2. Block 2: B fires it from an envelope that A does not sign.
3. Block 3: A fires it from an envelope whose term does not contain P.

- *Today:* block 2 is refused and block 3 runs. The runner decides.
- *After the fix:* both follow the ruling, and both give the same result.

### F18 — Medium (latent until M-9): the `# P` check and the `# P` wallet use different forms

**Code:** `check_region_signature` calls `split_hash_seals(&signature)` and `seals_itself(process, body)` on the unsubstituted signature and body (P3-04 decision 2: "at the same substitution level"). `stack_payees` resolves a cell `# P` through `substitute_cost_signature` and then `signature_owner`, which gives `process_owner` of the substituted P (P3-04 decision 4).

**Consequences:**
- The seal check accepts `{% #Q %}[ Q ]` where Q has free variables. Every instance of the template passes, for every value that any runner supplies. The seal is "the template", not "this process".
- Deposits go to the wallet of the substituted process. The draw path is not built yet (the gate refuses `# P` until M-9). If draws key on the unsubstituted form, deposits and draws use different wallets. If draws key on the substituted form, the seal check and the wallet identity disagree.

**Proposed fix:** do the check and the key at one level. Substitute the seal and the body first. Admit `# Q` only if both are closed and equal in sorted form. Key the wallet on `process_owner` of that closed process. A template then gets one wallet per instance, and an instance sealed with its own hash is the only process that can draw on it.

**Test plan:** `for (@x <- @"in") { {% #(@"out"!(x)) %}[ @"out"!(x) ] }`, fired once with x = 1 and once with x = 2, and a stack cell that names `#(@"out"!(1))`. *Expected after the fix:* the two firings have two wallets, and the deposit lands in the wallet of the x = 1 instance only.

### F19 — Low: the gate assumes that a variable seal over a stack body is a kind

**Code:** `visit_par` (`region.rs`) does not name a signed term whose body is a stack body and whose signature `names_a_kind`. For a bound variable that it cannot resolve, `names_a_kind` returns true. At run time the variable can resolve to a wallet name. `stack_kind` then returns `None`, `check_region_signature` compares the wallet with the runner's signers (F17), and the stack pays from the runner (F14).

**Effect:** the gate admits a term that names a wallet seal it did not check. No value moves beyond F14. This is a fail-open default, as F12 is.

**Proposed fix:** name an unresolved variable seal as unresolved, unless the gate can prove that it is a kind. At run time, refuse a stack-body region whose signature resolves to a wallet name rather than a kind. A wallet-sealed stack then uses the nesting form of F15.

### F20 — Low (latent until M-9): a self-sealed process can read the runner's context

**Code:** P3-04 makes `# P` self-certifying. P's body may bind `rho:system:deployerId`, `rho:deploy:data` or `rho:system:cosignerId:<i>`. Those URNs resolve to the running deployment.

**Effect:** when draws on the `# P` wallet exist, a P that pays "the deployer" pays whoever fires it. The URN is part of P, so a funder can see it in P's hash. But the authority over the funds is then the runner's identity, which is the execution-context pattern that P10 rules out.

**Proposed fix:** at M-9, refuse a `# P` seal whose body binds a URN that resolves from the running deployment. If P must pay a party, the party is a name in P, fixed when P is written.

### What P3-04 and P3-07b get right

- The `# P` seal certifies itself from the term. The runner's identity plays no part (P3-04 decision 2), and a bind seal `# Q` is refused (decision 3). This follows P10.
- The gate still refuses an envelope that names `# P` (decision 5), so no draw on a process wallet is reachable before M-9.
- A kind mint needs the unforgeable name of the kind in the signature after substitution. The public form `bundle0{*k}` transfers and never mints. REV mints are refused in the journal, in settlement and in the record decoder (P3-07b decisions 6 and 7).
- Kind moves go through the journal and settle per kind. Conservation is checked per kind over the touched wallets (F9 fixed, F10 partly fixed).

## Revision 3 (action A-7): re-audit of patch/FX-1 and patch/FX-4

- **Branches audited:** `patch/FX-1` at `8886c8c9a` (lane B, cut from `patch/P2-integration`, `3b5a7e90a`). `patch/FX-4` at `0fbf80c85` (lane B, cut from `patch/P3-05`, `685627c28`, with FX-1 merged in at `67104a363`).
- **Findings re-audited:** F1, F8 (FX-1). F7, F13, F16, F18, F20 (FX-4).
- **Method:** a code reading of each fix and of the fixing lane's tests against the A-6 test plans. The fixing lanes own the tests (decision of 2026-11-01). This revision runs no tests and adds none. A gap between a test plan and the lane's tests is recorded as a finding.
- **Files read:** `system_processes.rs` (`urn_refusal`, `UrnPhase`, `registry_lookup`, `rev_genesis`, `rev_bond_admin`), `reduce.rs` (`eval_new`, `check_region_signature`), `rho_runtime.rs` (`enter_genesis_phase`, `setup_reducer`, `setup_maps_and_refs`), `casper` `runtime.rs`, `replay_runtime.rs`, `reporting_casper.rs`, `rev_stack/{genesis,bond,process,hooks,compute}.rs`, `PoSV6.rhox`, the FX-1 and FX-4 decision logs, and the tests that those logs name.

### Results

| # | Fix | Result | New findings |
|---|---|---|---|
| F1 | FX-1 decisions 1 to 4 | **Closed.** Lane B confirmed the leak on `3b5a7e90a` (mint, bonds and an fs-native URN through the lookup). `eval_new` and `registry_lookup` now apply one refusal, `urn_refusal`, before the lookup reads `urn_map` or the versioned registry. The `rho:rev:genesis` handler also refuses every call outside the genesis phase, so a handle kept from genesis is refused when used. | F24 (Low) |
| F8 | FX-1 decisions 1 and 5 | **Closed.** The genesis phase has its own flag, entered only by genesis play, genesis replay and reporting of a parentless block. Lifting the fs-native filter no longer exposes the genesis URNs. | — |
| F7 | FX-4 decisions 8 to 10 | **Closed.** `release` and `slash` moved to `rho:rev:bondAdmin`. Holding that name is the authority, and the name is genesis-only. `PoSV6.rhox` binds it in its top-level `new` and uses it only as a channel, never as data. The calls sit inside `closeBlock` and `slash`, which check the system auth token before they reach it. | F25 (Low) |
| F16 | FX-4 decisions 5 to 7 | **Closed for the API.** `ComputeSpender` and `InstalledHook` are gone. `spend_compute` takes a seal and admits only the authority's quorum or `# H` of the installed hook. The function stays unwired until FX-2 supplies the seal record. | F23 (Low), F26 (gap) |
| F13 | FX-4 decisions 5 to 7 | **Closed for the API**, with the same condition as F16. | F23 (Low), F26 (gap) |
| F18 | FX-4 decisions 1 to 3 | **Closed.** The seal and the body are substituted before the check. `# Q` is admitted only when both are closed and equal in sorted form, and the wallet is keyed on that closed process. The gate's self-sealed check uses the same function, `hash_seal_admissible`. | — |
| F20 | FX-4 decision 4 | **Partly closed.** See F22. The rule searches the body's encoding for five URN strings. Four of them are bound by the author, not by the runner. Two names that do resolve from the runner are not covered. | F22 (Low) |

**The check you asked for, `rho:rev:bondAdmin` through the registry lookup:** covered. `is_genesis_only_urn` returns true for `rho:rev:genesis` and `rho:rev:bondAdmin` (`genesis.rs`, FX-4). `registry_lookup` calls `urn_phase.refusal` on the URN string before it reads `urn_map` or delegates a versioned URN, and it reads the string from either a `String` or a `Uri` argument. Outside the genesis phase the lookup therefore refuses `rho:rev:bondAdmin` with "reserved for genesis". `rev_bond_spec::only_a_holder_of_the_bond_admin_name_can_release_or_slash_a_bond` checks this case, the direct `new`, and the old route through `rho:rev:bond`. Two conditions apply:

1. The coverage exists only on FX-4. `patch/FX-1` alone has no `rho:rev:bondAdmin`. FX-4 must merge with FX-1 or after it, which its history already does.
2. The filter matches the exact string. F24 records the hardening for other spellings.

### Correction to revision 1

F8 listed the reporting replay (`!with_cost_accounting`) as a place that keyed on cost accounting rather than on genesis. In `reporting_casper.rs`, `with_cost_accounting(parents)` is `!parents.is_empty()`. The condition is therefore "the block has no parents", which is the genesis test. That row was not a leak. FX-1 makes the meaning explicit with its own `is_genesis` argument.

### Coverage of the A-6 test plans by the lanes' tests

| Plan | Lane tests | Covered | Gap |
|---|---|---|---|
| F1: mint, bonds and an fs-native URN through the lookup after genesis | `rev_genesis_spec`: `a_user_deploy_cannot_mint_through_the_registry_lookup`, `..._mint_bonds_...`, `..._reach_an_fs_native_urn_...`, `a_genesis_handle_kept_past_genesis_is_refused_by_the_handler`. `gated_executor_spec::a_user_deploy_cannot_reach_the_genesis_mint_through_the_registry_lookup` (v6 shard, supply and balance unchanged, replay agrees). | Yes, with a case added for a kept handle | — |
| F8: reporting of a non-genesis block | `lifting_the_fs_filter_alone_does_not_expose_the_genesis_mint` | Yes for the shared flag | The reporting case cannot occur (see the correction above). No test needed. |
| F7: a user deploy calls release and slash, with and without a token | `rev_bond_spec::only_a_holder_of_the_bond_admin_name_can_release_or_slash_a_bond` | Yes | F25: no test that a system deploy other than PoS's own path cannot reach `rho:rev:bondAdmin`, and no end-to-end test through `PoSV6.rhox` (the FX-4 log defers it to v6 slashing). |
| F16: a contract that the hook calls asks for a compute spend, and a term equal to H but not installed | `hooks::tests::the_compute_wallet_starts_empty_and_spends_only_under_the_authority` | At unit level only | F26: the plan's cases are tested as owner values passed to `spend_compute`, not through the reducer, because nothing is wired. |
| F13: replacement by enough members whose redex is not sealed by them | `hooks::tests::replacing_the_rev_hook_needs_the_threshold_of_genesis_addresses` | At unit level only | F26, as above. |
| F18: a template fired with x = 1 and x = 2 | `hash_seal_spec::each_instance_of_a_hash_sealed_template_has_its_own_wallet`, `a_hash_seal_is_checked_on_the_substituted_process` | Yes | — |
| F20: a self-sealed P that pays "the deployer" | `hash_seal_spec::a_hash_sealed_body_may_not_read_the_running_deployment` | No | F22: the test binds `rho:system:deployerId`, which the normalizer fills with the author's id. It tests the over-broad part of the rule and not a name that resolves from the runner. |

### F22 — Low: the F20 rule refuses author context and misses runner context

**Code:** `reads_runner_context` (`process.rs`, FX-4) searches the encoding of a `# P` body for `rho:system:deployerId`, `rho:rchain:deployerId`, `rho:system:deployId`, `rho:system:cosignerId:` and `rho:deploy:data`. The gate's `self_sealed_envelope` and the reducer's `check_region_signature` both apply it.

**What the rule gets wrong:**

1. **Over-refusal.** The first four URNs are normalizer injections (`normalizer_env.rs`). `eval_new` resolves an injected URN from `New.injections`, which the normalizer filled when the term was compiled (`reduce.rs`, `add_urn`, the branch for a URN that is not in `urn_map`). The value is the id of the deployment that wrote P, fixed in P. It does not change with the runner. Refusing it is safe but refuses legitimate terms. Revision 2's proposal for F20 named "`rho:system:deployerId`, `rho:deploy:data` or `rho:system:cosignerId:<i>`" as names that resolve to the running deployment. For the injected URNs, that statement was wrong. The error came from the audit, and the fix followed it.
2. **Under-refusal.** Two system processes in `urn_map` do answer from the running block or deployment: `rho:deploy:data` (in the list) and `rho:block:data` (not in the list). The URN filter does not refuse either one, so `registry_lookup` serves both. A `# P` body can therefore reach `rho:deploy:data` through the lookup with a URN string built at run time, which a search of the encoding cannot find. It can reach `rho:block:data` directly.

**Effect today:** bounded. Under the interim payer rule (P3-04 decision 21), settlement debits a process wallet only for the charges of P's own work. A runner-context read cannot redirect the wallet's funds. The finding becomes Medium when FX-2 makes the enclosing seal the payer of stacks and native draws, because a `# P` body could then pay "whoever runs this" from P's wallet.

**Proposed fix:** refuse by resolution, not by encoding. With FX-2's seal record, the reducer knows when it evaluates inside a `# P` region. Pass that state to `urn_refusal`, and refuse `rho:deploy:data`, `rho:block:data` and any later URN that answers from the runner, in `eval_new` and in `registry_lookup` alike. Drop the four injected URNs from the refusal, or keep them refused by an explicit decision that says why. Until FX-2 lands, add `rho:block:data` to the list, and record the registry-lookup route as a known gap.

**Test plan for lane B:** inside a self-sealed P, (a) bind `rho:block:data`, and (b) obtain `rho:deploy:data` through `rho:internal:registry_lookup` with a URN string that does not appear literally in P. *Expected after the fix:* both refused. A third case binds `rho:system:deployerId` in P. *Expected:* the value equals the author's id whoever fires P, so the decision on item 1 decides whether it is admitted.

### F23 — Low: the installed hook can replace itself

**Code:** `replace_rev_hook` (`hooks.rs`, FX-4) admits a seal that `seal_authorizes` accepts, and `seal_authorizes` accepts `# H` for the installed hook H. H can therefore install its successor without the authority's quorum, and the successor can do the same.

**Effect:** the authority approves H's code, which includes any successor logic. But replacement then leaves the authority's review after the first step. Revision 2 proposed this rule for F13 ("apply the same rule to `replace_rev_hook`"), so the gap came from the audit.

**Proposed fix:** `replace_rev_hook` admits the authority's quorum only. `spend_compute` keeps both cases. Recorded for the FX-2 wiring, since the function is not wired yet.

### F24 — Low: genesis-only URNs match one exact spelling

**Code:** `is_genesis_only_urn` compares the URN string with `rho:rev:genesis` and `rho:rev:bondAdmin` exactly. A versioned spelling such as `rho:rev:bondAdmin:1.0.0` is not refused. Today it resolves nowhere: `eval_new` finds no `urn_map` entry or injection, and the lookup's versioned branch asks the registry, which holds no entry for it.

**Effect:** none today. A future registration of a versioned genesis-only URN, or a second `urn_map` key for the same fixed channel, would bypass the filter.

**Proposed fix:** key the refusal on the fixed channel rather than the string. Refuse any URN whose `urn_map` value is the bundle of `FixedChannels::rev_genesis()` or `rev_bond_admin()`, and refuse their versioned forms by prefix. Add an assertion at runtime setup that no other `urn_map` key maps to either channel.

### F25 — Low: genesis is the only barrier around `rho:rev:bondAdmin`, and no test covers the PoS path

**Code:** any deploy that runs while the genesis phase is on can bind `rho:rev:bondAdmin` or `rho:rev:genesis`. FX-4 decision 9 relies on PoSV6 being the only genesis deploy that binds the name. No check enforces that.

**Effect:** the genesis deploys are composed by the node from blessed contracts, so the assumption holds today. A genesis composition that adds a deploy from configuration (for example, a future use of the genesis fs bundle) would inherit both names.

**Proposed fix:** record the assumption in the genesis composition. Add a test that lists the genesis deploys whose terms bind either URN and asserts that the list is exactly `PoSV6` for `rho:rev:bondAdmin` and the REV genesis deploys for `rho:rev:genesis`. Add the end-to-end release test through `PoSV6.rhox` when v6 slashing lands, as the FX-4 log says.

### F26 — Gap: the F13 and F16 plans are tested only at unit level

The lane's tests pass owner values to `spend_compute` and `replace_rev_hook` directly. The A-6 plans need a seal that the reducer derives from a region, which exists only after FX-2. This is not a defect in FX-4, because decision 7 keeps both functions unwired. The reducer-level tests must arrive with the wiring patch, and that patch needs re-audit under A-7.

### What remains open after FX-1 and FX-4

- F2/F14, F3/F15, F5, F6, F17: FX-2 (lane C, then lane B).
- F4: FX-3 (lane C).
- F12, F19: FX-5 (lane C).
- F9, F10, F11: as in revision 2. F10's whole-state audit per kind is in `patch/P2-09`.
- F22 to F26: above. F22 and F23 depend on FX-2.

## Revision 4 (action A-7): re-audit of patch/FX-2 and patch/FX-3

- **Branches audited:** `patch/FX-2` at `4aed4d852` (lane C, cut from `patch/P3-04` at `8095c53b0`, with FX-1 merged in at `e82192f70`). `patch/FX-3` at `df0db3146` (lane C, cut from `patch/P3-04` at `8095c53b0`, with `patch/P3-02b` merged in at `d01d425e2`).
- **Findings re-audited:** F2/F14, F3/F15, F17, F22, F23 (FX-2). F4 (FX-3). F5 and F6 stay with lane B, which wires `rho:rev:wallet`, `rho:rev:bond` and the gate's paying wallets to the record (FX-2 open item 2).
- **Method:** as in revision 3. A code reading of each fix and of the lane's tests against the A-6 test plans. No tests were run or added.
- **Files read:** `rev_stack/seal.rs`, `reduce.rs` (`region_signers`, `stack_writer`, `check_region_signature`, `pay_stacks_from`, `mint_stacks`, `eval_cost_signed_term`), `executor.rs`, `settle.rs` (`settle_funders`), `rev_stack/journal.rs` (`journaled_transfers`), `casper_message.rs` (`Cosignature`, `DeployData::_to_proto`), `gate.rs`, the FX-2 and FX-3 decision logs, and the tests that they name.

### Results

| # | Fix | Result | New findings |
|---|---|---|---|
| F2/F14 | FX-2 decisions 1, 2, 7, 10 | **Closed on v6 runtimes.** A stack draws on `current_owner()`, the innermost enclosing seal. A deployment runs under the implicit seal of its deployer (Decision 51). A fired continuation runs under the record stored with it, so stored code keeps its author's seal. With records on and no record, a stack is refused. A runtime without records (legacy, genesis, the close-block runtime) keeps the old writer, the deployer. | F31 (Medium) |
| F3/F15 | FX-2 decision 11 | **Closed in the reducer.** The normalizer folds the nested form into one compound signature. The payer elements pass the region check against the origin, and the stack draws on them. With no payer element, the stack draws on the enclosing record. A mint still takes its authority from the kind name. The gate does not yet name the payer of the nested form, so an envelope that uses it is refused at the gate (FX-2 open item 1, with F19 in FX-5). | — |
| F17 | FX-2 decisions 4, 7, 9, 12 | **Closed, with one interaction.** A region is checked against the verified signers of the envelope that admitted the code, `current().origin`, not against the running deployment. Only `consume` writes the stored record (`TaggedContinuation` field 4), and the normalizer cannot produce that field. | F28 (Medium) |
| F4 | FX-3 decisions 1 to 5 | **Closed for lifting.** A cosignature signs a domain-separated, length-prefixed preimage of the deploy data, the deployer's key, the deployer's signature and its algorithm. Every caller passes the signed envelope. A lifted cosignature makes the envelope's cosignatures invalid, and the gate refuses it with `InvalidTerms`. | F27 (Medium) |
| F22 | — | **Open, now Medium.** FX-2 makes the enclosing seal the payer of stacks, and a `{% # P %}[P]` region enters a `Process` record whose owner is P's wallet. A stack in P now draws on P's wallet. A body that reads runner context can therefore pay the runner from P's funds. On FX-2 alone, P3-04's unsubstituted `seals_itself` check is still in place, and no runner-context rule exists (see F30). After the merge with FX-4, the gaps of revision 3 remain: `rho:block:data`, and `rho:deploy:data` through the lookup. | — |
| F23 | — | **Open.** FX-2 supplies the record but does not wire `replace_rev_hook`. The revision 3 proposal stands: replacement admits the authority's quorum only. | — |
| F5, F6 | — | **Open (lane B).** `"transfer"` and `"bond"` still check the running deployment's signers. Inside a deployment that A signs, a contract that M stored runs under M's record, but a transfer from A in that contract still passes the old check. The F6 test plan therefore still shows A paying M. | — |

### Coverage of the A-6 test plans by the lanes' tests

| Plan | Lane tests | Covered | Gap |
|---|---|---|---|
| F2/F14: M stores a stack, A triggers it | `region_spec::a_fired_continuation_draws_on_its_author_not_on_the_firing_region`, `seal_record_spec::a_fired_continuation_does_not_inherit_the_firing_region` | Yes. The author pays the stack, the trigger pays only its own charges, and replay agrees. | — |
| F14: `{% A %}[stack]` in an envelope that A and B sign, deployed by B | `region_spec::a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer`, `seal_record_spec::a_stack_pays_from_its_enclosing_seal_not_the_deployer` | Yes | — |
| F14: a bare stack in a deployment, expected refused | none | Superseded | Decision 51 makes the top level `{% D %}[deployer]`, so a bare stack in a deployment draws on the deployer by ruling. The plan's expectation is out of date. |
| F15: B fires `{% A %}[ {% bundle0{*k} %}[cells] ]` in an envelope that only B signs | `seal_record_spec::a_kind_transfer_draws_on_the_enclosing_seal` | In the reducer only | The gate refuses the nested form (FX-2 open item 1), so no end-to-end test is possible yet. The end-to-end test arrives with FX-5. |
| F17: three blocks, with A storing, B firing and A firing | `region_spec::a_region_sealed_in_its_origin_pays_from_its_sealer_for_any_later_runner`, `seal_record_spec::a_region_sealed_in_its_origin_runs_for_a_later_runner_and_pays_from_its_sealer`, `a_foreign_seal_in_a_later_runners_own_code_is_refused` | Yes, under Decision 59 (4) | — |
| F4: M lifts B's cosignature | `region_spec::a_lifted_cosignature_carries_no_authority_under_another_deployer`, four `casper_message` unit tests | Yes | F27 is a separate defect, and no test covers it. |

### F27 — Medium: the deployer's signature does not cover the cosignature set

**Code:** `DeployData::_to_proto` (`casper_message.rs`) builds the message that the deployer signs. It leaves out `cosignatures`, and FX-3 decision 2 keeps the other cosignatures out of each cosignature's preimage too. The envelope's identity is the deployer's signature, and deduplication keys on it (revision 1, F4).

**Consequence:** any relay or proposer can remove cosignatures from an envelope. The deployer's signature stays valid, the envelope keeps its identity, and the remaining cosignatures still verify. The stripped envelope names a seal whose owner is no longer a signer, so the gate refuses it (`ForeignSignature`). Under the same identity, the deploy is then rejected, and the complete envelope cannot be included afterward. No REV moves. The effect is a deterministic, attributable-looking refusal that any proposer can apply to any cosigned deploy, and a receipt whose identity does not say which cosignature set it judged.

**Proposed fix:** the deployer commits to the cosigner set. Add the list of required cosigner public keys to `DeployData`, inside the message that the deployer signs. Refuse an envelope whose verified cosigners differ from that list. The cosignature preimage already covers the deployer's signature, so it then covers the list too. This is a change to the deploy format and the gate, so it needs a ruling (escalation: authority and cross-lane interface).

**Test plan for lane C:** an envelope from A, cosigned by B, whose term draws on B. A relay removes B's cosignature and submits the result first. *Expected after the fix:* the stripped envelope is refused as malformed, and does not consume the identity. The complete envelope is then admitted. *Today:* the stripped envelope is refused for a foreign signature, and the complete one is a duplicate.

**Note, to verify:** the cosignature now binds the exact bytes of the deployer's signature. If the deployer's signature scheme accepted a second valid encoding of one signature (for example a high-S ECDSA value), a relay could make a second envelope identity from one deploy. `Secp256k1::verify` uses `k256`'s `verify_prehash` on a DER signature. Confirm that it refuses high-S values and non-canonical DER. This concerns deduplication in general, not only FX-3.

### F28 — Medium: Decision 12 turns F19 into a route to another party's seal

**Code:** FX-2 decision 12 checks a sealed region that arrives as data against the origin of the code that evaluates it. F19 (open, FX-5) lets the gate admit a variable seal over a stack body, because it assumes that the variable is a kind.

**Consequence:** M's admitted term can build, at run time, a stack region whose seal variable M binds to A's deployer id, and send it as data. If any code that A stored evaluates data that it receives, the region runs under A's record, passes the check against A's origin, and draws on A's wallet. Decision 12 is the ruling, and A's code chose to evaluate the data. But the gate is the barrier that keeps M from writing A's seal at all, and F19 is a hole in that barrier.

**Proposed fix:** raise F19 to Medium and fix it before FX-2 merges. At the gate, name an unresolved variable seal as unresolved unless it can be proved to be a kind. At run time, refuse a stack-body region whose seal resolves to a wallet name instead of a kind, unless the wallet is a payer element of a compound (FX-2 decision 11).

### F30 — Medium: merging FX-2 with FX-4 can silently drop the F18 and F20 fixes

**Code:** FX-2 is cut from P3-04, not from FX-4. Its `check_region_signature` still uses P3-04's `seals_itself` on unsubstituted forms, with no closedness check and no runner-context rule. FX-4 rewrote the same function (`hash_seal_admissible` on the substituted, closed process, and `held_or_signed` from P3-05). FX-2 rewrote it again (origin signers and the returned owner and kind).

**Consequence:** the two branches conflict in one function. A resolution that keeps FX-2's side reopens F18 and F20. Neither branch's tests fail on that resolution, because FX-2's tests do not use a hash seal with free variables or runner context.

**Proposed fix:** the merge keeps FX-4's hash-seal check, done once on the substituted signature and body, ahead of FX-2's origin check, and returns FX-2's owner and kind. Run FX-4's `hash_seal_spec` and FX-2's `seal_record_spec` together on the merge. A-7 re-audits the merge commit.

### F31 — Medium (to verify): a stack in a `# P` region spends P's wallet outside the funder ledger

**Code:** under FX-2, a stack inside `{% # P %}[P]` draws on `current_owner()`, which is P's process wallet. `journaled_transfers` records it as `Source::Wallet(process_owner)`. `settle_funders` (`settle.rs`) draws funder shares only for charges on a process wallet (`funders::draw`) and for `Source::Share` refusals. It does nothing for a `Source::Wallet` spend from a process wallet. P3-04 decision 21 states that settlement debits a process wallet only for charges.

**Reachable:** yes, by the interim rule. The gate admits an envelope that is exactly `{% P %}[# P]` (P3-04 decision 17), and its static walk names only the payees of a stack's cells. So P may contain a stack.

**Consequence:** the wallet's balance falls while the funders' recorded shares do not. A later refusal of a share by the authority, or a charge drawn pro rata, then works on shares that sum to more than the wallet holds. REV is still conserved, because the transfer itself is balanced. But the per-funder ledger (Decision 50) no longer matches the wallet, and a refusal may fail at settlement. Settlement failing on a recorded block would be a consensus failure. This last step is to verify.

**Proposed fix:** until region metering defines draws from shares (Decision 56), refuse a stack or a native draw whose writer is a `Process` owner. Alternatively, journal such a draw as a pro-rata `Source::Share` set, by Decision 50's rule. Lane C or lane B decides, with region metering.

**Test plan for lane B:** Alice funds `# P` with 3,000 and Carol with 1,000. A self-sealed envelope P contains a stack of 400 cells to Dave. *Expected after the fix:* either the stack is refused, or the shares fall by 300 and 100 and still sum to the wallet balance. A following refusal of Carol's share returns exactly her remaining share, and every block replays to the same settled root.

### What FX-2 and FX-3 get right

- The record is written only by the reducer, on a field that the normalizer cannot produce, and a continuation without a seal keeps its legacy bytes. A golden-root test pins the legacy root.
- A fired continuation never runs under the firing task's record. Stored code keeps its author's authority, and the firing party lends none.
- With records on, code outside every seal can neither pass a region check nor pay a stack. Both refuse.
- The cosignature preimage is domain-separated and length-prefixed. No caller can check a cosignature against the deploy data alone.

### What remains open after FX-1 to FX-4

- F5, F6: lane B's wiring of the record into `rho:rev:wallet`, `rho:rev:bond` and the gate.
- F12, F19 (now Medium, F28): FX-5.
- F22 (now Medium), F23, F24, F25, F26: as in revision 3. F22 and F26 need the native wiring. F23 needs the hook wiring.
- F27, F30, F31: above. F30 is a precondition for merging FX-2 with FX-4.
- FX-2 open item 3 (a merge error in a three-block run with a repeated send) is with lane A2. It is outside authority.

## Rulings of 2026-11-02 and the fixes that follow

Revisions 3 and 4 are accepted.

| Finding | Ruling | Fix | Lane |
|---|---|---|---|
| F27 | The deployer's signed message names the required cosigners (Decision 63). | FX-6 | C |
| F28, F19 | A seal must be written by its owner. A seal in data received at run time is refused. A kind seal taken from a received name is allowed (Decision 62). | FX-6 | C |
| F31 | A stack that draws on a process wallet is recorded as pro-rata share draws (Decision 64). | FX-7 | B |
| F22 to F26, F5, F6 | As proposed in revisions 3 and 4, with the native and gate wiring to the seal record. | FX-7 | B |
| F30 | The merge of the FX branches keeps FX-4's hash-seal check ahead of FX-2's origin check. A-7 checks the merge commit. | FX-8 | A2 |

A-7 re-audits FX-6, FX-7 and FX-8 as each lands.

## Revision 5 (action A-8): re-audit of FX-3, FX-5, FX-2-refunds and FX-7

- **Commits audited:**

  | Branch | Commit | Lane | Base |
  |---|---|---|---|
  | `patch/FX-3` | `d3c5d383a` | C | FX-3 at `df0db3146` |
  | `patch/FX-5` | `c0d48af7f` | C | `patch/FX-6` at `a978756ca` |
  | `patch/FX-2-refunds` | `38c8ca21c` | B | `patch/FX-2` at `4aed4d852` |
  | `patch/FX-7` | `d1c1a5df0` | B | `patch/FX-4` with FX-2 and P3-05 merged in |

- **Method:** a code reading of each fix and of its tests, as in revisions 3 and 4. No tests were run or added.
- **Rulings used:** Decisions 62 to 64 and 66 to 69 (plan r8.6).
  - Decision 67: lane C lists every route by which data received at run time becomes running code, and writes one test for each route. This revision does not analyze those routes. When lane C's tests land, revision 6 records them and checks that each test fails without the `from_data` mark.
  - Decision 68: a stack inside data received at run time is refused, sealed or bare. Kind mints are the one exception.
  - Decision 69: the normalizer injects into a `new` only the URNs that it binds. FX-9 (lane B) will build this.

### Results

| # | Fix | Result | New findings |
|---|---|---|---|
| F27 | FX-3 decisions 6 to 11 | **Closed.** The deployer's signed message names the required cosigners in order. A cosignature counts only for a named key, and each key signs once. `cosignerId:<i>` is the i-th named key. A stripped envelope does not decode, so it does not take the identity. A test confirms that `Secp256k1::verify` refuses a second encoding of one deployer signature (high S, padding, long-form length, trailing byte). | — |
| F12 | FX-5 decision 1 | **Closed.** An envelope whose term does not normalize names nothing and seals nothing, so the deployer alone pays. The test follows the A-6 F12 plan. | — |
| F19 | FX-5 decisions 2 to 4 | **Closed by ruling.** By Decision 62, the gate keeps its kind assumption for a seal over a stack body that it cannot resolve. At run time, a wallet behind such a seal counts only where its owner wrote the seal, and never inside data received at run time (FX-6). The gate now names the payer elements of the nested kind form, so that form passes the gate and draws only on its sealer. The end-to-end test follows the A-6 F15 plan. | F33 (Low) |
| F28 | FX-6, FX-5 | **Closed for the case tested.** Route coverage is left to lane C under Decision 67. | — |
| — | FX-2-refunds decisions 10 to 12 | **No authority finding.** A storage refund is charged inside the record of the region that paid for the item. This moves only metering credit, within one run, and the run's totals do not change. `seal::within_now` is lane C's code, and lane C should review it, as the log says. | — |
| F30 | FX-7 decision 1 (merge `692db527c`) | **Closed on FX-7.** `check_region_signature` substitutes once, checks each `# Q` with `hash_seal_admissible`, and only then checks the other components against the origin's signers. FX-8 must keep this order. | — |
| F22 | FX-7 decisions 3, 5, 6 | **Closed.** The refusal follows the channel that a URN resolves to, not the URN string. Inside a `Process` record, `rho:block:data` and `rho:deploy:data` are refused, also through the registry lookup with a URN built at run time. The test covers both. | — |
| F24 | FX-7 decision 4 | **Closed.** Genesis-only names are refused by channel and by name. Setup asserts that the two tests agree. | — |
| F23 | FX-7 decisions 7, 12 | **Closed.** `replace_rev_hook` and `replaceHook` need the quorum's seal. `# H` cannot replace the hook. | — |
| F25 | FX-7 decision 8 | **Closed.** A genesis test checks that only `PoSV6` names `rho:rev:bondAdmin`, that three terms name `rho:rev:genesis`, and that no blessed term asks the registry lookup. | — |
| F26 | FX-7 decision 12, `seal_wiring_spec` | **Closed.** The F5, F6, F13 and F16 plans now run through the interpreter. | — |
| F31 | FX-7 decision 9 | **Closed by Decision 64.** A casper test shows that a stack in a self-sealed region draws on the funders' shares pro rata, in play and replay. | — |
| F5, F6 | FX-7 decisions 10, 11, 15 | **Closed under a seal record, open outside one.** See F32. | F32 (High) |

### The two checks requested for FX-7

1. **A deploy that is cosigned but not sealed no longer carries its cosigners' authority.** This holds for code that runs under a seal record. The `Deployment` record's owner is the deployer alone. The cosigners are origin signers only, so they count for a region that names them, not for a draw. `wallet_authority` returns the record's owner, and FX-7 decision 15 records the change with a test.

   It does **not** hold for code that runs with no record. See F32. A contract installed at genesis has no stored record, so its body runs with none. A native draw there falls back to the running deployment's deployer and cosigners.

2. **A `deployerId` bound inside `# P` gets the installer's id.** This holds. The URN is a normalizer injection, which is fixed in the term when the installing deployment is normalized. `eval_new` reads it from `New.injections`, not from the running deployment. `hash_seal_spec::a_hash_sealed_region_may_not_read_the_running_deployment` stores Alice's self-sealed P, fires it from Carol's deployment, and checks that the bound name is Alice's id. Because the whole deploy environment is injected today, P's hash also changes with the installer. FX-9 (Decision 69) changes the hash and not the value.

### F32 — High: a native draw outside every seal falls back to the running deployment's signers

**Code:** `SystemProcesses::wallet_authority` (`system_processes.rs`, FX-7) returns the owner of the current seal record. With no record, it returns the running deployment's deployer and verified cosigners. It does not check whether records are on. The reducer's own checks do: `stack_writer` and `region_signers` refuse "outside every seal" when records are on.

**Why there is code with no record on a v6 runtime:** the dispatcher (`dispatch.rs`) runs a fired continuation under the record stored with it. A continuation stored at genesis has no record, because the genesis runtime has records off. `SystemVaultV6.rho` is such a contract. Its `transfer` body calls `rho:rev:wallet` `"transfer"` with the payer id that it receives.

**Consequence:** inside the bodies of genesis contracts, a draw's authority is the running deployment again. This is the execution-context pattern that F6 named and that Decision 51 ruled out. It covers cosigners too, so the first requested check fails on this path. The draw is bounded by what the running deployment's signers hold.

**Proposed fix:**
1. With records on and no record, `wallet_authority` returns an empty authority, as `stack_writer` and `region_signers` already refuse. The fallback to the deploy's signers stays only for runtimes with records off.
2. A genesis contract that moves REV for its caller must not draw by itself. The caller makes the draw in its own code, under its own record, either by calling `rho:rev:wallet` directly or by passing a region that it sealed. `SystemVaultV6.transfer` needs a matching change, or removal if the native process replaces it.
3. Every v6 genesis contract that calls `rho:rev:wallet` or `rho:rev:bond` needs a test that runs with records on.

**Test plan for lane B:** on a v6 shard after genesis, with records on:
- A deployment calls `SystemVaultV6.transfer` for its own deployer. *Expected after the fix:* the call is refused, or done through the caller's own record, as the design decides.
- An envelope cosigned by B calls the vault with B's id. *Expected after the fix:* B's wallet does not change.
- A contract that M stored forwards a payer id that it received to the vault. *Expected after the fix:* no wallet other than M's changes.

### F33 — Low: the gate does not commit a wallet that a variable seal resolves to at run time

**Code:** by FX-5 decision 2, the gate does not name a seal that it cannot resolve over a stack body. If the variable resolves at run time to a wallet that passes the region check (for example, a cosigner of the origin), the stack draws on that wallet. The gate made no commitment for it.

**Effect:** the gate's commitments and the run's draws can name different wallets. Decision 48's journal admission still checks each move against the wallet's balance less what is held, so the draw cannot overdraw. But the gate no longer bounds every paying wallet, which revision 2 proposed (F14, step 5).

**Proposed fix:** record the gap as accepted under Decision 62, or have the executor add any wallet that a run draws on but the gate did not commit to the envelope's settlement check. A ruling is not urgent.

### Merge requirements for FX-8

FX-8 joins branches that now differ in shared code. Revision 6 re-audits the merge commit against this list.

1. **F30:** keep FX-7's order in `check_region_signature`: the hash seal first, on the substituted closed process, then the origin's signers.
2. **FX-3 and the seal record:** FX-2's `Origin.signers` must come from FX-3's `verified_cosigners` on the signed envelope. That gives the named cosigners in the deployer's order, and nothing when a named cosigner is missing.
3. **FX-6 and FX-7:** under a `from_data` record, `wallet_authority` must also refuse, as FX-6's open item 2 asks and Decision 68 implies for draws. FX-7 was cut without FX-6, so its native handlers do not read `from_data`. The F32 fix applies in the same place.
4. **FX-2-refunds and FX-6:** the record kept with each stored item must keep its `from_data` mark.
5. **Tests to run on the merge:**
   - `hash_seal_spec` (F18, F20, F22), `seal_record_spec` and `seal_wiring_spec`.
   - FX-3's cosigner tests and FX-5's `region_spec` tests.
   - FX-6's F28 test, and lane C's route tests under Decision 67 when they land.

## Revision 6 (action A-8): lane C's route tests under Decision 67

- **Commit audited:** `patch/FX-6` at `ead0663ef` (lane C, second commit on FX-6). It adds route tests to `rholang/tests/seal_record_spec.rs`, adds FX-6 decision 6 (`runs_bound_process` in `eval_receive`), and extends the FX-6 decision log.
- **Scope:** by Decision 67, lane C lists the routes and owns their coverage. This revision does not analyze whether the list is complete. It records each test, and it checks by reading that each test fails if the `from_data` mark is removed.
- **Method:** a code reading of the test helper `route` and the eleven tests. No tests were run.

### How each test detects the mark

The helper `route(contract, deliveries)` runs three payloads in turn:
- `sealed`: a stack sealed by `a`, with `a` bound to Alice's deployer id.
- `bare`: a bare stack.
- `mint`: a kind mint.

Alice stores the contract in her own deployment. Bob then delivers the payload in his deployments, along the route that the contract makes. After the deliveries, the helper asserts four things:
- Alice's and Bob's REV balances are unchanged.
- The slot holds no REV.
- For `sealed` and `bare`, at least one error says "inside data".
- For `mint`, there are no errors and the slot holds one unit of the kind.

In every test, the payload runs inside code that Alice stored, so it runs under Alice's stored record. If the mark is removed, the `sealed` payload passes the region check against Alice's origin, and both the `sealed` and the `bare` payloads draw on Alice's wallet. Alice's balance assertion and the refusal assertion then fail. So each test fails without the mark, for both stack payloads. The `mint` case does not depend on the mark. It checks Decision 68's exception.

Two sources set the mark, and the tests depend on them as follows:
- `eval_received` sets it when a received variable or a method result runs in process position.
- FX-6 decision 6 (`runs_bound_process`) stores a continuation under a marked record when the substitution puts a received process into the continuation's stored body.
- The stored record keeps the mark (FX-6 decision 1).

| # | Route (lane C's list) | Test | Mark that the test depends on | Fails without the mark |
|---|---|---|---|---|
| 1 | Drop of a received name | `route_drop_of_a_received_name` | `eval_received` | Yes |
| 2 | A received process variable | `route_a_received_process_variable` | `eval_received` | Yes |
| 3 | A method result run as a process | `route_a_method_result_run_as_a_process` | `eval_received` | Yes |
| 4 | A received value in a stored continuation that fires later | `route_a_received_value_in_a_stored_continuation_that_fires_later` | decision 6, then the stored mark | Yes. Lane C reports that the test failed before decision 6. |
| 5 | The same, in a contract body | `route_a_received_value_in_a_stored_contract_body` | decision 6, then the stored mark | Yes, as 4 |
| 6 | Data that comes back out of the tuplespace | `route_data_that_comes_back_out_of_the_tuplespace` | `eval_received` | Yes |
| 7 | A persistent send | `route_a_persistent_send` | `eval_received` | Yes |
| 8 | A match binding | `route_a_match_binding` | `eval_received` | Yes |
| 9 | A received process inside the owner's own region | `route_a_received_process_inside_the_owners_own_region` | `eval_received`, kept through `enter` | Yes |
| 10 | `new`, `if` and `bundle` bodies | `route_new_if_and_bundle_bodies` | `eval_received` | Yes |
| 11 | A continuation stored by received data, fired later | `route_a_continuation_stored_by_received_data` | `eval_received`, then the stored mark | Yes |

**Result:** all eleven tests detect the mark's removal for both stack payloads. FX-6 open item 1 records that decision 6 marks the whole stored continuation, so it also refuses an owner's own sealed stack in that body. That is a precision limit, not an authority finding.

### Still open

- **FX-6 open item 2** is FX-8 merge requirement 3: `rho:rev:wallet` and `rho:rev:bond` must refuse a draw under a `from_data` record. The route tests check stacks only. When FX-8 lands, the same routes need a native-draw case. The fixing lane writes those tests.
- **F32** (lane B), FX-8 (lane A2) and FX-9 (lane B, Decision 69) have not landed.

## Rulings of 2026-11-02 (afternoon)

Revisions 5 and 6 are accepted.

| Finding | Ruling | Fix | Lane |
|---|---|---|---|
| F32 | Decision 70: P10 holds without exception. With records on, code that has no record has no authority. Fix now. | FX-11 | B |
| F33 | Deferred until after the preview. Journal admission prevents the overdraw. | — | C |
| FX-6 open item 1 | Decision 71: the data mark stays on the whole continuation. A per-value mark is revisited after M2. | — | — |
| FX-8 merge requirements (revision 5) | Sent to lane A2 as part of FX-8b. | FX-8b | A2 |

A-8 re-audits FX-11 and the FX-8b merge commit when each lands. For FX-11, it checks that a native draw with no record returns no authority, and that every v6 genesis contract is tested with records on. For FX-8b, it checks that native draws are refused under the data mark on the routes of revision 6.

## Revision 7 (action A-8): re-audit of the FX-8b merge

- **Branch audited:** `patch/FX-8b` (lane A2, cut from `patch/FX-8`).
  - Merges at `54f1df370` (FX-7), `0dd022adb` (FX-6 at `ead0663ef`), `e9c51c403` (FX-2-refunds), `b5f14ec20` (FX-9), `c79e8b32a` (FX-10) and `12436b644` (P3-01).
  - Post-merge fixes at `13e0abd85`.
  - Suite results at `1c281b7a0`.
- **Method:**
  - A code reading of the merged functions at the tip, against the merge requirements of revision 5.
  - A reading of the native-draw route tests against the routes of revision 6.
  - No tests were run. Lane A2's log reports the named spec files and the full suites passing at `13e0abd85`.
- **Not in scope:** route coverage (Decision 67). FX-11 is not merged yet. Lane A2's log plans it as the last merge.

### Merge requirements of revision 5

| # | Requirement | Result |
|---|---|---|
| 1 | F30: the hash seal first, on the substituted closed process, then the origin's signers | **Holds.** `check_region_signature` at the tip substitutes the signature once. It checks each `# Q` with `hash_seal_admissible` on the substituted body, and only then checks the other components against `region_signers` (with FX-6's data refusal) by `held_or_signed`. FX-8b decision 2. |
| 2 | FX-2's `Origin.signers` from FX-3's `verified_cosigners` | **Holds.** `deployment_record` builds the origin from `deploy_data.deployer_id` and `deploy_data.cosigners`. `DeployData::from_deploy` fills `cosigners` with `Cosignature::verified_cosigners(envelope)`, which gives the named cosigners in the deployer's order. If a named cosigner is missing, the field is empty. Such an envelope does not decode and is refused at the gate (FX-3 decision 9), so play and replay do not meet it. |
| 3 | Native draws refused under the data mark | **Holds.** Under a `from_data` record:<ul><li>`rho:rev:wallet` refuses every command except its reads (`find`, `balance`, `kindBalance`, `grantBalance`, `share`), so `spendCompute` and `replaceHook` are refused too.</li><li>`rho:rev:bond` refuses every command except `balance`.</li><li>`rho:rev:bondAdmin` refuses every command (FX-8b decision 5).</li></ul>The refusal comes before `wallet_authority` is read. |
| 4 | The F32 fix in the same place | **Not on this branch.** `wallet_authority` still returns the deploy's signers when there is no record (F32). The fix is FX-11 (Decision 70), which lane A2 merges last. |
| 5 | The data mark kept on refund records | **Holds.** The storage rule keeps the whole `SealRecord`, with `from_data`, for each item. `storage_rule::tests::a_refund_record_keeps_the_mark_of_data_received_at_run_time` tests this. |
| 6 | The named spec files run on the merged code | **Reported** by lane A2 at `13e0abd85`: `seal_record_spec` 21, `seal_wiring_spec` 7, `hash_seal_spec` 11, `rev_bond_spec`, `rev_genesis_spec`, `signed_region_spec`, `slot_spec`, `storage_left_in_space_spec`, and casper `region_spec` and `gated_executor_spec`. |

### Native draws on the routes of revision 6

`seal_record_spec`'s `route` helper now sends two more payloads along each of the eleven routes:
- A `rho:rev:wallet` transfer from Alice to Bob, named by Alice's deployer id.
- A `rho:rev:bond` bond of Alice's funds.

For both, the helper asserts that the native call answered with a refusal (`(false, _)` on its reply channel). As in revision 6, it also asserts that Alice's, Bob's and the slot's balances are unchanged.

By reading, each of the eleven tests fails if FX-9's refusal is removed. The payload runs in code that Alice stored, under Alice's record with the mark. Without the refusal, `wallet_authority` returns Alice's record owner. The transfer or bond is then admitted, Alice's balance changes, and the reply is `(true, _)`. Lane A2 reports the same result by switching the check off.

`seal_wiring_spec::a_bond_admin_call_inside_data_received_at_run_time_moves_nothing` covers `rho:rev:bondAdmin` under the mark, and the same call in the holder's own code still slashes.

**Result:** native draws are refused under the mark on all eleven routes, with tests from the fixing lanes.

### Other merge decisions read

- **FX-8b decision 1:** FX-5's envelope classification is kept, and FX-7's bare-stack flag is added to each case. An unnormalized term still names nothing, and the deployer alone pays.
- **FX-8b decision 3 and the post-merge fixes:** the FX-3 and FX-7 tests now use FX-3's cosignature API. Draws on a cosigner's wallet sit inside `{% … %}[c]`, as F6's ruling requires.
- **FX-8b decision 4:** the storage files take the refunds side.
- **Removed test:** FX-8's own records-on F20 test was dropped because FX-7 decision 6 admits an author-bound `deployerId` in `# P`. FX-7's `a_hash_sealed_region_may_not_read_the_running_deployment` covers the current rule with records on, so no coverage is lost.
- **Fixture:** the cross-host fixture is written again, because FX-7 and FX-10 change the v6 genesis contracts.

### Note (no severity)

The refusal message that all three native handlers return under the mark is the stack message: "a stack inside data received at run time draws on no wallet". For a native transfer, bond, release or slash, the text names the wrong operation. Lane B can give the native handlers their own message. No test depends on the wording.

### Open after FX-8b

- **F32:** FX-11 (lane B, Decision 70). A-8 checks two things:
  - A native draw with no record returns no authority.
  - Every v6 genesis contract that calls `rho:rev:wallet` or `rho:rev:bond` is tested with records on.
- **The FX-8b commit that merges FX-11:** A-8 re-audits it.
- **F33:** deferred (lane C).

## A-9: conformance audit against design r10 §6.3 (pass 1, `patch/FX-8b` at `1c281b7a0`)

- **Normative forms (Decision 74):**
  - **Two-party (R2/R3):** `{% for(ptrn <- chan){% P %}[t] %}[s1] | {% chan!(Q) %}[s2]`. Each surface pays its own cell at its own price.
  - **One-party (R1):** `{% for(ptrn <- chan){% P %}[t] | chan!(Q) %}[s]`. s pays once.
  - **Continuation:** a continuation not enclosed in its own membrane uses the enclosing seal. That is t = s1 in the two-party form, and t = s in the one-party form.
- **Rulings applied (plan r8.9):**
  - Decision 75: the installer pays for its continuations, from the purse at the surface of its stored code. An unfunded stored receive does not fire.
  - Decision 76: each surface of a COMM pays, and surfaces under one seal pay once.
  - Decision 77: no data mark. Received data runs under the continuation's membrane, and origin verification stays.
- **Source of the forms:** design r10 is not yet in the publications repository or the Drive folder. This pass works from the forms as stated in Decision 74 and the plan, and is re-checked against §6.3 when r10 is published.
- **Method:** a code reading of every path that charges phlo or moves REV, or that decides who may do so. No tests were run. The from_data checks of revisions 6 and 7 are dropped from this audit, because Decision 77 supersedes them.

### Charging paths

| # | Path (code) | What it does today | Form it should implement | Conforms | Pinning test |
|---|---|---|---|---|---|
| C1 | COMM phlo: `executor.rs` `envelope_run`, `comm_phlo = comm_count(deploy_log)`, and `settle.rs` `charges` | Every COMM in the run is counted to the running envelope, and charged at the envelope's price to its paying wallets. This includes a COMM with a receive that another deploy stored. | Two-party: the stored receive's surface (s1, the installer) pays its own cell at its own price, and the running envelope's send surface (s2) pays its own cell. One-party: one charge. | **No (D2).** The stored receiver's surface pays nothing, and the sender pays both cells. | `gated_executor_spec::comms_are_paid_to_the_proposer_in_the_canonical_form` and `settlement_spec::comms_are_paid_to_the_proposer_in_the_canonical_form` pin per-envelope COMM counts. |
| C2 | Continuation work: the per-runtime meter (`processed.cost.cost`, which becomes `compute_phlo`) | The work of a fired stored continuation runs on the firing envelope's meter and is charged to the firing envelope. | The continuation P runs under t and is paid by t. With no own membrane, t is the installer (Decision 75). | **No (D1).** This is FX-2 decision-log row (1), the Decision 59 metering clause. | `region_spec::a_fired_continuation_draws_on_its_author_not_on_the_firing_region` asserts that the firing party pays its own full charge (`paid(&second, &alice)`), which includes the continuation's work. |
| C3 | Explicit inner membrane `{% P %}[t]` (`eval_cost_signed_term` enters `t`) | Authority enters t, but metering does not. P's work stays on the envelope meter. The one exception is C6. | t pays for P. | **No (D4),** except for C6. | None pins t paying. `region_spec::charges_under_the_deployers_signed_term_land_on_the_deployers_wallet` conforms only because t is the deployer. |
| C4 | Paying wallets: `gate.rs` `paying_wallets` and `region.rs` `paying_owners` | If the term names any seal, the named owners pay and the deployer pays only for a bare stack (FX-7 decision 14). Code outside every region is under the implicit `{% D %}` seal, but D does not pay for it. | Every surface pays its own cell. A top-level send or receive is a surface of D. | **No (D7).** | `region_spec::a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` asserts that the deployer's balance is unchanged, although the term has a top-level send under D. |
| C5 | Several paying wallets: `settle.rs` `charges` loops over `paying_wallets` | Each paying wallet is debited the envelope's whole charge, and the proposer and compute credits are added once per wallet. | Each surface pays its own cells. Surfaces under one seal pay once (R1). A compound seal is one seal. | **No (D5).** Two named seals each pay for the other's work, and a compound seal pays k times. | `region_spec::each_component_of_a_compound_pays_the_full_charge` pins the k-times charge. `a_funded_joint_wallet_pays_once_for_its_compound` conforms, because a funded joint wallet pays once. |
| C6 | Self-sealed envelope `{% P %}[# P]` (P3-04 decisions 17 and 21, FX-7 decision 9) | The `# P` wallet pays the envelope's charges, drawn pro rata on the funders' shares. | One-party R1 with s = `# P`. | **Yes,** for the envelope that is exactly `{% P %}[# P]`. | `gated_executor_spec::a_self_sealed_envelope_is_paid_from_its_process_wallet_in_play_and_replay`, `a_stack_in_a_self_sealed_region_draws_on_the_funders_shares_pro_rata` |
| C7 | Storage: `storage_rule.rs`, `charging_rspace.rs` (P2-16, FX-2-refunds) | A run pays for the items it leaves in the space on its own meter. A refund is charged inside the record that paid for the item, but because metering is per envelope (C2, C3), the record does not change who pays. | A stored send or receive is a cell of its surface, and its surface pays for it. | **No,** as part of D1 and D4. The record kept with each item is the input that FX-12 needs. | `storage_rule::tests::a_removed_item_names_the_record_that_paid_for_it`, `v6::exit_spec::storage_charges_do_not_depend_on_the_interleaving` |
| C8 | Gate commitments: `gate.rs` `gate_block`, Delta × phloPrice on `paying_wallets` | It commits only the running envelope's paying wallets. | A stored receive's surface pays from the installer's purse when the COMM fires, in a block the installer may not be in. The gate, or the rule that replaces it for purses, must decide that charge deterministically. | **No (D6).** There is no purse and no rule for an unfunded stored receive. This is Q-31. | None. |
| C9 | Settlement credits: `settle.rs` `charges` | COMM rev goes to the proposer and compute rev to the compute wallet. | Unchanged by §6.3. | Yes, apart from the totals that C1, C2 and C5 change. | `settlement_spec::compute_is_paid_to_the_compute_wallet_and_the_charge_never_passes_delta` |

### Authority paths

| # | Path (code) | What it does today | Form | Conforms | Pinning test |
|---|---|---|---|---|---|
| A1 | Region check: `check_region_signature` (`reduce.rs`) | A key seal must be a signer of the origin envelope (Decisions 52, 53, 63). A `# Q` must equal its substituted, closed body. | The seals s, s1, s2 and t are verified at origin. Decision 77 keeps this. | Yes | `seal_record_spec::a_region_sealed_in_its_origin_runs_for_a_later_runner_and_pays_from_its_sealer`, `a_foreign_seal_in_a_later_runners_own_code_is_refused`, the `hash_seal_spec` F18 tests, `region_spec::a_lifted_cosignature_carries_no_authority_under_another_deployer`, `a_stripped_envelope_does_not_decode_and_the_complete_envelope_runs_after_it` |
| A2 | Implicit top-level seal: `deployment_record` | A deployment runs under `{% D %}` with D as the owner. | The top level is `{% … %}[D]`. | Yes | `region_spec::charges_under_the_deployers_signed_term_land_on_the_deployers_wallet` |
| A3 | Stored continuation: `consume` writes the record, and `dispatch.rs` fires the body under it | The fired body runs under the installer's record, never the firing task's. | A continuation not in its own membrane uses t = s1 (two-party) or t = s (one-party). | Yes, for authority. Metering diverges at C2. | `seal_record_spec::a_stored_continuation_carries_its_authors_record`, `a_fired_continuation_does_not_inherit_the_firing_region` |
| A4 | Explicit inner membrane: `eval_cost_signed_term`, `record.enter` | P runs under t after t's check against the origin. | `{% P %}[t]` | Yes, for authority. Metering diverges at C3. | `seal_record_spec::a_stack_pays_from_its_enclosing_seal_not_the_deployer`, `region_spec::a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` |
| A5 | Received data: FX-6 `eval_received`, `runs_bound_process`, the `from_data` mark. FX-9 and FX-8b native refusals under the mark. | Received data runs under the continuation's record with the mark, and wallet seals, stacks and native moves are refused under it. | Decision 77: received data runs under the continuation's membrane, with no mark. | **No (D3).** FX-13 removes the mark and the refusals. | The eleven `seal_record_spec` route tests, `seal_wiring_spec::a_bond_admin_call_inside_data_received_at_run_time_moves_nothing` and `storage_rule::tests::a_refund_record_keeps_the_mark_of_data_received_at_run_time` pin the superseded rule. FX-13 converts them. |
| A6 | Stack writer: `stack_writer` | The writer is the current record's owner. With records on and no record, the stack is refused. | The enclosing seal pays its own stack cells. | Yes | `seal_record_spec::a_stack_pays_from_its_enclosing_seal_not_the_deployer`, `region_spec::a_fired_continuation_draws_on_its_author_not_on_the_firing_region` (for the stack, not for metering) |
| A7 | Native draws: `wallet_authority` for `rho:rev:wallet` and `rho:rev:bond` | The authority is the record's owner, or the components of a compound owner. With no record, it falls back to the deploy's signers (F32). | The enclosing seal. With records on and no record, no authority (Decision 70). | **No on FX-8b (F32).** FX-11 fixes it. | `seal_wiring_spec::a_transfer_draws_only_on_a_wallet_whose_owner_holds_the_enclosing_seal`, `a_bond_draws_only_on_the_holder_of_the_enclosing_seal` |
| A8 | Kind mint: `mint_stacks` | The authority is the unforgeable kind name in the signature. | Outside §6.3's REV forms. P9 policy. | Yes | `region_spec::a_kind_mint_and_a_kind_transfer_reach_the_settled_root`, `the_nested_kind_form_passes_the_gate_and_draws_only_on_its_sealer` |
| A9 | Gate names: `named_owners`, `ForeignSignature`, `self_sealed_envelope` | The gate refuses an envelope that names a key seal of a non-signer, and names the payer of the nested kind form. | Consistent with origin verification. | Yes, for authority. The commitments diverge at C8. | `region_spec::the_gate_refuses_a_signed_term_that_names_another_signature`, `the_gate_refuses_a_compound_that_a_component_cannot_cover_or_did_not_sign` |
| A10 | Hooks: `spendCompute` (`seal_authorizes`), `replaceHook` (`quorum_authorizes`) | They act under the enclosing seal: the quorum's compound, or `# H` for spending. | The enclosing seal. | Yes | `seal_wiring_spec::the_compute_wallet_spends_under_the_quorum_or_the_installed_hook_and_nothing_it_calls`, `a_hook_is_replaced_only_inside_a_region_the_quorum_sealed` |
| A11 | Funding slots: `held_or_signed` (P3-05) | A slot is drawn by holding its name. | A purse whose authority is a held name. It is a candidate for the located purse of Q-31. | Yes, pending Q-31 | `slot_spec` (4 tests) |

### Divergences

| # | Divergence | Code | Ruling | Fix |
|---|---|---|---|---|
| D1 | Work under an implicit seal of stored code is metered to the firing deployment. | per-runtime meter, `executor.rs` `envelope_run`. FX-2 decision-log row (1). | Decision 75 | FX-12 |
| D2 | Each COMM is charged only to the running envelope, and the stored receiver's surface pays nothing. | `executor.rs` `comm_count`, `settle.rs` `charges` | Decision 76 | FX-12 |
| D3 | The data mark refuses wallet seals, stacks and native moves in received data. | FX-6 `eval_received`, `runs_bound_process`. FX-9 and FX-8b native refusals. | Decision 77 | FX-13 |
| D4 | An explicit inner membrane `{% P %}[t]` does not meter P to t, except in the self-sealed envelope. | `eval_cost_signed_term`, per-envelope meter | Decisions 74, 75 | FX-12 |
| D5 | Every paying wallet is debited the whole envelope charge. Two named seals each pay for both, and a compound pays once per component. | `settle.rs` `charges` | Decision 76 (surfaces under one seal pay once) | FX-12. `each_component_of_a_compound_pays_the_full_charge` must change. |
| D6 | No located purse at a stored receive's surface, and no deterministic rule for a COMM whose surface purse cannot pay. | `gate.rs`, `settle.rs` | Q-31 | FX-12, after the Q-31 ruling |
| D7 | When a term names a seal, the deployer pays nothing for the top-level surface under `{% D %}`. | `paying_owners`, `paying_wallets` | Decisions 74, 76 | FX-12. `a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` must change its deployer assertion. |

D1 and D2 are the two divergences known before this pass. D4, D5 and D7 are new. Together with D1 and D2 they come from one cause: metering is per envelope, and the record that FX-2 built is used for authority but not for metering. D6 is the open question that Q-31 asks. D3 is ruled (Decision 77) and assigned (FX-13).

### Re-runs

A-9 is re-run on FX-12 and FX-13 when they land. For FX-12, the check is one test per elaborated form, plus the sugar cases (the implicit seal, `-o`, a compound, `# P`) elaborated and checked against the form, as the FX-12 row of the plan says.


## Revision 8 (action A-8): re-audit of FX-11

- **Branch audited:** `patch/FX-11` at `1b2d387e1` (lane B, cut from `patch/FX-10`). Commits `54c8ff4e0` (Decision 70) and `1b2d387e1` (Decisions 72 and 73).
- **Method:** a code reading of `system_processes.rs` (`wallet_authority`, `UrnPhase`), `rev_stack/native.rs` (`spendCompute`, `replaceHook`), `PoSV6.rhox`, `SystemVaultV6.rho`, the FX-11 decision log and its tests. No tests were run.
- **Not in scope:** the data-mark message change (FX-11 decision 3), because Decision 77 removes the mark. FX-8b has not merged FX-11 yet. That merge is re-audited when it lands.

### The five checks

| # | Check | Result | Code | Test |
|---|---|---|---|---|
| 1 | A native draw with no record returns no authority | **Holds.** With records on, `wallet_authority` returns an empty list when there is no record (FX-11 decision 1). The flag is one `Arc` that `setup_reducer` shares between the reducer and `UrnPhase`, so the native handlers and the reducer cannot disagree. `rho:rev:wallet` and `rho:rev:bond` read only `wallet_authority`. `spendCompute` and `replaceHook` need `seal::current_owner()` and refuse without one. `rho:rev:bondAdmin` needs no wallet authority: its authority is the name. | `system_processes.rs` `wallet_authority`, `native.rs` | `seal_wiring_spec::code_with_no_record_draws_on_no_wallet_under_seal_records` (it pays with records off and moves nothing with records on) |
| 2 | Every v6 genesis contract that calls `rho:rev:wallet` or `rho:rev:bond` is tested with records on | **Holds.** Two blessed contracts call them.<ul><li>`SystemVaultV6.rho` calls only `find` and `balance` now, because `transfer` is removed.</li><li>`PoSV6.rhox` calls `rho:rev:bond` `balance`, plus `rho:rev:bondAdmin` `release` and `slash`.</li></ul>Both run with records on in the tests listed. `closeBlock` runs in the close-block system deploy on a runtime with records off. Its `bondAdmin` calls are authorized by the name, so the flag does not change them. | `SystemVaultV6.rho`, `PoSV6.rhox` | `system_vault_v6_test` (`the_facade_has_no_transfer`, and the balance and overdraft tests with `set_seal_records(true)`). `genesis_contracts_records_spec::the_v6_vault_facade_finds_and_reads_under_seal_records_and_has_no_transfer`, `every_pos_v6_entry_point_answers_under_seal_records`, `the_v6_utility_contracts_answer_under_seal_records`. `pos_bond_admin_spec` (four tests through `BlockExecutor`, each replayed). |
| 3 | Bond: PoS moves no REV | **Holds.** `PoS!("bond", …)` checks the amount and the key, then appends `(amount, handleCh)` to `pendingBonds`. It calls no `rho:rev` command. `settlePendingBonds` reads `rho:rev:bond` `balance` only. | `PoSV6.rhox`, `bond` and `settlePendingBonds` | `pos_bond_admin_spec::two_pos_bonds_on_one_native_bond_admit_the_first_and_release_the_rest`, `gated_executor_spec::a_user_transfer_and_bond_survive_play_and_replay` |
| 4 | Dropped bonds: released through `rho:rev:bondAdmin` and nothing else | **Holds.** After a key's pending bonds are folded, any native bond above the recorded total (with a withdrawing bond still counted) is released with `RevBondAdmin!("release", (pk, native - total))`. The only REV moves in `PoSV6.rhox` are three `rho:rev:bondAdmin` calls: this release, the release after quarantine, and the slash. The contract has no `rho:rev:wallet` call. | `PoSV6.rhox` lines with `RevBondAdmin!` | `two_pos_bonds_on_one_native_bond_admit_the_first_and_release_the_rest` (500 returns to the wallet), `a_pending_bond_backed_only_by_a_withdrawn_bond_is_dropped` (nothing is released) |
| 5 | Handle: `withdraw`, `commitRandomImage` and `revealRandom` accept only the validator's handle, and only the deploy that bonded can read it | **First part holds, second part fails.** All three look up `handles[handle]` and refuse "Unknown validator handle" otherwise. But the handle goes to the `handleCh` of whoever registered the pending bond, and `bond` identifies the validator from a deployer id passed as data. A party that is not the bonder can therefore receive the handle. See F35. | `PoSV6.rhox` `withdraw`, `commitRandomImage`, `revealRandom`, `bond` | `pos_bond_admin_spec::a_published_deployer_id_does_not_act_for_its_validator` covers the first part. No test covers the second. |

### F35 — High: the PoS bond registration identifies the validator from a deployer id passed as data

**Code:** `PoS!("bond", deployerId, amount, returnCh, handleCh)` takes the key from `getUser!(deployerId, …)`. Any caller that holds a validator's deployer id can register a pending bond for that key, and names the channel that receives the handle. If the native bond covers it, `closeBlock` admits it and sends the handle to that channel. FX-11 lists this as open item 2.

**Why it breaks the ruling:** Decision 73 says that PoS must not identify the validator from a deployer id passed as data. `withdraw`, `commitRandomImage` and `revealRandom` now follow it, but `bond` still identifies the validator that way, and `bond` is where the handle is issued. The handle then confers the rights that Decision 73 meant to protect: withdrawal, commit and reveal. No REV moves to the third party. A withdrawal returns the bond to the validator's own wallet. The harm is control of the validator's withdrawal and of its random-beacon participation.

**Related effect:** a registration that the native bond does not cover is dropped, and any native bond above the recorded total is released at the close. A third-party registration can therefore cause a validator's unregistered native bond to be returned before the validator's own registration arrives. This is griefing only, because the REV goes back to its owner.

**Proposed fix:** issue the handle where the bond is made. The native `rho:rev:bond` `bond` runs under the bonder's own seal (Decisions 70 and 72). It can return a bond receipt, an unforgeable name, to its caller, and `PoS!("bond", …)` takes that receipt in place of a deployer id. Then only the code that moved the REV can register, and the handle goes to a channel that this code chose. This matches lane B's option (C). It also settles open item 1 (custody) for the bonding deploy. A ruling is needed, because it changes PoSV6's interface and the bond flow.

**Test plan for lane B:** Mallory registers a pending bond for Alice's key, using Alice's published deployer id, before Alice registers. *Expected after the fix:* Mallory's registration is refused. Alice's registration is admitted, and her handle reaches only her channel.

### F36 — Medium: user code fired during the close-block system deploy runs unmetered

**Code:** `closeBlock` sends each validator's handle, or a refusal, on the caller's `handleCh`. It runs in the close-block system deploy (`executor.rs`, `compute_state` with `close_block`), on a runtime with records off.

**Consequence:** a continuation that the caller stored on `handleCh` fires inside the system deploy. Its authority is correct, because the dispatcher installs the record stored with the continuation. But its work runs on the system deploy's meter, which no envelope pays. Under Decision 75 the installer pays for its continuation. This is a case of A-9's D1 that the envelope meter does not reach at all. It is also a cost-free path to running user code at every close.

**Proposed fix:** FX-12 meters a fired continuation to its stored record's purse (Decision 75). An unfunded continuation does not fire, also in system deploys. Until then, PoS can answer on a channel that the caller reads with a receive in its own deploy, rather than firing caller code at the close. That is lane B's choice.

### Other open items from the FX-11 log

- **Genesis validators** (open item 3) have no handle, so they cannot withdraw, commit or reveal. This is liveness, not authority. It needs a design answer before M2.
- **Handle custody** (open item 1) is closed for the bonding deploy by the F35 fix. A validator's later deploys still need a way to use the handle. Lane B's options A and C both give one.

### A-9 addendum

F36 adds a divergence to the A-9 table:

| # | Divergence | Code | Ruling | Fix |
|---|---|---|---|---|
| D8 | A continuation fired inside a system deploy (close block, slash) runs on no envelope's meter. | `executor.rs` `compute_state` with `close_block`, and the system-deploy runtime | Decision 75 | FX-12 |


## Open questions for review

1. **F6:** answered on 2026-10-31 with the seal reading (b). Every draw on a wallet must sit inside a region that the wallet's owner sealed. F14 to F17 use this ruling.
2. **F4:** should a cosignature cover the envelope (the deployer key and the deploy signature, or the hash of the whole signed envelope less the cosignatures)? Binding to the deployer key is enough to stop reuse under another deployer. Binding to the deploy data alone is not.
3. **F7:** should release and slashing move behind PoS-held unforgeable names, as §6.9 says, with the system auth token kept only for the system deploys' own calls into PoS?
4. **F14 and F15:** may the payer of a stack come only from the enclosing seal, with the deployer no longer a default? This changes charging and the state root of blocks that hold stack literals.
5. **F17:** may a region that A sealed in A's envelope run later in an envelope that A did not sign? Should the record of a region's origin be a field of `CostSignedTerm` or a wrapper that only the reducer can build?
6. **F16:** should the hook's spending authority be the seal `# H` of the installed contract, the compound seal of the hook authority, or both?
7. **F27:** answered on 2026-11-02 (Decision 63). Should the deployer's signed message name the required cosigner keys, so that a relay cannot strip cosignatures from an envelope without changing its identity?
8. **F31:** answered on 2026-11-02 (Decision 64). Until region metering, should a draw whose writer is a `Process` owner be refused, or journaled as pro-rata share draws?

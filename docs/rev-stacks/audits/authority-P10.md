# A-6: authority audit of patch/P2-integration and patch/P3-04 against P10 and P9

- **Branch audited:** `patch/P2-integration` at `3b5a7e90a` (revision 1). `patch/P3-04` at `45bfb2246`, which includes P3-07b and the `# P` seal rule (revision 2, below).
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

## Open questions for review

1. **F6:** answered on 2026-10-31 with the seal reading (b). Every draw on a wallet must sit inside a region that the wallet's owner sealed. F14 to F17 use this ruling.
2. **F4:** should a cosignature cover the envelope (the deployer key and the deploy signature, or the hash of the whole signed envelope less the cosignatures)? Binding to the deployer key is enough to stop reuse under another deployer. Binding to the deploy data alone is not.
3. **F7:** should release and slashing move behind PoS-held unforgeable names, as §6.9 says, with the system auth token kept only for the system deploys' own calls into PoS?
4. **F14 and F15:** may the payer of a stack come only from the enclosing seal, with the deployer no longer a default? This changes charging and the state root of blocks that hold stack literals.
5. **F17:** may a region that A sealed in A's envelope run later in an envelope that A did not sign? Should the record of a region's origin be a field of `CostSignedTerm` or a wrapper that only the reducer can build?
6. **F16:** should the hook's spending authority be the seal `# H` of the installed contract, the compound seal of the hook authority, or both?

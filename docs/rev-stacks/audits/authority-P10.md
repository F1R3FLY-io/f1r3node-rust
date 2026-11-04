# A-6: authority audit of patch/P2-integration and patch/P3-04 against P10 and P9, with the A-7 re-audit of the fixes

- **Branch audited:** `patch/P2-integration` at `3b5a7e90a` (revision 1). `patch/P3-04` at `45bfb2246`, which includes P3-07b and the `# P` seal rule (revision 2, below). `patch/FX-1` at `8886c8c9a` and `patch/FX-4` at `0fbf80c85` (revision 3, action A-7). `patch/FX-2` at `4aed4d852` and `patch/FX-3` at `df0db3146` (revision 4). `patch/FX-3` at `d3c5d383a`, `patch/FX-5` at `c0d48af7f`, `patch/FX-2-refunds` at `38c8ca21c` and `patch/FX-7` at `d1c1a5df0` (revision 5, action A-8). `patch/FX-6` at `ead0663ef`, route tests (revision 6). `patch/FX-8b` at `1c281b7a0` (revision 7). `patch/FX-11` at `1b2d387e1` (revision 8). `patch/FX-8b` at `c41ccb87b` and `patch/FX-13` at `ecd2b0169` (revision 9). `patch/D76` at `427d72396` (revision 10). `patch/FX-8b` at `e17e3ed32` (revision 11). `patch/FX-12` at `435568f9d`, `patch/D76` at `1502a4e98`, `patch/FX-14` at `c334f9287` and `patch/FX-8b` at `f00a2da3d` (revision 12). `patch/FX-12` at `d7537c771` and `patch/FX-14` at `e5c574656` (revision 13). Design r12 and Decision 88, with no code change (revision 14). `patch/FX-8b` at `aea9b529d` and `patch/FX-14` at `2bfaaf942` (revision 15). `patch/FX-12` at `eab30238f`, `patch/FX-14` at `cc3168032` and `patch/FX-8b` at `4e20959b8` (revision 16). `patch/FX-8b` at `4a74b0e34`, `patch/FX-15-replay` at `b5537b44b` and `patch/FX-15-settle` at `0f1d037f7` (revision 17). `patch/Q-32` at `d3df10dd3`, `patch/FX-15-replay` at `fac9aca5a`, `patch/FX-15-settle` at `dda77b2c5` and `patch/FX-16` at `8d0a837b5` (revision 18). `patch/FX-15` at `fc3a6ca72` (revision 19). Q-33 revision 2 at `1d62fac6f` and `patch/FX-17-epoch` at `214571b2e` (revision 20). `patch/FX-15` at `e30313108` and `patch/FX-16` at `a51dd79fc` (revision 21). `patch/FX-15-charging` at `c345b400d` (revision 22). Q-33 revision 3 at `191f74ccb`, `patch/FX-8b` at `efa511e93`, `patch/FX-17-epoch` at `dcb3ce695`, `patch/FX-16` at `bee4c599f`, `patch/FX-15` at `432dc390f` and `patch/FX-15-close` at `34e6c842d` (revision 23). `patch/FX-15-charging` at `dea48f13d` (revision 24). `patch/FX-8b` at `f7af68ca8` (revision 25).
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
- **Source of the forms:** design r10 §6.3 (`cost-accounting/stall-loop-design-r10.tex`, publications `187a5ab`), "The elaborated forms (normative)" and "The two charges". This pass was first written from Decision 74 before r10 was published, and then checked against r10. See "Correction after r10" below.
- **Method:** a code reading of every path that charges phlo or moves REV, or that decides who may do so. No tests were run. The from_data checks of revisions 6 and 7 are dropped from this audit, because Decision 77 supersedes them.

### Charging paths

| # | Path (code) | What it does today | Form it should implement | Conforms | Pinning test |
|---|---|---|---|---|---|
| C1 | COMM phlo: `executor.rs` `envelope_run`, `comm_phlo = comm_count(deploy_log)`, and `settle.rs` `charges` | Every COMM in the run is counted to the running envelope, and charged at the envelope's price to its paying wallets. This includes a COMM with a receive that another deploy stored. | Two-party: the stored receive's surface (s1, the installer) pays its own cell at its own price, and the running envelope's send surface (s2) pays its own cell. One-party: one charge. | **No (D2).** The stored receiver's surface pays nothing, and the sender pays both cells. | `gated_executor_spec::comms_are_paid_to_the_proposer_in_the_canonical_form` and `settlement_spec::comms_are_paid_to_the_proposer_in_the_canonical_form` pin per-envelope COMM counts. |
| C2 | Continuation work: the per-runtime meter (`processed.cost.cost`, which becomes `compute_phlo`) | The work of a fired stored continuation runs on the firing envelope's meter and is charged to the firing envelope. | The continuation P runs under t and is paid by t. With no own membrane, t is the installer (Decision 75). | **No (D1).** This is FX-2 decision-log row (1), the Decision 59 metering clause. | `region_spec::a_fired_continuation_draws_on_its_author_not_on_the_firing_region` asserts that the firing party pays its own full charge (`paid(&second, &alice)`), which includes the continuation's work. |
| C3 | Explicit inner membrane `{% P %}[t]` (`eval_cost_signed_term` enters `t`) | Authority enters t, but metering does not. P's work stays on the envelope meter. The one exception is C6. | t pays for P. | **No (D4),** except for C6. | None pins t paying. `region_spec::charges_under_the_deployers_signed_term_land_on_the_deployers_wallet` conforms only because t is the deployer. |
| C4 | Paying wallets: `gate.rs` `paying_wallets` and `region.rs` `paying_owners` | If the term names any seal, the named owners pay and the deployer pays only for a bare stack (FX-7 decision 14). Code outside every region is under the implicit `{% D %}` seal, but D does not pay for it. | Every surface pays its own cell. A top-level send or receive is a surface of D. | **No (D7).** | `region_spec::a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` asserts that the deployer's balance is unchanged, although the term has a top-level send under D. |
| C5 | Several paying wallets: `settle.rs` `charges` loops over `paying_wallets` | Each paying wallet is debited the envelope's whole charge, and the proposer and compute credits are added once per wallet. | r10 §6.3: each component of a compound pays the full charge for the compound's work (Rule 2), and a funded joint wallet pays once (Rule 3, Decision 43). A signature pays only for the work under its own region (Decision 49). | **Compound: yes. Separate seals: no (D5).** Two seals named in separate regions each pay for the whole envelope. | `region_spec::each_component_of_a_compound_pays_the_full_charge` and `a_funded_joint_wallet_pays_once_for_its_compound` conform. None pins the separate-seal case. |
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
| D5 | Seals named in separate regions are each debited the whole envelope charge, not the work of their own region. (The compound's n-times charge conforms to r10, see the correction below.) | `settle.rs` `charges` | r10 §6.3, region metering (Decision 49) | FX-12. `each_component_of_a_compound_pays_the_full_charge` stays. |
| D6 | No located purse at a stored receive's surface, and no deterministic rule for a COMM whose surface purse cannot pay. | `gate.rs`, `settle.rs` | Q-31 | FX-12, after the Q-31 ruling |
| D7 | When a term names a seal, the deployer pays nothing for the top-level surface under `{% D %}`. | `paying_owners`, `paying_wallets` | Decisions 74, 76 | FX-12. `a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` must change its deployer assertion. |

D1 and D2 are the two divergences known before this pass. D4, D5 and D7 are new. Together with D1 and D2 they come from one cause: metering is per envelope, and the record that FX-2 built is used for authority but not for metering. D6 is the open question that Q-31 asks. D3 is ruled (Decision 77) and assigned (FX-13).

### Correction after r10

Design r10 was published after this pass was first written. Checked against r10 §6.3:

1. **D5 is narrowed.** r10 says that a compound signature does not split the charge: under `{% P %}[s1 (*) s2]` each component pays the full charge for P's work, and a joint wallet that holds REV pays once. So the compound's n-times charge, and its pinning test `each_component_of_a_compound_pays_the_full_charge`, conform. The first version of this pass listed that test as one that must change, which was wrong. D5 now covers only seals named in separate regions, which each pay for the whole envelope instead of their own region.
2. **The linear transfer `{% for(y <- x) P %}[s1 -o s2]`** is in r10's list: the COMM is charged to s1 and the continuation's compute to s2. The current code charges both to the envelope's paying wallets. This is a case of D4 and needs its own FX-12 test (the FX-12 row already names `-o`).
3. **Price.** r10 says that an explicit region pays at the price recorded with its origin, and that a `# P` region pays at its installer's price. `settle.rs` `charges` uses the running envelope's `phloPrice` for every charge. This is part of D1 and D4. FX-12 must charge a region at `origin.phlo_price`, which the seal record already holds.
4. **Storage** conforms on timing. r10 has no cross-envelope refund, and the storage rule refunds only items stored and consumed within one run. Who pays for a stored item is still D1 and D4 (C7).
5. **D7 is confirmed.** r10 says that a deployment with no explicit wrapper is `{% D %}[deployer]`, and that parsing the source is charged to the deployer. Work outside every region is the deployer's.

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


## Revision 9 (actions A-8 and A-9): the FX-11 merge into FX-8b, and A-9 pass 2 on FX-13

- **Commits audited:**
  - `patch/FX-8b`: merge `2e9339435` (FX-11) and post-merge fix `c41ccb87b`.
  - `patch/FX-13`: commits `6c6ab12d2` (native refusals removed) and `ecd2b0169` (Decision 77). The branch is cut from FX-8b at `1c281b7a0`, before the FX-11 merge.
- **Method:** a code reading, as before. No tests were run.
- **Q-31:** no memo is on any branch or in the plan on Drive. The newest plan, r8.9, still shows Q-31 as "Open: lane B + C proposal".

### The FX-11 merge into FX-8b

| Check | Result |
|---|---|
| `wallet_authority` with records on and no record | **Holds at the FX-8b tip.** The `None if self.urn_phase.seals_recorded() => Vec::new()` branch is present. `setup_reducer` shares the flag between the reducer and `UrnPhase` (`seal_records: urn_phase.seal_records.clone()`). |
| FX-11's tests on the merge | **Present:** `seal_wiring_spec::code_with_no_record_draws_on_no_wallet_under_seal_records`, `genesis_contracts_records_spec` (three tests), `system_vault_v6_test`, `pos_bond_admin_spec` (four tests) and `gated_executor_spec::a_user_transfer_and_bond_survive_play_and_replay`. The post-merge fix `c41ccb87b` makes the three records-on genesis tests read the merged root (`mid_root`) instead of the run's own post-root. This changes where the test reads, and not what it checks. Lane A2 has not yet posted suite results for the tip. |
| F35, F36 | **Unchanged by the merge.** Both are still open. |

### A-9 pass 2: FX-13 against r10 §6.3

| # | Divergence | Status on FX-13 | Test |
|---|---|---|---|
| D3 | The data mark | **Closed.** The reducer no longer marks received data (FX-13 decision 1). `SealRecord.from_data` and proto field 4 are removed, and the field number is reserved (decision 2). The native refusals are removed in `6c6ab12d2` (decision 4). A seal in received data is still checked against the membrane's origin signers, so origin verification stays (decision 3, r10 "The membrane decides"). | The eleven `seal_record_spec` route tests, each with six payloads. A seal naming the membrane's owner runs. A seal naming the sender is refused by the origin check. A bare stack draws on the membrane. A kind mint mints. A wallet transfer and a bond run under the membrane. `region_spec::data_sent_to_an_owners_contract_runs_under_the_owners_membrane_and_is_paid_there`. |
| D1 | Continuation work metered to the firing deployment | **Open.** It is out of FX-13's scope (FX-12). | `region_spec::data_sent_to_an_owners_contract_runs_under_the_owners_membrane_and_is_paid_there` asserts that Mallory, the firing party, pays her whole charge (`paid(&second, &mallory)`), and that charge includes running Alice's continuation. The test's name says "paid there", but only the stack's REV units come from Alice, not the continuation's phlo. FX-12 must change this assertion. |
| D2, D4 to D8 | — | **Open (FX-12).** FX-13 does not change the cost path. | — |

**F33:** FX-13's log argues that located purses (Decision 75) make F33 moot. The ruling stays with Q-31, as the log says. A-9 re-checks F33 when FX-12 lands.

### Merge requirement for FX-13 into FX-8b

FX-13 is cut before the FX-11 merge, and FX-11 decision 3 changed the same refusal sites that FX-13 removes: the per-command messages in `rho:rev:wallet`, `rho:rev:bond` and `rho:rev:bondAdmin`. The merge must remove FX-11's per-command refusals as well, so that no refusal under the removed mark survives. It must keep FX-11's `wallet_authority` no-record branch, which is Decision 70 and not part of the mark. A-9 re-checks the merge commit.


## Revision 10 (action A-9): pass 3 on `patch/D76` (Decision 76, executor side)

- **Branch audited:** `patch/D76` (lane A2, cut from FX-8b at `c41ccb87b`). Commits `33b40ef29` (the change) and `427d72396` (suite results). This is the executor half of FX-12. Decision 75's purse is Q-31 and lane B's, and `surface_purse` is the single place where lane B plugs it in (D76 decision 6).
- **Method:** a code reading of `comm_ledger.rs`, the `charging_rspace` and `storage_rule` changes, `v6/surface.rs`, the `settle.rs` surface charges, and the D76 decision log and its tests. No tests were run. Lane A2 reports all suites passing at `33b40ef29`.
- **Not seen yet:** the FX-12 cost-path commit (lane B), the FX-13 merge into FX-8b, and the Q-31 memo. `D76.md` refers to Q-31, but no memo is on any branch, and no plan newer than r8.9 is on Drive.

### What D76 implements

At each COMM, the storage-rule path records one surface for the receive and one for each matched send. A surface is keyed by its seal: the owner, plus the origin envelope and its price. A COMM counts one cell for each distinct key (decisions 1 and 2). The tally goes into the record entry (field 18) and is checked in replay (decision 4).

Settlement charges surfaces in two ways (decision 5):
- The client's own surfaces are charged at the envelope's price to its paying wallets, against the gate's commitment.
- Each foreign surface (stored code from another envelope) is charged its cells times the origin's price. The draw is on `surface_purse`, which is the seal's owner today.

The gate commits nothing for a foreign surface. A foreign draw is capped at what the purse holds, and a shortfall is neither drawn nor credited (decision 7).

### Divergences after D76

| # | Divergence | Status on D76 | Test |
|---|---|---|---|
| D2 | Each COMM charged only to the running envelope | **Closed for continuations, open for data.** A stored receive's surface now pays one cell at its origin price. A stored send (datum) from an earlier envelope has no record, so its surface is charged as the client's part (D76 gap 1). In the two-party form the sender's seal s2 pays its own cell. See D9. | `region_spec::each_surface_of_a_comm_pays_one_cell_at_its_origin_price` (two-party, receive stored at price 1, send at price 2). `exit_spec::replay_refuses_a_record_whose_surface_attribution_is_forged`. `comm_ledger::surface_tests::each_distinct_surface_of_a_comm_is_counted_once`. |
| D1 | Continuation work metered to the firing deployment | **Open.** D76 attributes COMM cells only. The compute of a fired continuation stays on the firing envelope's meter. | `a_fired_continuation_draws_on_its_author_not_on_the_firing_region` now adds the author's cell, but the firing party still pays the continuation's compute. |
| D4 | An inner membrane does not meter P to t | **Open for compute.** COMM cells under an explicit region in the same envelope count as the client's own surfaces, because `is_own` compares envelopes. So a cosigner's region in the envelope pays nothing of its own. The charge falls on the envelope's paying wallets. | — |
| D5 | Separate seals each pay the whole envelope | **Open.** The client's own cells still go to every paying wallet in full. | — |
| D6 | No located purse, and no deterministic rule for an unfunded stored receive | **Partly open.** The draw is now deterministic, capped at the purse's balance less the block's other debits. But r10 says that a stored receive whose purse cannot fund its cell does not fire. Under D76 decision 7, the COMM fires and the cell goes unpaid. The purse is still the owner's whole wallet, not a purse located at the surface. Both points are Q-31. | — |
| D7 | The deployer does not pay for the top-level surface when a seal is named | **Open.** The own-surface charge goes to `paying_wallets`, which are unchanged. | `a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` (unchanged) |
| D8 | Continuations fired in a system deploy run unmetered | **Open.** D76 changes the user envelopes' settlement only. | — |
| D9 (new) | A stored send carries no seal, so its surface is charged as the client's part when a later envelope consumes it | **Open.** This is D76 gap 1. A datum needs a reducer-built wrapper like the continuation's (FX-2 decision 4). That is lane C's work. | — |

### Elaborated-form tests

| Form or sugar | Test on D76 | Status |
|---|---|---|
| Two-party, receive stored (s1 ≠ s2) | `each_surface_of_a_comm_pays_one_cell_at_its_origin_price` | Present, for the COMM cell |
| Two-party, send stored (datum from an earlier envelope) | — | **Missing.** It cannot pass until D9 is fixed. |
| One-party R1 (one seal) | `surfaces_under_one_seal_pay_once` (bare, and with the receive in `{% … %}[d]`) | Present |
| Implicit seal | covered by the bare case of `surfaces_under_one_seal_pay_once` | Present |
| Compound | `v6::surface::tests` (a compound purse charged once per component) | Unit-level only. It needs an executor-level test. |
| `-o` (COMM to s1, compute to s2) | — | **Missing.** The compute split needs D1 and D4. |
| `# P` | — | **Missing.** By D76 decision 8, a surface draw on a funded process wallet goes pro rata over the shares, but no test covers it. |
| Origin price | `each_surface_of_a_comm_pays_one_cell_at_its_origin_price` | Present, for COMM cells. Compute at the origin price waits for D1 and D4. |

### F37 — Medium: an installer's whole wallet pays every surface of its stored code

**Code:** `surface_purse` returns the seal's owner, a whole wallet, until lane B's Q-31 purse replaces it (D76 decision 6). Any envelope that fires a stored receive makes the installer's wallet pay one cell at the installer's origin price, with no commitment from the gate (decision 7).

**Effect:** this is Decision 75 as ruled ("the installer pays for its continuations"). But r10 bounds the exposure by the purse located at the surface, and today the bound is the whole wallet. A party that fires a stored receive repeatedly moves the installer's REV to the proposer one cell at a time, up to the wallet's balance. No REV goes to the firing party. Decision 78 already holds the branch back until the forms are implemented end to end, and the Q-31 purse is part of that.

**Proposed fix:** none beyond Q-31. This finding records that `patch/D76` must not merge into a shipped line before lane B's purse replaces `surface_purse`.

### Question for the ruling on Q-31

D76 decision 2 keys a surface by owner and origin. So one owner's stored receive (origin E1) meeting the same owner's later send (origin E2) counts as two seals and pays two cells, each at its own price. r10 §6.3 says that a COMM whose surfaces lie under one seal costs that seal once. Does "one seal" mean one owner, or one owner at one origin? The answer decides whether D76's key is correct. It also decides the price when the origins differ.


## Rulings of 2026-11-03 (design r11, plan r8.10)

A wallet is a wrapper around a purse, and the two are one idea. r10's "purse located at the surface" did not mean a separate purse on each channel.

| Item | Ruling | Effect on this audit | Fix |
|---|---|---|---|
| Q-31, D6 | Decision 79. A surface draws on the wallet at its seal, located by the seal record. A stored receive draws on the wallet of the owner in its stored record, at that record's origin price. A send draws on the wallet of the running record's owner. There is no `fundPurse` or `drainPurse`: an installer limits its exposure by installing under a separate seal or `# P`. An unfunded stored receive is not a match candidate: its cell is tested against its wallet's pre-state balance less what the block holds. Replay re-checks every logged COMM. Draws are still checked at the close of the run (Decision 48) and across the block. | D6 is closed by ruling. F37 is closed by ruling: the whole wallet at the seal is the purse, by design. D76 decision 7 (an unfunded COMM fires and its cell goes unpaid) contradicts Decision 79, so FX-12 must replace it with the matcher filter. | FX-12 |
| Question 9 | Decision 80. One cell only when the two surfaces share a membrane: the same owner and the same origin. Two separately sealed terms pay two cells, even under one signer. | D76 decision 2's key (owner and origin) conforms. | — |
| D76 decision 3 | Decision 81. A surface with no record (genesis code) is charged to the client whose run fired it. | D76 decision 3 conforms. | — |
| F35 | Decision 82. `rho:rev:bond` returns an unforgeable bond receipt, and PoS `bond` takes the receipt instead of a deployer id. | F35 is assigned. | FX-14 (lane B) |
| F36, D8 | Goes to FX-12 as D8. | — | FX-12 |

A-9 re-runs on FX-12 with three added checks:
1. the matcher filter, which keeps an unfunded stored receive out of the match candidates;
2. replay re-checking each logged COMM;
3. a datum stored in an earlier block charged to its sender (D9).

A-8 re-audits FX-14 and the merge of FX-13 into FX-8b.

## Revision 11 (action A-8): the FX-13 merge into FX-8b, and design r11

- **Commit audited:** `patch/FX-8b` merge `e17e3ed32`, with parents `8947dc111` (FX-8b with FX-11) and `ecd2b0169` (FX-13).
- **Method:** a code reading of the merged files. No tests were run, and lane A2 has not yet posted suite results for this merge.

| Check | Result |
|---|---|
| No refusal under the removed data mark survives, FX-11's per-command refusals included | **Holds.** At `e17e3ed32` the tree has no `from_data`, `received_at_run_time`, `RECEIVED_AT_RUN_TIME`, `eval_received` or `runs_bound_process`, and no "moves no REV" or "data received at run time" message in `rholang/src` or `casper/src`. `native::READS` and `native::refusal` are gone. The only `refusal` left in `system_processes.rs` is the URN filter (`UrnPhase::refusal`). `SealRecordProto` field 4 is `reserved`. No test refers to `from_data`. |
| `wallet_authority` keeps its no-record branch (Decision 70) | **Holds.** `None if self.urn_phase.seals_recorded() => Vec::new()` is present. |

**Result:** the merge meets both requirements of revision 9. D3 is closed on the integration branch.

### Design r11

Design r11 is published (publications `8c81189`). Its §6.3 differs from r10's in three consequences, which are Decisions 79 to 81:
- The purse is the wallet at the seal.
- An unfunded stored receive is not a match candidate, and replay re-checks every logged COMM.
- One membrane means one cell, for the same owner and origin, and a surface with no record is charged to the firing client.

The rest of §6.3 is unchanged. From the next pass on, A-9 reads against r11. None of the A-9 rows changes because of r11: D6 was already closed by Decision 79, and the matcher-filter and replay checks are the added FX-12 checks.


## Revision 12 (actions A-8 and A-9): A-9 pass 4 on FX-12 and D79, and the FX-14 re-audit

- **Commits audited:**
  - `patch/FX-12` at `435568f9d`: metering commit `aa47795d4` and the FX-13 merge `e51906722`. The branch is cut from D76 `427d72396`.
  - `patch/D76` at `1502a4e98` (lane C, D79). The branch is rebased onto FX-8b `e17e3ed32`.
  - `patch/FX-14` at `c334f9287`. The branch is cut from FX-12.
  - `patch/FX-8b` at `f00a2da3d` (suite results after the FX-13 merge, no code change).
- **Reference:** design r11 §6.3 and Decisions 75 to 82.
- **Method:** a code reading of the commits and the decision logs. No tests were run.
- **Branch state:** FX-12 and D79 do not contain each other. Their merge base is `8947dc111`. FX-14 is on FX-12, so FX-14 does not contain D79. The checks below that depend on D79 hold on `patch/D76` only, until lane A2 merges the branches.

### Divergences after FX-12 and D79

| Divergence | Status | Closed by | Pinning test |
|---|---|---|---|
| D1: continuation work metered to the firing deployment | **Closed** on FX-12 | FX-12 decision 1: `CostManager::charge` records each charge under `seal::current()` | `forms_spec::the_two_party_form_charges_each_surface_and_the_installer_for_its_continuation`, `region_spec::data_sent_to_an_owners_contract_runs_under_the_owners_membrane_and_is_paid_there` |
| D2: stored receives | Closed (D76, revision 10) | — | — |
| D4: inner membrane not metered | **Closed** on FX-12 | FX-12 decisions 1 and 3 | `forms_spec::the_lollipop_charges_the_comm_to_s1_and_the_continuation_to_s2` |
| D5: separate seals each pay the whole envelope | **Closed** on FX-12 | FX-12 decision 3 | `forms_spec::separately_named_regions_pay_only_their_own_work` |
| D6 | Closed by ruling (Decision 79). The implementation is D79 decision 9. | — | see check (a) |
| D7: the deployer pays nothing for the top-level surface | **Closed** on FX-12 | FX-12 decision 4 | the three changed `region_spec` tests in the FX-12 log |
| D8: system-deploy continuations unmetered | **Open** | FX-12 open item 1 gives a proposal | none |
| D9: a stored send has no record | **Closed** on `patch/D76` only | D79 decisions 6 to 8 | `region_spec::a_datum_stored_in_an_earlier_block_is_charged_to_its_sender` |

### Checks (a) to (g)

| Check | Result |
|---|---|
| (a) The matcher filter (Decision 79) replaces D76 decision 7 | **Holds on `patch/D76`, with a gap (F40).** `Match::admits_waiting` runs in `extract_first_match` for each waiting continuation. `EnvelopeFunding` funds a record of the running envelope without a test. For another envelope's record, each wallet of `purse()` must hold `price()` at the pre-state, less the gate table. `BlockPurses` reads the pre-state once for each block and shares the reads. The test fails when the hook returns `true` (D79 verification). D76's cap stays at settlement (D79 decision 12), as Decision 79 keeps it. The test covers one cell only. It does not cover the continuation's work (F40). |
| (b) Replay re-checks every logged COMM | **Holds on `patch/D76` for stored receives.** Replay of a send goes through the same `extract_first_match`, with the oracle installed in `replay_one` from the recorded gate. `an_unfunded_stored_receive_is_not_a_match_candidate_in_play_or_replay` includes a replay that gives the funded run's log with the held block's gate, and that replay fails. On the consume path the stored side is data, so there is no candidate to filter (D79 decision 9). |
| (c) A datum stored in an earlier block is charged to its sender | **Holds on `patch/D76`.** `eval_send` writes the record on the datum (`ListParWithRandom` field 3) when records are on. At a COMM the record on the datum decides the payer, before the storage rule. A datum without a record keeps the legacy bytes. Native acks, contract calls and cells that the REV layer writes carry no record, so Decision 81 applies to them. |
| (d) One cell only for the same owner and origin (Decision 80) | **Holds on `patch/D76`, with one open point.** `SealRecord::shares_membrane` compares the owner and the origin. `comm_cells` gives one key for each distinct membrane, and a unit test covers it. The key is D76's `Surface` (owner, envelope, price). FX-12 keys its work tally with the same `Surface::of`, so the two tallies agree. Open point: a side with no record and the client's own record are two keys. A COMM between the client's send and genesis code therefore counts two cells (D79 decision 5). See question 10. |
| (e) A surface with no record is charged to the firing client (Decision 81) | **Holds** on both branches. `comm_cells` maps a side with no record to the key `None`. FX-12's `own_payers` charges `None` to the deployer, or to every committed wallet when the gate did not commit the deployer. The same fallback also takes keys that do have a record (F39). |
| (f) The elaborated-form test table | See the table below. |
| (g) D8 metered | **Open.** FX-12 open item 1 proposes a close-runtime tally, a charge after the close, and a record for replay. That proposal touches the close block, so it needs a ruling before code. |

### Elaborated-form tests

| Form | Test | Status |
|---|---|---|
| Two-party, stored receive | `forms_spec::the_two_party_form_charges_each_surface_and_the_installer_for_its_continuation` (cells, work and both prices, sugar compared) | Present (FX-12) |
| Two-party, stored send | `region_spec::a_datum_stored_in_an_earlier_block_is_charged_to_its_sender` | Present on `patch/D76` only |
| One-party | `forms_spec::the_one_party_form_pays_once_for_a_redex_under_one_seal` | Present (FX-12) |
| `-o` | `forms_spec::the_lollipop_charges_the_comm_to_s1_and_the_continuation_to_s2` (explicit layers compared) | Present (FX-12) |
| Compound, at executor level | `region_spec::each_component_of_a_compound_pays_the_full_charge` and `a_funded_joint_wallet_pays_once_for_its_compound`, both through `EnvelopeExecutor` | Present |
| `# P` | `gated_executor_spec::a_stack_in_a_self_sealed_region_draws_on_the_funders_shares_pro_rata` | Present for the draw. No test compares the `# P` sugar with its elaborated form. |
| Origin price for compute | the two-party test above: Bob's work is charged at price 1 while Alice's envelope is at price 2 | Present (FX-12) |

### F38 — Low: a refund is keyed to the record that runs when the refund occurs

**Code:** `CostManager::charge` records a refund (a negative charge) under `seal::current()`. `take_work` drops a key whose net work is not positive.

**Effect:** if a charge occurs under one record and its refund occurs under another, the first key pays the full charge. The second key goes to zero or below and is dropped. The sum of the keys can then be more than the run's net work. Also, the `Exhausted` test in `executor::entries` adds the envelope's own cells to `run.compute_phlo`, which includes foreign work. The two terms come from different meters.

**Test plan:** a run in which a stored item is written under one seal and removed under another seal. The sum of the work keys must equal the run's net compute.

**Proposed fix:** key a refund to the record of the charge that it reverses, or net the refunds per run before the split. Use the own work in the `Exhausted` test. Reversible, lane B.

### F39 — Low: an own key whose owner the gate did not commit is charged to the client

**Code:** `settle::own_payers` charges a key to its owner only when the gate committed the owner's wallet, or every component of a compound. Otherwise it charges the client, as for a key with no record.

**Effect:** Decision 81 covers a surface with no record. A region with a record whose owner the gate did not commit is a different case, but it is charged to the client. F33 reaches this branch: a variable seal that resolves at run time to a cosigner draws on the cosigner (bounded by journal admission), and its work and cells are charged to the deployer. So F33 is not moot under located purses. It now also has a charging effect.

**Test plan:** a test that lists which keys reach the fallback, and the payer of each.

**Proposed fix:** a ruling on whether such a key is charged to the client, refused at the gate, or added to the settlement check (F33's proposed fix). Escalated: this touches charging.

### F40 — Medium: the matcher filter funds one cell, and the continuation's work can go unpaid

**Code:** `BlockPurses::funds_cell` admits a stored receive when each wallet of its purse holds the record's price for one cell. FX-12 then charges the continuation's cells and work to the same purse at settlement, capped by D76 decision 7.

**Effect:** a purse that holds one cell at the pre-state can fire a continuation whose work costs more. The draw is capped, and the shortfall is charged to no wallet. The work still counts against the firing client's phlo limit (FX-12 open item 4), but the client does not pay for it. The validator does the shortfall without payment.

**Test plan:** a stored receive whose purse holds exactly one cell, fired by a send from another envelope, with a continuation that does work. The settlement must show who pays the work, and replay must give the same result.

**Proposed fix:** a ruling. The options are: the shortfall goes to the firing client, the continuation stops when its purse is empty, or the filter tests a bound on the work. Escalated: this touches charging and Decision 79.

### FX-14 re-audit (Decision 82, F35)

| Check | Result |
|---|---|
| `rho:rev:bond` returns an unforgeable receipt to its caller | **Holds.** A successful `bond` answers `(true, receipt)`. The receipt is a `GPrivate` whose identifier is the domain tag, the key and the hash of the call's random state. A term cannot write a `GPrivate`, and a name from `new` cannot carry the domain tag. The random state makes the receipt the same in play and replay. The bond process issues a receipt only after the FX-7 seal check. |
| PoS bond takes the receipt and no deployer id | **Holds.** `PoS!("bond", receipt, amount, returnCh, handleCh)` reads the key through `RevBondAdmin!("receipt", ...)`. `getUser` is removed, and no PoS entry point reads a deployer id. `receipt_key` refuses a name without the domain tag and a key part of length zero. |
| F35's test plan is covered | **Holds.** `pos_bond_admin_spec::a_published_deployer_id_does_not_act_for_its_validator` refuses Mallory's call with Alice's deployer id, and the close sends Mallory no handle. `rev_bond_spec::a_deployer_bonds_its_own_rev_natively` checks the receipt's key and refuses a value that is not a receipt. `genesis_contracts_records_spec` refuses a deployer id. `gated_executor_spec::a_user_transfer_and_bond_survive_play_and_replay` uses the receipt in play and replay. |

**Note (no finding):** a receipt is not spent (FX-14 decision 4), and it can be passed on. A holder that is not the bonder can register for the bonder's key and get the handle. This is the capability reading of Decision 82: the bonder decides who holds the receipt. Each registration still passes the native-bond check at the close (Decision 72). If Decision 82 means that only the bonding deploy can get the handle, the receipt must be spent on use or bound to the bonding deploy.

### Merge requirements for FX-12, D79 and FX-14 into FX-8b

1. **Gate table:** D79's `BlockPurses` subtracts the gate table. FX-12 decision 4 adds the deployer to the paying wallets of more envelopes, so the held amounts change. Run D79's two `region_spec` tests and the grant test after the merge.
2. **`comm_ledger.rs`:** keep D79's `comm_cells` call site and FX-12's work map. `reset` must clear both.
3. **`executor.rs`:** `run` and `replay_one` gain the work tally (FX-12) and the funding oracle (D79). Keep both, and keep `set_funding(None)` before the runtime goes back to the pool.
4. **Cross-host fixture:** both branches rewrite `casper/tests/fixtures/v6/cross_host`. Do not take either side. Write the fixture again after the merge with `write_cross_host_fixture`.
5. **FX-14 after D79:** FX-14 and D79 both change `gated_executor_spec.rs`. Merge FX-14 after FX-12 and D79.
6. **D79 open point 4:** a self-sealed envelope with a `for` form is refused as `ForeignSignature`. The owner of `hash_seal_admissible` must check this before the merge, because the funding tests use the `contract` form only.
7. **Suite results:** the FX-12 and FX-14 logs post no suite results. Lane A2 must post them for the merge.

A-8 re-audits the merge commit against this list. A-9 pass 5 runs on the merged branch, and on D8 when its ruling lands.


## Revision 13 (action A-9): pass 5 on the FX-12 D8 commit, and the FX-12 merge into FX-14

- **Commits audited:** `patch/FX-12` at `d7537c771` (D8). `patch/FX-14` merge `e5c574656`, with parents `c334f9287` (FX-14) and `d7537c771` (FX-12).
- **Branch state:** lane A2 has not yet merged FX-12, D79 or FX-14 into FX-8b. `patch/FX-8b` is still at `f00a2da3d`, and `patch/D76` is still at `1502a4e98`. The merge re-audit against revision 12's requirements waits for that merge.
- **Method:** a code reading of the commits and the decision log. No tests were run.

### The FX-14 merge

The merge is clean. Its combined diff is empty, so each file comes from one parent. FX-14 now contains the D8 commit. It still does not contain D79.

### D8: work fired inside a system deploy

**What the commit does:** `eval_system_deploy` clears the run's ledger before the system source runs. After the system deploy, `charge_recorded_work` charges the work of each recorded key to the key's owner, at the key's origin price. The charge is made in the same runtime and before the checkpoint. Each draw goes to the compute wallet. A compound draws on each component in full, and a wallet with funders draws on them pro rata. A draw is capped at the owner's balance. Work with no record is not charged. The post-state root includes the charge, so replay checks it.

| Check | Result |
|---|---|
| Work fired in the close is charged to the record that runs it (Decision 75) | **Holds.** The key is `Surface::of` the running record, as in the envelope runs. |
| The price is the origin price | **Holds.** The charge uses `surface.phlo_price`. |
| Replay | **Holds by construction.** Play and replay both run `eval_system_deploy`, and the charge is a state change inside it. |
| Conservation | **Holds by reading.** Each draw moves REV from the payer to the compute wallet, and a draw never exceeds the payer's balance. |
| The test pins the charge | **Partly.** `a_continuation_fired_inside_the_close_is_paid_by_its_installer` asserts that the installer pays something in the close, and that the compute wallet gains the same amount. It does not assert that the amount equals the continuation's work at the origin price. No test covers the cap, a compound owner or a wallet with funders on this path. |

**D8 status: closed for work on `patch/FX-12`, with two residues.**
1. **Cells:** a COMM fired in the close pays no cell (FX-12 decision 8). r11 §6.3 charges each surface of a COMM one cell. The residue is small, because the close fires only the continuations that PoS answers. It stays recorded as a divergence (D8b) until a ruling says that the close is exempt.
2. **The cap:** a payer that cannot cover the work pays its balance, and the rest goes unpaid. This is the F40 shortfall on a second path. The D79 matcher filter does not apply here, because no funding oracle is installed for a system deploy. F40's ruling must cover both paths.

### F41 — Low: the close-time charge has its own purse rule

**Code:** `compute::charge_recorded_work` maps a compound owner to its components inline. D79 decision 1 makes `seal::purse_of` the one rule, and D76's `surface_purse` calls it.

**Effect:** none today, because the two rules give the same purse. After the merge, a change to `purse_of` would not reach the close-time charge.

**Proposed fix:** call `purse_of` in `charge_recorded_work` when FX-12 and D79 are merged. Reversible, lane B or lane A2.

### Test plan for D8

1. A continuation fired in the close with known work. Assert that the payer's draw equals that work times the origin price.
2. A payer whose balance is less than the work. Assert the draw is the balance and that replay gives the same root.
3. A compound owner and a wallet with funders on the close path.

### Added merge requirements

8. **F41:** `charge_recorded_work` calls `seal::purse_of`.
9. **The funding oracle in the close:** decide with F40 whether the D79 oracle is installed for system deploys. Until then, record that the close has no match filter.
10. **The FX-12 open list:** items 1 and 2 of the log are done on `patch/D76` (D79). Update the log at the merge.

### Divergence status after pass 5

| Divergence | Status |
|---|---|
| D1, D4, D5, D7 | Closed on FX-12 (revision 12) |
| D2, D3, D6 | Closed |
| D8 | Closed for work on FX-12. D8b (cells in the close) is open. |
| D9 | Closed on `patch/D76` only |

Findings F38 to F41 and questions 10 to 13 are open.


## Revision 14 (actions A-8 and A-9): design r12 and the blocker gate (Decision 88)

- **Reference:** design r12 §6.3 (publications `137ffca`), Decisions 83 to 88, plan r8.12.
- **Branch state:** no change since revision 13. `patch/FX-8b` is at `f00a2da3d`, `patch/FX-12` at `d7537c771`, `patch/D76` at `1502a4e98` and `patch/FX-14` at `e5c574656`. No `patch/FX-15` exists yet.
- **Method:** a reading of r12 against the code that revisions 12 and 13 audited. No new code was read, and no tests were run.

### The gate

Decision 88 lets only two kinds of finding block the handoff:
- a divergence from design r12 §6.3.
- an authority or safety finding of High severity or worse.

Every other finding goes on the known-issues list that ships with the branch. From this revision on, each finding has a label, **blocking** or **known issue**, and a reason. A-9 reads only against r12 §6.3. A-8 reports only authority and safety findings of High severity or worse as blocking.

### What r12 changes in §6.3

r12 adds one consequence: **work is gated, not only the cell** (Decision 86). Each step under a membrane t is admitted against the wallet at t. The available amount is the pre-state balance, less what the block holds, less what t has already drawn in the run. When the wallet cannot pay, work under t stops. No step runs unpaid, and no shortfall goes to another party. Q-32 decides how play and replay stop at the same point.

The other consequences are unchanged from r11.

### Rulings that answer open questions

| Question | Ruling | Effect on the audit |
|---|---|---|
| 10: two cells for the client's send and genesis code | Decision 85: two cells, both charged to the client | D79 decision 5 conforms. The question is closed. |
| 11: who pays the F40 shortfall | Decision 86: nobody, because work under t stops | F40 is now a §6.3 divergence. |
| 12: an own key whose owner the gate did not commit | Decision 83: that wallet pays, admitted like any surface draw | F39 is now a §6.3 divergence. F33 is closed by ruling and folds into F39. |
| 13: cells in the close (D8b) | Not ruled | D8b stays a divergence from "each surface pays its own cell". |

### The gate applied to the open items

| Item | Label | Reason |
|---|---|---|
| **F40** (revision 12): the matcher filter admits one cell, and work after the cell is capped at settlement. The shortfall is unpaid, and foreign work counts against the firing client's phlo limit (FX-12 open item 3). | **Blocking** | It diverges from the r12 §6.3 consequence "work is gated, not only the cell": a step runs unpaid, and the client's limit carries work under another membrane. The fix depends on Q-32. |
| **F40, close path** (revision 13): `charge_recorded_work` caps the close-time charge at the payer's balance, and no funding oracle is installed for system deploys. | **Blocking** | The same §6.3 consequence. The fix for the envelope runs must also cover the close. |
| **F39** (revision 12): `own_payers` charges an own key to the client when the gate did not commit its owner. | **Blocking** | It diverges from §6.3 "purses are located by the seal" and "no shortfall is passed to another party". Decision 83 gives the fix: that wallet pays, admitted like any surface draw. |
| **D8b** (revision 13): a COMM that the close fires pays no cell. | **Blocking** until question 13 is ruled | It diverges from §6.3 "each surface pays its own cell". A ruling that the close is exempt would move it to the known-issues list. |
| **D9** (revision 12): a stored send has no record on FX-12 and FX-14. | **Blocking** until the merge | It is closed on `patch/D76` by D79. It stays blocking only if the merge into FX-8b does not take D79. |
| **F38** (revision 12): a refund is keyed to the record that runs when the refund occurs, a key whose net work is not positive is dropped, and the `Exhausted` test mixes two meters. | **Known issue** | §6.3 keys each charge to the membrane that runs it, and the code does that. It does not define refunds. The effect is a possible overcharge of one key, with no authority or safety effect. The step-by-step gate of Decision 86 will read this meter, so lane B should fix the keying with F40. |
| **F41** (revision 13): the close-time charge has its own purse rule. | **Known issue** | No effect today. The two rules give the same purse. Merge requirement 8 asks for one rule. |

### FX-14 against Decisions 84 and 87

The FX-14 follow-ups have not landed. Two items in the current FX-14 differ from the rulings.

**F42 — Medium, known issue: a bond receipt is not spent on use.** FX-14 decision 4 keeps the receipt valid after use. Decision 84 allows one registration and one handle for each receipt. A receipt that reaches other code after its use can make a second registration for the same key. Each registration still passes the native-bond check at the close, and a key that is already bonded is refused.
- **Reason for the label:** the finding is about authority and is Medium, below the gate. Only code that the bonder gave the receipt to can use it. It is not a §6.3 item.
- **Change of label:** if lane B's tests show that one close can confirm two registrations for one key, each with a handle, the finding becomes High and blocking.

**F43 — Medium, known issue: the handle goes to a channel that the caller chooses.** `PoS!("bond", receipt, amount, returnCh, handleCh)` sends the handle to `handleCh`. Decision 87 holds the handle on a channel located at the validator's seal, readable only by code under that seal. Genesis must also place the handles of the genesis validators there.
- **Reason for the label:** today only the receipt holder chooses `handleCh`, and the receipt goes only to the bonder. So the finding is Medium, below the gate. It is not a §6.3 item.
- **Change of label:** the label changes if FX-15 or the follow-up lets code under another seal read a handle.

### Watch list

- **Merges into FX-8b:** revision 12 requirements 1 to 7 and revision 13 requirements 8 to 10.
- **FX-14 follow-ups:** they close F42 and F43.
- **FX-15:** A-9 reads it against r12 §6.3. If FX-15 is the Decision 86 work gate, check four things:
  - Each step under t is admitted against the wallet at t, less the block's holds, less what t has drawn.
  - Work stops when that wallet cannot pay.
  - The close path uses the same gate.
  - Play and replay stop at the same point, by the Q-32 answer.

Q-32 goes through the lane C and lane A2 memo, and A-9 reads the answer when it is ruled.


## Revision 15 (actions A-8 and A-9): the merges into FX-8b, and the FX-14 follow-up

- **Commits audited:**
  - `patch/FX-8b` at `aea9b529d`, with three merges: D76 `2102621ec` (D76 at `1502a4e98`, with D79), FX-12 `20d675e94` (FX-12 at `d7537c771`) and FX-14 `44e9c1766` (FX-14 at `e5c574656`). The last commit writes the cross-host fixture again and moves `admits_waiting` above the guard's doc comment.
  - `patch/FX-14` at `2bfaaf942`, the follow-up for Decisions 84 and 87. It is not yet merged into FX-8b.
- **Reference:** design r12 §6.3, Decisions 83 to 88, plan r8.12.
- **Method:** a code reading of the merged tree and the follow-up, and the merge log in `FX-8b.md`. No tests were run.
- **Not audited:** `patch/Q-32` (`9eac37153`), the Q-32 memo. It goes through lane C and lane A2. No `patch/FX-15` exists.

### The merges into FX-8b against the merge requirements

| Requirement | Result |
|---|---|
| 1. Gate table and `BlockPurses` | **Holds by the log.** Lane A2 reports that D79's two `region_spec` tests and the grant test pass. Both tests are in the merged `region_spec.rs`. |
| 2. `comm_ledger.rs` | **Holds.** `comm_cells` and the work map are both present. `reset` clears the surfaces and the work. `CostManager::charge` calls `on_work`. |
| 3. `executor.rs` | **Holds.** `run` and `replay_one` take the work tally and install the funding oracle. `set_funding(None)` runs before each runtime goes back to the pool. |
| 4. Cross-host fixture | **Holds.** It is written again in `aea9b529d`, after the last merge. |
| 5. FX-14 after FX-12 and D79 | **Holds.** The merge order is D76, FX-12, FX-14. |
| 6. The `for` form in a self-sealed envelope | Lane A2 labels it a known issue. **Agreed.** The gate refuses a `for` form that the `contract` form passes. That is an admission defect in `hash_seal_admissible`, not a change to who pays. §6.3 lets an installer use `# P`, and the `contract` form gives that today. |
| 7. Suite results | **Partly met.** The log gives the v6 tests only (85 passed, 0 failed, 7 ignored). The full release suites of the crates are not posted for this merge. Lane A2 must post them before the handoff. |
| 8. F41 | **Holds.** `charge_recorded_work` calls `seal::purse_of`. F41 is closed. |
| 9. Funding oracle in the close | **Not installed**, as the log says. This is part of F40 (blocking), and FX-15 decides it. |
| 10. FX-12 open list | **Holds.** The FX-12 log records items 1 and 2 as done by D79, and item 3 as lane B's. |

Lane A2's resolution keeps the rebased D76 in each conflict and applies FX-12's own metering commit on it. The merged tree has `admits_waiting` in `extract_first_match`, the record on a stored datum written by `eval_send`, FX-12's work map and settlement, and the close-time charge. `admits_waiting` is accepted as the hook point (D79 open point 3).

**Result:** the merges meet requirements 1 to 6, 8 and 10. Requirement 7 is open, and requirement 9 is part of F40. D9 is now closed on the integration branch.

### FX-14 follow-up against Decisions 84 and 87

| Check | Result |
|---|---|
| A receipt is spent on use (Decision 84) | **Holds.** PoS keeps `spentReceipts`. `bond` refuses a spent receipt and adds the receipt to the set when it records the pending bond. A refused attempt does not spend the receipt. `a_bond_receipt_is_spent_by_its_first_registration` covers it. **F42 is closed.** |
| The handle is located at the validator's seal (Decision 87) | **Partly.** `rho:rev:bond` `handle` gives the handle only to code under the validator's enclosing seal. The handle is a name derived from the key, so it needs no state. `withdraw`, `commitRandomImage` and `revealRandom` resolve the key through `handleKey`. The close still also sends the handle on the bonding deploy's `handleCh` (FX-14 decision 7). |
| Genesis validators take their handles at their own seals | **Holds.** `a_genesis_validator_takes_its_handle_at_its_own_seal` covers it. |
| A published deployer id gives nothing | **Holds.** `a_validator_takes_its_handle_only_at_its_own_seal` and the extended `a_published_deployer_id_does_not_act_for_its_validator` cover it. |

**F43 (narrowed) — Medium, known issue: the close sends the handle to a channel that the registering code chooses.** The `handle` command reads the handle at the validator's seal. But the close also sends `handleOf (pk)` on the `handleCh` of the registration. Decision 84 lets a receipt be passed on before use. Code that registers with a passed-on receipt therefore gets the validator's handle. The handle cannot be revoked (FX-14 known issue 1). So that code can act for the validator for the life of the key. Decision 87 says that only code under the validator's seal reads the handle.
- **Reason for the label:** the validator must choose to pass on the receipt, so the finding is Medium, below the gate. It is not a §6.3 item.
- **Proposed fix:** the close sends only the result of the registration on `handleCh`, and every holder takes the handle through `handle` at its own seal. Reversible, lane B.
- **Change of label:** if code that the validator did not give a receipt to can get a handle, the finding becomes High and blocking.

Lane B's two known issues in the FX-14 log are accepted with their labels:
- A handle cannot be revoked. This is Medium at most, because the validator must pass the handle on.
- `spentReceipts` grows without bound. This is storage growth.

### Gate status on FX-8b at `aea9b529d`

| Item | Label | State |
|---|---|---|
| F40 (envelope runs and close): work is not gated step by step. Foreign work counts against the client's phlo limit. | **Blocking** | Open. It waits for FX-15 and the Q-32 ruling. |
| F39: an own key whose owner the gate did not commit is charged to the client | **Blocking** | Open. `settle::own_payers` is unchanged on the merged tree. Decision 83 gives the fix. |
| D8b: a COMM that the close fires pays no cell | **Blocking** until question 13 is ruled | Open. |
| Requirement 7: full suite results for the merge | Merge condition | Open. |
| F38 | Known issue | Open. |
| F43 (narrowed) | Known issue | Open. |
| The `for` form under `# P` (D79 open point 4) | Known issue | Open. |
| F41, F42, D9 | — | Closed. |

The FX-14 follow-up must still be merged into FX-8b. A-8 re-audits that merge, FX-15, and the fixes for F39 and D8b when they land.


## Ruling of 2026-11-03: Q-32 (Decision 89, design r13)

Decision 89 answers Q-32. It is in design r13 (publications `95844af`). From this point on, A-9 reads against r13 §6.3. The r12 text of §6.3 is unchanged in r13, except for the paragraph "Work is gated, not only the cell".

**What r13 §6.3 says:**
- Play may stop work under t at a different step from one execution to another. That is admissible.
- One validator executes the block and produces a trace. The other validators replay that trace and confirm the same state change.
- A refused step is an event of the trace, like a COMM. It carries the step's identity (its task's random state and position) and the amount that t had drawn when the step was refused.
- Replay follows the trace. It admits exactly the steps that the trace admits. It checks that each admitted step was payable, and that each refused step was not, at the drawn amount that the trace records.

Lane C's "early stop" known issue no longer applies.

**Correction to revision 14:** revision 14's watch list asked that "play and replay stop at the same point". That is wrong under r13. Play and replay stopping at different points across executions is not a divergence in itself. The requirement is that replay follows the trace, and accepts the trace only if it is honest.

**The three FX-15 replay checks:**

| Check | Replay must reject | Pinning test |
|---|---|---|
| Early stop | A trace that refuses a step that was payable at the recorded drawn amount | Take an honest trace. Change one admitted step under t to a refused step. Expect replay to reject the block. |
| Forged drawn amount | A trace whose recorded drawn amount for a refused step differs from what t had drawn at that point in the trace | Take an honest trace. Change only the drawn amount on one refused step. Expect replay to reject the block. |
| Unpayable admitted step | A trace that admits a step that was not payable | Take an honest trace. Change one refused step under t to an admitted step. Expect replay to reject the block. |

**Gate label:** replay that accepts a trace that fails one of the three checks is a divergence from r13 §6.3, so it is **blocking** under Decision 88.

**Note on the forged-amount check:** r13 says that replay checks payability "at the drawn amount the trace records". The forged-amount check needs one more step. Replay must compute what t has drawn as it follows the trace, and compare that sum with the recorded amount. If replay only reads the recorded amount, then a forged amount that is too high can make a payable step look unpayable. An early stop would then pass. FX-15 must show that replay computes the drawn amount, and the pinning test shows that it compares the two.

## Revision 16 (actions A-8 and A-9): the client's phlo limit on FX-12, the FX-12 merge into FX-14, and the FX-8b suite results

- **Commits audited:**
  - `patch/FX-12` at `eab30238f`: the client's phlo limit counts only its own surfaces, and foreign work in a run has its own bound.
  - `patch/FX-14` merge `cc3168032`, with parents `2bfaaf942` (the FX-14 follow-up) and `eab30238f`.
  - `patch/FX-8b` at `4e20959b8`, suite results only.
- **Reference:** design r13 §6.3 (publications `95844af`), Decisions 83 to 89, plan r8.12.
- **Method:** a code reading of the commits and the decision logs. No tests were run.
- **Not yet landed:** `patch/FX-15`, a fix for F39, and a ruling on question 13 (D8b). FX-8b does not yet contain the FX-14 follow-up or `eab30238f`. `patch/Q-32` (`e6abd727b`, lane A2's review of the FX-15 refusal event) goes through lane C and lane A2 and is not audited here.

### FX-8b suite results (revision 15, requirement 7)

**Met.** Lane A2 posts the full suites for the tip `aea9b529d`:
- rholang lib 1,451 passed
- rspace_plus_plus 421 passed
- casper lib 851 passed
- casper `--test mod` 1,260 passed
- node 466 passed

None failed.

### The client's phlo limit (FX-12 decisions 9 and 10)

**What the commit does:** `CostManager` has an own origin, which the executor sets to the envelope's signature for each play and replay run. A charge under a record from another envelope goes into the work tally, but it does not use up the client's phlo limit. It counts against a separate per-run bound, `FOREIGN_WORK_BUDGET`, which equals the default block demand cap. When foreign work passes that bound, the run stops with `OutOfPhlogistons` and the envelope is `Exhausted`. A charge with no record, or under the envelope's own records, counts against the client's limit as before.

| Check | Result |
|---|---|
| Foreign work no longer counts against the client's phlo limit | **Holds.** `forms_spec::work_that_stored_code_pays_for_does_not_count_against_the_clients_limit` fires a continuation whose work is more than Alice's limit. Alice's run is not exhausted, and her charge and her run's meter stay within her limit. The test replays. The unit test `only_the_clients_own_work_uses_up_its_limit_and_foreign_work_has_its_own_bound` covers the two meters and the legacy runtime. |
| A surface with no record still counts as the client's (Decision 81) | **Holds.** A charge with no record is not foreign. |
| The own origin is cleared before the runtime goes back to the pool | **Holds.** `give_back` calls `set_own_origin(None, 0)`. |
| F38, second half (the `Exhausted` test mixes two meters) | **Closed.** `run.compute_phlo` now holds the client's own work only. The first half of F38 (refund keying) stays a known issue. |

**Lane B's finding in the FX-12 log:** lane B labels as blocking a High safety finding in its own first version of decision 9. The finding is that a continuation that never stops could run without limit. Decision 10 fixes it in the same commit. **Agreed.** The finding is closed on FX-12.

**F40 after this commit — still blocking.** The bound in decision 10 is a constant. It is not the wallet at t.
- Under the bound, work under t still runs when t's wallet cannot pay, and settlement still caps the draw.
- At the bound, the whole run stops, and that includes the client's own work.

r13 §6.3 says that work under t stops when the wallet at t cannot pay, and that no shortfall goes to another party. Lane B's log calls the bound an interim measure until the Decision 86 gate. **Label: blocking**, as part of F40. The reason is the same as in revision 14. FX-15 must replace the bound with the per-step gate at t, or keep it only as an upper bound on top of that gate. In both cases, the client's own work must not stop because of work under another membrane.

### The FX-12 merge into FX-14

**Clean.** The combined diff is empty. FX-14 at `cc3168032` contains the FX-14 follow-up and `eab30238f`. F43 (narrowed) is unchanged, because the close still sends `handleOf (pk)` on the registration's `handleCh`.

### Gate status

| Item | Label | State |
|---|---|---|
| F40: work is not gated step by step at t. The interim bound is a constant, and it stops the client's own work. | **Blocking** | Open. It waits for FX-15. |
| F39: an own key whose owner the gate did not commit is charged to the client | **Blocking** | Open. No fix has landed. |
| D8b: a COMM that the close fires pays no cell | **Blocking** until question 13 is ruled | Open. |
| F38, first half (refund keying) | Known issue | Open. |
| F43 (narrowed) | Known issue | Open. |
| The `for` form under `# P` | Known issue | Open. |
| Revision 15, requirement 7 | — | Met. |
| F38, second half. Lane B's finding about the unbounded foreign run. | — | Closed on FX-12. |

The next merge into FX-8b must take the FX-14 follow-up and `eab30238f`. A-8 re-audits that merge.


## Rulings of 2026-11-03: design r15 (Decision 94)

From this point on, A-9 reads against design r15 §6.3 (publications `0bea858`). r15 differs from r14 only in the charging rule sentence, and in the close and genesis paragraphs of §6.3. r14 adds one sentence to the r13 text. Replay adds up what each membrane has drawn as it follows the trace, and a recorded amount that differs from that running total fails the block. That is the forged-amount requirement of the Decision 89 section. r14's exemption of the close (Decision 92) is withdrawn by r15. This audit never applied that exemption.

**The rule (r15 §6.3):** all Rholang code is charged. Code that is not Rholang is not charged. Rholang that is not decorated is charged to its deployer.

**Consequences:**
- **The close is charged.** PoS's undecorated close code is charged to the system deploy's deployer, which is the proposing validator. The validator pays from its fees. A stored continuation that the close fires is charged to its installer, for its cells and its work. FX-15 gates it like any other membrane. Only consensus code written in Rust is free.
- **Bootstrap is sunk cost.** The genesis block and its system deploys are not charged.
- **Genesis code is charged to its caller.** Code installed at genesis has no deployer. When a later deployment fires it, the deployer of that run pays (Decision 81, confirmed).

### Effect on the audit

| Item | Before | After r15 |
|---|---|---|
| D8: work of a stored continuation that the close fires | Closed for work on FX-12 by `charge_recorded_work`, with a cap at the balance | **Stays closed** as to who pays. The cap is part of F40: FX-15 must gate this work like any membrane. |
| D8b: cells of a COMM that the close fires | Blocking until question 13 is ruled | **Blocking (r15 §6.3).** It stays open until FX-15 gates the close and charges its cells to the installer. Question 13 is answered by Decision 94. |
| **D10 (new): PoS's undecorated close code is not charged** | — | **Blocking.** FX-12 decision 7 says that work with no record (the close itself, PoS) is not charged. r15 §6.3 charges it to the proposing validator. The fix must charge the cells and work of the close's undecorated code to the proposer, and the post-state root must carry the charge so that replay checks it. |
| Genesis code fired by a deployment (Decision 81) | Holds: a surface with no record is charged to the firing client. Decision 85 gives two cells for a COMM between the client's send and genesis code. | **No change.** |
| Bootstrap | Not audited | **To check:** the genesis block and its system deploys draw no charge. |

**D10 test plan:**
1. A close whose PoS code does known work and COMMs. The proposer's wallet pays that work and those cells at the close.
2. The block replays to the same root.
3. A stored continuation that the close fires is charged to its installer, not to the proposer.

**Question for the lanes (no ruling needed):** the proposer pays "from the fees it earns". The fix must state the order: does settlement credit the fees before the close charge, or does the close charge draw on the proposer's pre-state balance? Both are reversible. A lane decides and logs it.


## Watch item: the merge of `dev` into FX-8b (instruction of 2026-11-03)

Lane A2 will merge `dev` into FX-8b before the handoff. A trial merge of `origin/patch/FX-8b` (`4a74b0e34`) with `origin/dev` (`746a1c368`) from the merge base `1a19f1127` stops with conflicts in two files only:
- `casper/src/rust/rholang/runtime.rs`
- `node/src/rust/api/web_api.rs`

Lane B's dry-run log lists the files that changed on both sides and merged without a conflict. Two of them touch authority:
- `rholang/src/rust/interpreter/reduce.rs`
- `rspace++/src/rspace/space_matcher.rs`

**A-8 checks on the merge commit, by reading:**
1. **`runtime.rs`:** keep FX-12's ledger reset before the system source, and `charge_recorded_work` after it and before the checkpoint. Keep the D10 charge when it lands.
2. **`web_api.rs`, injection on both wallet paths:**
   - On the legacy path, `dev` binds the address as a deploy parameter in the AST (`BALANCE_RHOLANG_TEMPLATE`, with tests that compare the compiled `Par` with an injection payload). The merge must keep that binding.
   - On the native path, FX-8b builds the term with `native::balance_term`. The merge must keep the address check on that path, and keep FX-8b's `balance_query_rejects_addresses_that_are_not_valid` test.
   - Both `dev` tests and the FX-8b test must stay.
3. **`reduce.rs` (spot check):** `dev`'s join rule sits next to FX-2's record at fire and FX-4's hash-seal check. Check three things:
   - The seal record installed at fire is not lost on the join path.
   - FX-4's check still runs before the origin check (F30).
   - D79's record on a stored datum is still written by `eval_send`.
4. **`space_matcher.rs` (spot check):** `admits_waiting` still runs for each waiting continuation in `extract_first_match`, also on any path that `dev` added.

### F44 — Low, known issue: the native balance path builds its query from a string

**Code:** on the native wallet path, `web_api.rs` calls `native::balance_term`. That function checks the address with `VaultAddress::parse` and then puts it into the term with `format!`. The `dev` fix for the legacy path binds the address as a deploy parameter in the AST instead, so no input text reaches the source.

**Effect:** the native path depends on the address check to stop injection. The check accepts only a parsed REV address, and FX-8b's test refuses an injection payload. No injection is known.

**Reason for the label:** this is a read-only query path, the input is checked, and no authority or §6.3 effect is known. So the finding is Low, below the gate.

**Proposed fix:** bind the address on the native path through the same deploy-parameter mechanism as `dev`'s legacy template. Reversible, lane B.


## Ruling of 2026-11-03: D10 (Decision 96, design r16)

From this point on, A-9 reads against design r16 §6.3 (publications `6a4c777`). r16 changes only the close paragraph of §6.3 and adds Decision 96.

**Decision 96:**
- The close's PoS code must run to completion, so it is not gated.
- Its cells and work are charged to the proposer from the proposer's balance after the block's settlement. That balance includes the fees that the block earned. This answers the ordering question of the r15 section: the charge comes after settlement.
- A block whose proposer cannot cover the charge is invalid, and the other validators reject it.

The fix is FX-16 (lane B). D10 stays **blocking** until FX-16 lands and passes the checks below. Stored continuations that the close fires do not change. They go to their installers and FX-15 gates them (D8b).

**D10 test plan, as amended:**
1. A close whose PoS code does known work and COMMs. The proposer's wallet pays that work and those cells at the close, from the post-settlement balance.
2. The block replays to the same root.
3. A stored continuation that the close fires is charged to its installer, not to the proposer.
4. A block whose proposer cannot pay the close charge is rejected by validation and by replay.

**Checks on FX-16, by reading:**
- The charge is made after settlement, and the post-state root carries it.
- The PoS code of the close has no gate. It runs to completion, and only the validity check can refuse the block.
- The validity check reads the proposer's post-settlement balance in the same way in play, in validation and in replay.
- Cells and work with no record that the close runs go to the proposer. Work under a stored record still goes to its installer (`charge_recorded_work`, gated by FX-15).
- **Bootstrap (in FX-16):** the genesis block and its system deploys draw no charge.


## Revision 17 (actions A-8 and A-9): the FX-14 follow-up merge into FX-8b, and FX-15 (replay and settlement sides)

- **Commits audited:**
  - `patch/FX-8b` merge `11c9cb42f` (parents `4e20959b8` and `cc3168032`) and the log commit `4a74b0e34`.
  - `patch/FX-15-replay` at `b5537b44b` (lane A2): `8bee6cdde`, the Decision 89 checks, and `b5537b44b`, the close's `SystemGate` and the cover check.
  - `patch/FX-15-settle` at `0f1d037f7` (lane B), on `8bee6cdde`.
- **Reference:** design r16 §6.3 (publications `6a4c777`), Decisions 86, 88, 89, 94 and 96.
- **Method:** a code reading of the commits and the two FX-15 logs. No tests were run.
- **Not yet landed:**
  - Lane C's play-side meter and refusal event. `refusals_in` returns no refusals.
  - FX-16.
  - A fix for F39.
  - The merge of `dev` into FX-8b.

  `patch/Q-32` (`0579f558b`, a memo on Decision 96) is not audited here.

### The FX-14 follow-up merge into FX-8b

**Holds.** The merge commit has one conflict, in the FX-12 log. In `executor.rs`, `run` and `replay_one` keep both `set_funding` (D79) and `set_own_origin` (FX-12). `give_back` clears the own origin, and each run clears the funding oracle before the runtime goes back to the pool. Lane A2 posts the v6 tests (88 passed) and `rev_bond_spec` (3 passed). F42 stays closed. F43 (narrowed) does not change, because the close still sends the handle on `handleCh`.

**Label note:** lane A2 labels `FOREIGN_WORK_BUDGET` a known issue. This audit keeps it **blocking** as part of F40 (revision 16). The reason: a constant bound that stops the client's own work diverges from r16 §6.3, which says that work under t stops at t's wallet and that no shortfall goes to another party. The two labels agree that FX-15 replaces the bound.

### FX-15: what has landed

FX-15 is in three parts. Only the replay and settlement parts have landed. The gate itself, which is lane C's meter and refusal event, has not landed. So the first three gate checks have nothing to read yet:
- Each step under t is admitted against the wallet at t.
- Work under t stops when the wallet cannot pay.
- The close gates the stored continuations that it fires.

### The Decision 89 replay checks (lane A2)

`check_stop` uses A, the free balance of the membrane's purse at the pre-state less the gate table, in phlo at the origin price. It also uses D, the drawn amount on the refusal, c, the step cost, and W, the gross work that replay observes under the membrane.

| r16 check | Code | Unit test | Trace-alteration test |
|---|---|---|---|
| (a) Early stop | refuse if A − D ≥ c | `a_stop_at_a_payable_step_is_refused`, `a_drawn_amount_below_the_stop_point_is_refused` | **None.** |
| (b) Forged drawn amount | refuse if D > W | `a_drawn_amount_above_the_replayed_work_is_refused` | **None.** |
| (c) Unpayable admitted step | refuse if D > A on a stopped membrane. `check_unstopped` (W ≤ A) exists, but replay does not call it. | `an_admission_past_the_balance_is_refused` | **None.** |
| The refused step is real | replay must reach the step at the recorded cost | `a_refused_step_that_replay_does_not_reach_is_refused`, `a_forged_step_cost_is_refused` | **None.** |

The unit tests call the check functions directly. They do not alter an honest trace and replay it, because no trace can carry a refusal until lane C's event type lands. Lane A2's log names that integration test as open.

**F45 — blocking: replay checks the drawn amount against a bound, not against a running total.**
- **What r16 says:** r16 §6.3 (the r14 sentence) says that replay adds up what each membrane has drawn, and accepts the trace only if that running total *equals* the amount recorded on each refusal.
- **What the code does:** the code accepts any D with A − c < D ≤ min(A, W). Lane A2's log gives the reason. Replay does not stop a membrane on its own meter, because another interleaving can run more compute before the next IO step. So W can be more than the drawn amount at the true stop point.
- **Effect:** a recorded D that is higher than the true drawn amount, but not higher than W, passes. That D can make a payable step look unpayable. That is an early stop that replay accepts.
- **Reason for the label:** replay accepting a trace that fails check (a) or (b) is the blocking case of the Decision 89 section.
- **Proposed resolution:** a ruling on one of two paths. Either replay computes the running total at the refusal point exactly, as r16 says, or the design admits a bounded window and states its bound. Escalated, question 14.

**Check (c) on unstopped membranes stays blocking** as part of F40. `check_unstopped` is not called. Lane A2's log says that the call must turn on with lane C's meter, or replay rejects blocks that FX-12 accepts today.

**Refusal that names the client:** `v6::stops::check` rejects a refusal of the client's own membrane. No test covers this. This is a test gap, and it is not labelled.

### Settlement of a stopped membrane (lane B)

`settled_work` takes a stopped membrane's work from the record, and bounds it by the least D of the membrane's refusals. A stop without recorded work is refused. The other membranes keep the replayed work. Unit tests cover each case.

**F46 — blocking: the recorded work of a stopped membrane has no lower bound.** Lane B labels this a known issue. The reason given: nobody is overcharged, and the loss is fee income of at most D for each stopped membrane. **This audit labels it blocking.**
- **Reason:** r16 §6.3 says that all Rholang is charged and that no step runs unpaid. A record that reports less work than ran under t lets steps run unpaid, up to D for each stopped membrane. That is a §6.3 divergence, whatever the authority effect.
- **Proposed resolution:** the same ruling as F45 (question 14). A replay that knows the running total at the refusal also knows a lower bound. Lane B's log proposes one: net work ≥ D − (refunds under the membrane in replay).

### The close (lane A2, `b5537b44b`)

- `compute_state_gated` and `replay_compute_state_gated` take a `SystemGate`, which has the funding oracle, the payer and a cover cell, and the stop check. The legacy paths pass no gate.
- The payer is the block sender's wallet.
- The stop check reads `BlockPurses` at the settled root, with an empty commitment table, because settlement releases the block's holds.
- `stops::close_covered` runs after the close, in execute and in replay. A proposer that owes more than it holds makes execute return an invalid-block error and replay return a mismatch.

This holds as a frame for Decision 96. Three items wait for other lanes:
- FX-16 records the charge. Until then, the cover check passes because nothing is recorded.
- The funding oracle in the close is `None` until lane C sets it, so D8b stays open.
- A cached replay of the close skips both checks. Lane A2's reason: only a block that this node played is in the cache, and play ran both checks. **Accepted.**

### Gate status

| Item | Label | State |
|---|---|---|
| F40: no step-by-step gate at t (lane C's meter), `check_unstopped` not called, the constant `FOREIGN_WORK_BUDGET` | **Blocking** | Open |
| F45: the drawn amount is checked against a bound, not the running total | **Blocking** | New. Question 14. |
| F46: no lower bound on a stopped membrane's recorded work | **Blocking** | New. Lane B labels it a known issue. Question 14. |
| No trace-alteration tests for checks (a) to (c) | **Blocking** | Open until lane C's event type lands |
| D8b: the close's funding oracle and the cells of fired continuations | **Blocking** | Open |
| D10: the close's PoS code charged to the proposer | **Blocking** | Open. Waits for FX-16. The frame is in `b5537b44b`. |
| F39 | **Blocking** | Open |
| F38 (refund keying), F43 (narrowed), F44, the `for` form under `# P` | Known issue | Open |
| The `dev` merge into FX-8b | Watch item | Not merged |


## Ruling of 2026-11-03: D10 (Decision 97, design r17)

From this point on, A-9 reads against design r17 §6.3 (publications `0c58795`). r17 changes only the close paragraph of §6.3. Decision 97 replaces Decision 96.

**Decision 97:**
- The close's PoS code is pre-charged to the proposer. Before any envelope of the block runs, the proposer's wallet is charged a fixed upper bound for that code, at the proposer's price.
- After the close, the part that was not used is returned.
- A block whose proposer cannot cover the pre-charge is not accepted, so no block is ordered whose close could go unpaid.
- The PoS code of the close is still not gated. Stored continuations that the close fires still go to their installers, gated by FX-15 (D8b).

This ruling also resolves lane A2's chain-halt finding.

**Effect on the D10 section of Decision 96:** its post-settlement order, its fourth test and its FX-16 checks are withdrawn. The fees of the block no longer count toward the close charge. Lane A2's `stops::close_covered` check after the close (`b5537b44b`) was written for Decision 96. A-8 checks that FX-16 replaces it with the acceptance check, or keeps it only as a guard that cannot fail.

**D10 test plan (Decision 97):**
1. The proposer pays for known PoS work, and the unused part of the bound is returned.
2. The block replays to the same root.
3. A stored continuation that the close fires is charged to its installer, not to the proposer.
4. A block whose proposer cannot cover the pre-charge is refused at acceptance.
5. A test fails if a close ever uses more than the bound.

**Checks on FX-16, by reading:**
- The pre-charge is taken before the first envelope runs, and the post-state root carries both the pre-charge and the return.
- The amount held for the close reduces what the proposer's wallet can pay for anything else in the block. This includes the gate table, `BlockPurses`, and the proposer's own envelopes and surfaces.
- The bound and the price are fixed values that play, validation and replay read alike. The design says "at the proposer's price". FX-16 must state which price that is.
- The acceptance check reads the proposer's balance at the pre-state, and it reads it in the same way in proposal, validation and replay.
- The close's PoS code is not gated.
- Cells and work with no record that the close runs are taken from the pre-charge. Work under a stored record still goes to its installer.
- **Bootstrap:** the genesis block and its system deploys draw no charge.

D10 stays **blocking** until FX-16 lands and meets these checks.


## Ruling of 2026-11-03: P11 (Decision 98, design r18)

Design r18 adds principle P11 (Decision 98): **no block is final until its state is known.** r18 is not yet in publications (the latest commit there is `0c58795`, r17). This section records the ruling as the plan states it. A-9 reads the r18 text when Meredith commits it.

### D11 — blocking: the v6 seam finalizes a block on its order, before its execution

**Code (`patch/FX-8b` at `4a74b0e34`, `casper/src/rust/v6/engine.rs`):**
- `tick` reads `dag.last_finalized_block()` and advances the linearizer on it.
- The linearizer emits each block on the finalized main chain into the executor core (`Advance::Emitted` and `core.emit`).
- The block is executed only after that, in a later `Action::Execute`, and its record is replayed in `Action::Replay`.

So consensus makes a block final on its order alone. Its post-state, and whether its execution is valid, are known only after it is final.

**Effect:** a block can be final before its state is known. That contradicts P11.

**Reason for the label:** this is a divergence from the design (r18, P11, Decision 98), so it is **blocking** under Decision 88. The resolution is the subject of Q-33. Lane A2 writes the memo, and Meredith rules.

### Review of lane A2's Q-33 memo

The memo has not arrived. When it does, A-8 reviews it before the ruling, and checks two things in particular:

1. **A block that its execution shows to be invalid can never be final.**
   - Finality must depend on a known, valid post-state, and not on order alone.
   - The review checks every invalid outcome that execution or replay can give. This includes a failed replay, a record mismatch, a failed Decision 89 check, a close pre-charge that the proposer cannot cover (Decision 97), and a conservation failure.
   - It checks that each of these outcomes keeps the block from finality, in play, in validation and in replay.
   - It also checks that no other path to finality bypasses execution, for example a cached record, the A-LIN hold path, or a restart.
2. **Blocks built on top of an invalid block are handled safely.**
   - The memo must say what happens to the descendants of a block that execution shows to be invalid. The options are that they are invalid too, that they are re-based, or that their envelopes return to the pool.
   - The review checks that no descendant can become final on top of the invalid state.
   - It checks that the outcome is the same on every validator, and that envelopes and held amounts are neither lost nor charged twice. That covers gate holds, the Decision 97 pre-charge and pending bonds.
   - It checks that the chain does not halt. Lane A2's chain-halt finding was resolved by Decision 97. The memo must not reopen it.

The review also checks that the memo keeps the Decision 89 rule: the trace of an honest validator is accepted on replay, even when play stopped at a different point.


## Review of the Q-33 memo (Decision 99, design r19), before FX-17

- **Document reviewed:** `docs/rev-stacks/memos/Q-33.md` on `patch/Q-32` at `1c0256324` (lane A2). The memo's base is `patch/FX-15-replay` at `7ac56f01a`.
- **Code read for the review, at `7ac56f01a`:**
  - `finalization_runner::validate_commit` and `commit_lfb`.
  - `v6::engine::tick` and `admits_envelopes`.
  - `v6::lag::LagGate`.
  - `v6::core::attestation`.
  - `validate::repeat_deploy`.
- **Reference:** P11 (Decision 98), Decision 99 as the plan states it. r18 and r19 are not yet in publications (the latest commit there is `0c58795`, r17).
- **Method:** reading only. No tests were run.

**Result: not signed off.** The design direction is sound: split the order floor from the LFB, put P5 in Validate, and void instead of reject. Five items must change in the memo before lane A2 writes FX-17 code. Two of them break the no-stall argument. Each item has its Decision 88 label.

### What holds

- **Option A is correctly refused.** After order certification, descendants cannot be rejected without invalidating honest blocks.
- **Attestations name canonical roots.** `ExecutorCore::attestation` gives the root of the last canonical position, which citation and replay fix. It does not give the node's own speculative root. So attestations converge on the canonical records, and the finality rule does not require deterministic play. **Decision 89 still holds:** an honest trace whose play stopped at a different point is accepted on replay, and the canonical record that results is what validators attest.
- **The P5 safety argument holds.** Two roots for one position cannot both have more than 2/3 of the stake, unless more than 1/3 misattests, and misattestation is already evidence.
- **Every v6 block carries an attestation** (`check_close_cover` requires one). So blocks without envelopes, under the lag gate or floor pacing, still carry attestations, and P5 can still advance.
- **The A-LIN hold:** under `ALinPolicy::Halt`, `validate_commit` refuses the commit. The memo keeps the A-LIN check on the order floor, so a held node commits nothing.
- **Void and held amounts:** a void record has post-root = pre-root. So no gate hold, no Decision 97 pre-charge and no bond move of the void block reaches the state. Pending bonds from earlier blocks stay pending until the next close that runs. Nothing is lost or charged twice in the state, on the condition of item Q5 below.

### Blocking items

**Q1 — blocking: the order floor cannot advance while P5 refuses, so the chain stalls.**
- **What the memo says:** P5 refuses with `StateUnattested` when the clamped block is not above the current LFB. The order floor is written in the same transaction as the commit marker and the LFB pointer.
- **Effect:** a refused commit writes nothing, so the order floor does not move either. Execution reads the order floor. Attestations need execution above the LFB. P5 needs those attestations. So at start, and after every refusal, the order floor stays at the LFB, and nothing above it is executed or attested. This is the dependency loop that the stall analysis says does not exist. It exists through the shared commit.
- **Reason for the label:** this is a safety finding of High severity (the chain stalls), and the design would diverge from P11's no-stall argument.
- **Required change:** the order floor must advance on the order oracle's certification alone, in its own commit, which is guarded by the existing preconditions (P1 to P4, A-LIN, containment). P5 then moves only the LFB, up to the order floor. Some details move to the order-floor commit:
  - `NotHigher` is checked against the order floor.
  - The containment check and `derived_against` are checked against the order floor.
  - The finalizer's σ folds in the order floor.

  The memo must state each of these.

**Q2 — blocking: the lag gate measures against the LFB, so under P11 it never engages.**
- **Code:** `admits_envelopes` measures `Lag::measure(LFB number, number of the attested canonical block)`, which is LFB − canonical, floored at 0. Under P11, the LFB can never pass the canonical frontier, because the LFB needs attestations of canonical roots. So the lag is always 0, and the `LagGate` never engages.
- **Effect:** the memo's bound E ("blocks above the lag threshold carry no envelopes") fails. Execution backlog between the canonical frontier and the order floor is not bounded.
- **Reason for the label:** this is a High safety finding. It is the stall class of Issue #24.
- **Required change:**
  - The lag gate measures order floor − canonical.
  - `max_unattested` measures order floor − LFB, as the memo says.
  - Floor pacing (`floor_pacing_engaged`, measured from the LFB) must be assigned to one of the two floors.

**Q3 — blocking: a void record must be a canonical record that replay checks in both directions.**
- **What the memo says:** "every honest executor derives the same void record."
- **Gap:** under Decision 89, one executor's record is fixed by citation, and the others replay it. So two forgeries are possible, and replay must reject each:
  - A void record for a block that is valid.
  - A normal record for a block that is invalid.
- **Required change:**
  - The memo must state that the void decision is a function of the block and the canonical pre-state only. For the uncovered close, that is the proposer's balance at the pre-state, less the gate table.
  - It must state that replay recomputes the decision and rejects a record that disagrees.
  - It must list each invalid outcome and say which ones make a block void and which ones are faults of the attester or the citer. For example, a failed Decision 89 check or a record mismatch is a fault of the citer, not of the block.
  - A trace-alteration test for each direction belongs in FX-17's test list.
- **Reason for the label:** without it, a forged void record can drop a valid block's envelopes, and a forged normal record can make an invalid block's state final. Both are P11 divergences.

**Q4 — blocking: `VoidBlock` slashing can hit an honest proposer.**
- **What the memo says:** `Offence::VoidBlock` is a safety-class offence, slashed with B6.
- **Gap:** the uncovered-close test reads the proposer's balance at the block's pre-state, which is at its linear position. The proposer cannot see that state when it proposes, because blocks ordered before it are not yet executed. Other activity can lower the proposer's balance before that position. For example, a stored receive that the proposer installed draws on the proposer's wallet when other parties fire it (Decisions 75 and 79). `create_block`'s check is against the proposer's own view, so an honest proposer can pass it and still be voided.
- **Reason for the label:** slashing an honest validator is a High safety finding.
- **Required change:** a ruling. The options are:
  - A void without slashing for invalidity that depends on the pre-state at the linear position.
  - Slashing only for invalidity that the proposer could decide from its own justifications.

**Q5 — blocking: re-inclusion of a void block's envelopes is not specified.**
- **What the memo says:** the envelopes return to the pool with a rejection entry.
- **Gap:** `validate::repeat_deploy` refuses a block that carries a deploy signature already present in the DAG. The inclusion checks can also report `InclusionViolation`. A later block that includes an envelope from a void block could then be refused or reported. If the envelope can never be included again, it is lost. If the gate charges it again while the void block's charge somehow stands, it is charged twice.
- **Required change:**
  - The memo must state that an envelope in a void block counts as not included, for `repeat_deploy`, for the inclusion checks and for the deploy-index.
  - It must state that it is charged once, in the block that finally runs it.
  - FX-17 needs a test that re-includes such an envelope.
- **Reason for the label:** the "no envelope lost or charged twice" requirement of P11's descendant handling.

### Conditions on FX-17 (not blocking for the memo)

1. **Consumers of `last_finalized_block()`:** list every consumer and assign it to the order floor or the LFB. The list includes the linearizer, the lag gate, floor pacing, deploy status, bond lookup, pruning and the mergeable-block GC on `dev`.
2. **Restart:**
   - The linearizer resumes from the persisted canonical frontier and the order-floor register, not from in-memory state.
   - The attestation tally is rebuilt from the DAG.
   - The crash test must cover an order-floor commit without an LFB commit (from Q1).
3. **The close of a void block:** lane B confirms the PoS change (an epoch at the first block at or above the boundary). Lane B also lists the close's other per-block duties, and shows that each one is deferred to the next close that runs, and not lost.
4. **Wording:** the memo says that one attestation covers earlier positions "because execution is deterministic over a certified prefix". It should say "because the canonical records fix the state of the prefix". Under Decision 89, play is not deterministic.

### Known issues (Decision 88)

- **Storage growth while more than 1/3 of the stake does not attest.** `max_unattested` empties envelopes but does not stop proposals, so empty blocks still accumulate. This is the standard BFT liveness assumption, and it is not a §6.3 or P11 divergence.
- **Client latency.** "Finalized" now waits one more attestation round. This is P11's intent.
- **Decision 97's acceptance check** stays until the void path lands, as the memo says. It is inert at a pre-charge of 0.

**Sign-off condition:** A-8 signs off when the memo is revised for Q1 to Q5. Q4 needs a ruling.


## Revision 18 (actions A-8 and A-9): the FX-17 storage paper, F46, the Decision 97 gate hold, and the FX-16 measurement

- **Commits read:**
  - `patch/Q-32` at `d3df10dd3`: `docs/rev-stacks/memos/FX-17-order-floor-storage.md`, the storage paper for FX-17.
  - `patch/FX-15-replay` at `fac9aca5a`: `7ac56f01a` (Decision 97: the pre-charge is held at the gate) and `fac9aca5a` (a storage-boundary test).
  - `patch/FX-15-settle` at `dda77b2c5`: the F46 rule.
  - `patch/FX-16` at `8d0a837b5`: the close measurement and the bound, documentation only.
- **Reference:**
  - Design r19 (publications `8530da2`, with r18). r19 records P11 (Decision 98) and the Q-33 design (Decision 99). The r19 Decision 97 text says that an uncovered close is found at execution.
  - The other §6.3 items are unchanged from r17.
- **Method:** reading only. No tests were run.
- **Not changed:**
  - The Q-33 memo itself (`1c0256324`).
  - `patch/FX-8b`. The `dev` merge has not happened.
  - No lane C meter or refusal-event branch, no FX-17 code, and no F39 fix.

### Q-33: the FX-17 storage paper against the review

| Review item | Status |
|---|---|
| Q1: the order floor advances in its own commit | **Met on paper.** `commit_floors` writes the order pointer when P1 to P4 pass, whether or not P5 passes. A state-only call runs in every cycle, after Compute and whatever its outcome. So attestations that arrive after order still move the LFB. The two pointers share one LMDB transaction, but neither depends on the other being written. The loop of Q1 is gone. |
| Q2: the lag gate's reference | **Open.** The paper does not change `admits_envelopes`, which still measures LFB − canonical. |
| Q3: void as a replayed record, checked both ways | **Open.** |
| Q4: `VoidBlock` slashing of an honest proposer | **Open.** Needs the ruling on question 15. |
| Q5: re-inclusion of a void block's envelopes | **Open.** |

**Also from the paper:**
- **P5 reads the local canonical record.** An attestation supports position j only if its root equals the local canonical post-root at j. An attestation at a position with no local record waits. So the LFB never passes a block whose state this node does not know. **Holds.**
- **The tally is rebuilt on startup** from the block bodies above the LFB. **Holds** (condition 2 of the review).
- **Startup invariant:** height(order) ≥ height(LFB). **Holds.**
- **Block metadata "finalized" flags keep the state-final meaning. `order_floor()` is a new accessor.** **Holds.** The assignment of each consumer (condition 1) is still required.

**Answers to the paper's questions:**
1. **Must the order floor run P2 and A-LIN?** Yes. Execution follows the order floor, so the order floor takes today's LFB guards in full: P1 to P4, containment and A-LIN. `NotHigher`, containment and `derived_against` are checked against the order pointer, and the finalizer's σ folds in the order pointer. The LFB is a main-chain prefix of the order floor, so it needs only P5.
2. **Bonds from the LFB, one attestation round late:** acceptable to the audit, on one condition. Each computation of p* uses one bond set, which is the bonds at the LFB of that computation. This is not a §6.3 or P11 item. It is a known issue for latency, and the bond-latency ruling is Meredith's.
3. **Deploy status:** under P11, "finalized" must mean state-final only. An order-final status, if clients want one, needs a separate name. That is a product choice, and it is not labelled.

**Sign-off:** not yet. Q2, Q3 and Q5 need revisions to the memo or the paper. Q4 needs the ruling on question 15.

### F46: closed for a given drawn amount

`settled_work` now requires the recorded net work of a stopped membrane to equal D − R. D is the least drawn amount on its refusals. R is the refunds that replay observes under the membrane. A record with any other value is refused (`WorkNotSettled`).
- **Lane B's argument:** refunds occur only at IO steps, and replay under a stopped membrane runs exactly the logged IO steps of play. So R in replay equals R in play. Extra compute in replay changes only the gross work, not R.
- **This audit accepts the argument as written.** The claim that compute steps have no refunds should have a test that fails if a compute refund is ever added.
- Unit tests cover equality, a value below, a value above, R ≥ D and several refusals.

**Result:** F46 is **closed for a given D.** F45, which is whether D itself is exact, stays **blocking** under question 14.

### Decision 97 in `7ac56f01a`

- The pre-charge is a policy field on chain (`close-precharge`), and its default is 0. So every validator applies the same value.
- The gate holds the pre-charge on the proposer's wallet before any envelope, in play and in replay. Envelopes that draw on that wallet see the hold.
- Acceptance and `create_block` check the proposer's balance at the attested root.
- The check after the close (Decision 96) is removed.

**Holds against the r17 checks,** except for these items:
- **The price** is 1 until a price source is fixed (FX-16).
- **A partial hold:** the gate holds what is free when the balance has fallen. Under r19, an uncovered close makes the block void (Q-33). So lane A2's open item is a known issue only until the void path lands.
- **The five D10 tests** wait for the FX-16 mechanism.

### FX-16: the close measurement (`8d0a837b5`)

Lane B measures the close's PoS work. The worst case is about 4.87 million phlo, with 100 validators at an epoch boundary. Lane B chooses `CLOSE_PHLO_BOUND` = 10 million phlo. The mechanism is parked until Q-33.

**F47 — blocking: the close's work grows with the number of bonds, so a fixed bound does not hold.**
- **Code:** at an epoch boundary, `pickActiveValidators` runs over all bonds. Anyone can bond with the minimum bond, so the bonded set has no fixed size.
- **Effect:** the close must run to completion (Decision 97). When its work passes the bound, the part above the pre-charge runs unpaid. Lane B labels this a known issue, because nobody loses funds.
- **Reason for the label:** this audit labels it **blocking.** r19 §6.3 says that all Rholang is charged and that the pre-charge is a fixed upper bound of the close. Work above it is Rholang that runs unpaid.
- **Proposed resolution:** a ruling. The options are a cap on the bonded set, or a bound that scales with the number of bonds (a fixed part plus a part for each bond). Question 16. Lane B's planned test, `the_close_stays_inside_its_bound`, must include a row at the largest bonded set that the ruling allows.

**F48 — blocking: substitution into a fired continuation is charged to the firer.**
- **Code (measured by lane B):** a larger continuation body raises the firer's charge. In this case the firer is the proposer's close. The likely cause is that substitution of the received data into the body runs before the body enters the installer's membrane.
- **Reason for the label:** r19 §6.3 says that received data substituted into a continuation runs under the continuation's membrane, and that a fired continuation's work goes to its installer (Decisions 75 and 94). Lane B labels it a known issue, because the amount is small and bounded. This audit labels it **blocking**, because it is a §6.3 divergence whatever its size. It also applies to every stored continuation that a client fires, not only to the close.
- **Proposed fix:** run the substitution inside the record scope of the continuation. Lane C confirms the cause. The test is lane B's measurement, made exact: the firer's charge does not change with the size of the body.

### Gate status

| Item | Label | State |
|---|---|---|
| D11: v6 finalizes on order before execution | **Blocking** | Open. The Q-33 sign-off waits for Q2 to Q5. |
| F40: no per-step gate at t | **Blocking** | Open. Lane C's meter has not landed. |
| F45: D is checked against a bound | **Blocking** | Open. Question 14. |
| No trace-alteration tests | **Blocking** | Open |
| D8b: the close's funding oracle and the cells of fired continuations | **Blocking** | Open |
| D10: the close charged to the proposer | **Blocking** | Open. Waits for the FX-16 mechanism. |
| F47: the close bound does not hold as bonds grow | **Blocking** | New. Question 16. |
| F48: substitution charged to the firer | **Blocking** | New |
| F39 | **Blocking** | Open |
| F46 | — | Closed for a given D |
| F38 (refund keying), F43 (narrowed), F44, the `for` form under `# P`, storage growth under P11, client latency, the partial hold until the void path | Known issue | Open |


## Revision 19 (action A-9): lane C's FX-15 meter

- **Commit audited:** `patch/FX-15` at `fc3a6ca72` (lane C), cut from `patch/FX-8b` at `aea9b529d`.
- **Not in this branch:**
  - FX-12's own-origin commit `eab30238f` and the FX-14 follow-ups (which are in FX-8b at `4a74b0e34`).
  - Lane A2's `patch/FX-15-replay`.
  - Lane B's `patch/FX-15-settle`.
- **Reference:** design r19 §6.3 (publications `8530da2`).
- **Method:** a code reading of `accounting::membrane`, the executor and surface changes, and the FX-15 log. No tests were run.
- **Not changed since revision 18:** the Q-33 memo, the FX-17 paper, FX-16, FX-8b (no `dev` merge), and F39, F47 and F48. The Q-33 sign-off is still open on Q2 to Q5.

### The gate checks

| Check | Result |
|---|---|
| Each step under t is admitted against the wallet at t, less the block's holds, less what t has drawn | **Holds within one run.** `MembraneMeter::admit` tests each positive charge under a gated record against each wallet of its purse. The limit is `BlockPurses::free` (the pre-state balance less the gate table) less the gross amount that the wallet drew in this run. The charge is the phlo times the record's origin price. A record is gated when its owner is not a payer that the gate committed for the envelope (Decision 93). Across runs, see F49. |
| Work under t stops when the wallet cannot pay | **Holds.** A refusal stops the wallet in the same lock as the test. Every later step on that wallet is refused and logged with the same drawn amount. A refused step ends its branch with `MembraneStopped`, and the run does not fail. |
| The client's own work does not stop because of another membrane | **Holds.** Gated work goes to the work tally and not to the client's phlo state. This branch does not have `FOREIGN_WORK_BUDGET` (FX-12 `eab30238f`), and the merge must not bring it back (merge requirement 2). With the gate, work under a gated record is bounded by its wallet. |
| The cells of a COMM | **Holds.** `check_commit` admits the cells of all gated surfaces of a COMM together, after the spatial match. An unpayable COMM is vetoed, with a `Cell` refusal for each unpayable record. D79's `admits_waiting` filter now returns `true` by default (D79 decision 9 is superseded). |
| The close's funding oracle (D8b) | **Not wired.** `CloseFunding` gates every record that the close fires, against the block's limit less what the envelope runs drew. Lane A2 must install it through the `SystemGate`. The close's COMM cells are still not charged in the state, because the storage rule is off in system deploys (FX-12 decision 8). **D8b stays open.** |
| `check_unstopped` is on | **Holds.** `accept_metered` checks W ≤ A for each wallet that did not stop. |
| Refusals are read from the trace | **Not yet.** `replay` passes no refusals until lane A2's deploy-log event lands (FX-15 decision 10). Only `replay_with_refusals`, which tests use, takes them. |

### Trace-alteration tests

`region_spec::work_under_an_underfunded_installer_stops_and_the_client_pays_nothing_for_it` plays a block in which a loop under an underfunded process wallet stops. Then it replays the block in three ways:

| Alteration | Expected | Check |
|---|---|---|
| The honest refusals, replayed eight times | Same root, drawn amounts and settlement | Present |
| No refusals (each refused step becomes admitted) | Rejected, or a different root | Present. This covers (c), an admitted step that was not payable. |
| Drawn amount set to 0 on each refusal | Rejected | Present. This covers (b) in the "too low" direction. |
| A drawn amount that is too high, inside A − c < D ≤ min(A, W) | Rejected | **Absent.** This is F45. `accept_metered` uses the same window as lane A2's `check_stop`. |
| One admitted step changed into a refused step | Rejected | **Absent.** This is (a). |

### F49 — blocking: two runs in one block can each draw a wallet up to its whole free balance

- **Code:** the meter's limit is per run. It is `free(wallet)` less what that run drew. Envelope runs in one block run in parallel, and each has its own meter. Two runs that fire code under the same gated wallet can each be admitted up to the wallet's full free balance. Settlement then caps the draw (`surface_charges`, D76 decision 7), so the second run's admitted work is partly unpaid.
- **Design:** r19 §6.3 admits each step against "what t has already drawn in the run". It also says that no step is executed unpaid. Across parallel runs the two clauses conflict.
- **Reason for the label:** a step runs unpaid, which is a §6.3 divergence.
- **Proposed resolution:** a ruling (question 17). The options are:
  - One block-wide meter for each gated wallet, with the draws of all runs counted against one limit. Decision 89 replay handles the nondeterministic order.
  - A split of each gated wallet's free balance between the runs before they start.
  - A statement in the design that the settlement cap applies across runs, which makes this a known issue.

### F50 — blocking: some charges under a gated record are taken without admission

- **Code:**
  - FX-15 decision 8: a charge in a restore scope draws nothing and is never refused. A restore scope covers a persistent datum or continuation that is stored again, or a peeked datum that is produced again. The same applies to the post-COMM charges in `handle_result`.
  - Lane C's known issue 1: the `event_storage_cost` charge of a gated produce comes after the datum is stored. If it is refused, the datum stays and the charge is unpaid.
- **Effect:** these charges go to the work tally, and settlement caps them. So they can run unpaid, and they do not count toward the wallet's drawn amount.
- **Reason for the label:** r19 §6.3 admits "each charge of the cost schedule", and no step runs unpaid. Lane C's reason for decision 8 is real: a refused restore would delete a persistent contract. That makes it a design question, not a code defect.
- **Proposed resolution:** a ruling (question 18). The options are:
  - These charges are pre-admitted with the step that caused them (the COMM, or the store).
  - The design exempts storage bookkeeping charges from the gate, which makes this a known issue.

### Lane C's other known issues

| Item | Label | Reason |
|---|---|---|
| For a compound owner, the stop is checked against the first component wallet that fits its drawn amount | Known issue | Play and replay use the same rule, so the outcome replays. It only changes which component stops first. |
| A gated charge outside every step scope has a fixed identity | Known issue | No test reaches this path. If two such refusals occur, `accept_metered` refuses two refusals with one step. That gives a failed replay, not an unpaid step. |
| The veto in `check_commit` costs one extra match attempt | Known issue | It affects cost to the validator only. |

### Status of earlier items on this branch

- **F39:** still **blocking.** Decision 93 gates an own-envelope record whose owner the gate did not commit, so admission draws on the owner's wallet. But `settle::own_payers` is unchanged. Settlement still charges such a key to the client. The fix must charge a gated own key to its owner, as a surface charge.
- **F45:** still **blocking** (question 14).
- **F48:** not addressed. The fired body's identity uses the COMM's merged state, but substitution still runs before the installer's membrane.
- **FX-15 decision 11** cites Decision 96. Under Decision 97 the close's PoS code is pre-charged. The text must be updated at the merge.

### Merge requirements for FX-15 into FX-8b

1. **One replay mechanism.**
   - Lane A2's `ReplayStops` (stop at the storage boundary, `stops::check`) and lane C's `MembraneMeter` replay (refuse exactly the logged steps, `accept_metered`) implement the same Decision 89 checks.
   - The merge keeps one of them, and one refusal type (`stops::Refusal` or `membrane::Refusal`).
   - It keeps lane A2's checks that replay reaches each refused step at its recorded cost.
2. **The client's limit.** FX-12's `set_own_origin` and `FOREIGN_WORK_BUDGET` (`eab30238f`) conflict with lane C's gate in `accounting/mod.rs`. Keep lane C's rule (gated work never counts against the client's limit). Remove the constant bound, which F40 asks for. Keep the own-origin rule only for records that are not gated.
3. **Settlement.** Lane B's `settled_work` (recorded work = D − R) applies to the stopped wallets of lane C's meter.
4. **The deploy-log refusal event (lane A2) lands before or with the merge.** Without it, `BlockExecutor::replay` passes no refusals. Then every block in which play stopped a membrane fails replay on the other validators. That is a High safety finding (the chain halts), so it is **blocking** as a merge condition.
5. **The close.** Install `CloseFunding` in the `SystemGate`. The close limit must also subtract the Decision 97 hold on the proposer's wallet (`7ac56f01a`) when a fired record draws on that wallet.
6. **Tests at the merge:**
   - The region_spec test above.
   - Lane A2's storage-boundary test.
   - Lane B's settlement tests.
   - The two absent alterations: a too-high D, and an admitted step changed into a refused one.

### Gate status

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | Open. The Q-33 sign-off waits for Q2 to Q5. |
| F40 | **Blocking** | Implemented on `patch/FX-15`. It stays open until the merge into FX-8b meets requirements 1 to 4. |
| F45 | **Blocking** | Open. Question 14. |
| F49: per-run limits across parallel runs | **Blocking** | New. Question 17. |
| F50: charges without admission | **Blocking** | New. Question 18. |
| FX-15 merge requirement 4 (deploy-log refusal event) | **Blocking** | Open |
| Trace-alteration tests (a) and the too-high D | **Blocking** | Open |
| D8b, D10, F39, F47, F48 | **Blocking** | Open |
| F38, F43, F44, the `for` form under `# P`, the three lane C known issues above, storage growth and client latency under P11, the partial hold | Known issue | Open |


## Rulings of 2026-11-03 (evening): design r20, Decisions 101 to 106, and question 14

From this point on, A-9 reads against design r20 (publications `23fb421`). r20 changes §6.3 only by the items below.

| Ruling | Answers | Effect on the audit |
|---|---|---|
| **Decision 101:** a void caused by state that the proposer could not see is recorded but not slashed. Only invalidity visible from the proposer's own justifications is slashable. | Question 15, Q-33 item Q4 | **Q4 is resolved by ruling.** The revised Q-33 memo must apply it to `Offence::VoidBlock`. An uncovered close at the true pre-state is recorded and not slashed. |
| **Decision 102:** the close's pre-charge is a base plus a rate per bond. The bonds are those at the attested root, plus one for each block of execution lag, because at most one bond merges per block. | Question 16, F47 | **F47 stays blocking until FX-16 implements it.** Check: the rate per bond covers the measured cost of one more bond at an epoch boundary. Lane B's `the_close_stays_inside_its_bound` must include rows at an epoch boundary with the largest bonded set that the test genesis allows. It must fail if a close passes the scaled bound. |
| **Decision 103:** one meter per wallet per block, shared by the block's parallel runs. Which run reaches a wallet's stop may differ between executions, and the trace records it. | Question 17, F49 | **F49 stays blocking until the meter is block-wide.** Check: lane C's per-run `MembraneMeter` limit becomes one allowance per wallet for the block. `accept_metered`'s W ≤ A check and the stop checks read the block-wide totals, not one run's totals. |
| **Decision 104:** a step is admitted together with the storage that it will cause. If the wallet cannot cover both, the step itself is refused. | Question 18, F50 | **F50 stays blocking until implemented.** Check: restore scopes, post-COMM charges and the event charge of a stored datum are admitted with their causing step, and no charge under a gated record runs without admission. |
| **Question 14:** the drawn amount must be exact, as r14 says. Lane C adds the two trace-alteration tests. | F45 | **F45 stays blocking until replay computes the exact running total.** Check: replay compares the recorded D with its own running total for the wallet at the refusal point, across the runs of the block (Decision 103). The two tests: a too-high D is rejected, and an admitted step changed into a refused one is rejected. |
| **Decision 105:** the close starts an epoch at the first block at or past the boundary that runs a close, by an epoch counter in the PoS state. | Condition 3 of the Q-33 review | **Answered for epochs.** FX-17 still needs a test: a void block at an epoch boundary, and the next block that runs a close starts the epoch. |
| **Decision 106:** the close is priced at the shard's price floor. | The price source for the pre-charge (FX-16, `7ac56f01a`) | **Answered.** Check at FX-16: the pre-charge and the close charge use the price floor from the on-chain shard configuration, in play, validation and replay. |

Questions 14 to 18 are answered. Lane A2 revises the Q-33 memo for Q2, Q3 and Q5, with the three answers of revision 18 included. A-8 reviews it when it lands.


## Revision 20 (action A-8): Q-33 revision 2, and the FX-17 epoch change

- **Documents and commits reviewed:**
  - `docs/rev-stacks/memos/Q-33.md` on `patch/Q-32` at `1d62fac6f` (lane A2, revision 2).
  - `patch/FX-17-epoch` at `214571b2e` (lane B, Decision 105).
- **Reference:** design r20 (publications `23fb421`) and Decisions 99 and 101 to 106.
- **Method:** reading only. No tests were run.
- **Not changed:** FX-8b (no `dev` merge), FX-15 (lane C), FX-15-replay, FX-15-settle, FX-16, and the fixes for F39 and F48.

### Q-33 revision 2 against the review

| Item | Result |
|---|---|
| Q1: the order floor commits on its own | **Met** (FX-17 paper, `d3df10dd3`). |
| Q2: gates and their floors | **Met.** The lag gate measures OF − CF. `max_unattested` measures OF − LFB. Floor pacing measures the newest block − OF. Each gate reads its floor once for each proposal, from one snapshot. Bound E of the stall analysis holds again. |
| Q3: void records | **Met, except for V4.** See below. |
| Q4: slashing | **Met** (Decision 101). `VoidBlock { reason, slashable }` is slashable only for V2 and V3. V1 and V4 are recorded and not slashed. |
| Q5: re-inclusion | **Met, subject to Meredith's ruling** on the consensus change that the memo asks for. An envelope in a void block counts as not included. The repeat check moves to execution (`Refusal::AlreadyIncluded`, decided from the canonical records before the position, so replay computes the same refusal). The inclusion screen uses the same rule. Status readers skip `Voided`. A void record charges nothing, and the block that runs the envelope charges it once. |
| Revision 18's three answers | **Folded in.** The order floor takes every guard of today's LFB. Each p* uses one bond set. "Finalized" means state-final. |
| Condition 1: each consumer to one floor | **Met** as a table. FX-17 adds a search for the call sites. |
| Condition 2: restart | **Met.** The crash test covers an order-floor commit without an LFB commit. |
| Condition 3: the close of a void block | **Met by Decision 105** and `patch/FX-17-epoch` (below). |
| Condition 4: wording | **Met.** |

**V4 — blocking for the void code (not for the start of FX-17):**
- **The memo:** `void_reason(block, pre)` is a function of the block and the canonical pre-state only, and replay computes it first.
- **Gap:** V4 (the close fails) is decided after the envelopes run. Under Decision 89, play is not deterministic, so the state before the close depends on the trace. So V4 is a function of the block, `pre` and the record's trace, not of the block and `pre` alone.
- **Required correction:**
  - Replay computes V1 to V3 first, from the block and `pre`.
  - Replay decides V4 only after it has replayed the record's envelopes and close.
  - The table "Replay checks in both directions" applies to V4 at that point: a V4 void record whose replayed close succeeds is rejected, and a normal record whose replayed close fails is rejected.
- **Required statement:** the memo must show that no envelope can make the close fail. If one can, a void caused by V4 returns that envelope to the pool, the envelope can then void each block that includes it again, and V4 is not slashable. In that case the causing envelope must not go back to the pool.
- **Reason for the label:** without the correction, the void predicate is not decidable as the memo writes it, and the replay check for V4 is undefined. That breaks Q3.

**Conditions on FX-17 (not blocking the sign-off):**
1. **V1's amount** must follow Decisions 102 and 106. The amount is a base plus a rate per bond, over the bonds at the attested root plus one for each block of lag, at the shard's price floor. The memo still says `close-precharge`. FX-17 must define "the blocks of lag" from data in the block and its linear position, so that every validator computes the same amount.
2. **Duplicates inside one block:** acceptance must keep the check that refuses one envelope signature twice in the same block. `repeat_deploy` skips v6 signatures only for carriers in other blocks.
3. **Deploy lifespan:** an envelope from a void block can expire before another block runs it. This is the normal expiry, so it is a known issue.

**Sign-off: given, with one exception.**
- Lane A2 can start FX-17 on the order floor, P5, the gates, the evidence flag, re-inclusion and the restart work.
- The void predicate's V4 case must be corrected as above before the void code lands.
- Q5's move of the repeat check to execution needs Meredith's ruling.

### FX-17 epoch change (lane B, `214571b2e`)

- PoSV6 keeps an epoch counter on a private channel. `closeBlock` peeks it, and starts an epoch when `blockNumber / epochLength` is greater than the counter. That close then writes the new index.
- `settlePendingBonds` still runs at every close.
- The test `an_epoch_starts_at_the_first_close_at_or_past_its_boundary` makes the boundary block void and checks that the next close starts the epoch. Each block replays.
- **Holds** against Decision 105. The cross-host fixture is written again.

**Lane B's known issue (slashing reads the epoch by arithmetic) is accepted, with a condition.** After a void boundary, PoS can be in the earlier epoch while `slashing_authorization` computes the later one. v6 runs no slashing today. The issue must be settled before B6 slashes any `VoidBlock` (V2, V3), because B6 then reads the epoch of the slashed block.

### Gate status

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | The design is signed off except for V4. It stays open until FX-17 lands. |
| Q-33 V4 | **Blocking** for the void code | New |
| F40, F45, F49, F50 | **Blocking** | Ruled. Open until the FX-15 merge meets revision 19 and the r20 rulings. |
| FX-15 merge requirement 4 (deploy-log refusal event) | **Blocking** | Open |
| D8b, D10, F39, F47, F48 | **Blocking** | Open |
| F38, F43, F44, the `for` form under `# P`, lane C's three known issues, storage growth and client latency under P11, the partial hold, the deploy-lifespan expiry of a void envelope, slashing epoch arithmetic | Known issue | Open |


## Revision 21 (action A-9): FX-15 under Decisions 103 and 104, and the FX-16 close charge

- **Commits audited:**
  - `patch/FX-15` at `e30313108` (lane C): one ledger for each block, and a COMM admitted with the storage it causes.
  - `patch/FX-16` at `a51dd79fc` (lane B): the close's PoS code charged to the proposer.
  - Documentation commits `efb0e3daf` (FX-17-epoch) and `085400b42` (FX-15-replay, FX-15 decision 11 under Decision 97).
- **Reference:** design r20 (publications `23fb421`).
- **Method:** reading only. No tests were run.
- **Not changed:** the Q-33 memo (V4 is still open), FX-8b (no FX-15 merge and no `dev` merge), FX-17 code other than the epoch branch, and the fix for F39.

### FX-15 at `e30313108`

| Item | Result |
|---|---|
| **F49 (Decision 103): one meter per wallet per block** | **Fixed on `patch/FX-15`.** One `BlockLedger` holds the gross drawn amount and the stop of each wallet. Every run of the block shares it, in play and in replay. Each run keeps only its own refusals, its share of the drawn amounts and its reached steps. Replay checks the refusals of all runs against the block ledger. The test `one_ledger_is_shared_by_the_runs_of_a_block` covers it. |
| **F45 (question 14): the drawn amount is exact** | **Fixed on `patch/FX-15`.** The drawn amount of every refusal must equal the wallet's total for the block. This holds because the stop is atomic across the runs, and replay refuses exactly the logged steps, which include compute steps as well as IO steps (decision 6). So replay reaches the same total. |
| **Trace-alteration tests** | **Present.** The region_spec test now checks five cases:<ul><li>The honest refusals replay to the same root, eight times.</li><li>No refusals: rejected.</li><li>D = 0: rejected.</li><li>D too high: rejected.</li><li>Refusals of steps that play admitted, from a block with half the room: rejected.</li></ul>The last two cover check (a) and the too-high D. |
| **F50 (Decision 104): storage admitted with its step** | **Fixed on `patch/FX-15`.** A send or a receive reserves its event storage when it pays its storage. A COMM reserves the COMM's event storage, the event storage of the side that fired it, and, for a persistent side, the storage and event storage of its re-store. If any wallet cannot pay, the COMM is refused. The reserved charges run in a prepaid scope. They are never refused, and they do not draw again. The reservation is a gross upper bound. Settlement charges the net work. Decision 8 (the exemption) is retired. Lane C's known issue 1 is retired. The test `a_comm_is_admitted_with_its_cells_and_the_storage_it_causes` covers it. |
| **F48: substitution into a fired continuation** | **Fixed for envelope runs, open for the close.** Lane C shows that the dispatcher evaluates a fired body inside the continuation's record, so substitution is charged there. The test `substitution_into_a_fired_continuation_is_charged_to_its_installer` makes a body substitute its argument 16 times. The installer pays more, and the firer's run costs the same as with an empty body. Lane B's measurement that found F48 was on the close path. `close_charge_spec::a_continuation_fired_in_the_close_charges_its_installer` asserts that the installer pays more, but it does not assert that the proposer's close charge is unchanged. **F48 stays blocking for the close** until that assertion is added and passes. |

**Merge requirement 1 is now decided by exactness.** The exact drawn amount depends on lane C's replay, which refuses exactly the logged steps, compute steps included. Lane A2's `ReplayStops` stops only at the storage boundary, so it cannot give an exact D. The merge must keep lane C's mechanism (`MembraneMeter`, `accept_metered`, the block ledger). It must also keep lane A2's check that each refused step is reached at its recorded cost, and lane B's `settled_work` (D − R).

### FX-16 at `a51dd79fc`

**What the commit does:**
- `CloseCharge { price, pre_charge }`, with `pre_charge = (CLOSE_BASE_PHLO + CLOSE_PHLO_PER_BOND × (bonds + lag)) × floor` (Decision 102).
- The price is the shard floor, read from PoS `getConsensusParameters` (Decision 106).
- After the close, the undecorated cells and work (the ledger's `None` key) are charged to the block sender's wallet. The charge is capped at the pre-charge and at the balance, and it is paid into the compute wallet inside the post-state root.
- The replay cache key includes the charge.
- Stored continuations that the close fires stay with their installers. Genesis charges nobody.

| Check (D10, Decision 97, test plan) | Result |
|---|---|
| 1. The proposer pays for known PoS work, and the rest of the pre-charge stays | **Present:** `the_proposer_pays_for_the_close_pos_code_and_the_rest_of_the_pre_charge_stays`. |
| 2. The block replays | **Present:** `the_close_charge_is_inside_the_post_state_root` (a charged record does not replay uncharged, and the reverse). |
| 3. A fired continuation charges its installer | **Present:** `a_continuation_fired_in_the_close_charges_its_installer`. |
| 4. An uncovered block is refused | **Moved to FX-17** (void V1, Q-33). |
| 5. A test fails if a close passes the bound | **Ignored** until F47 is ruled (below). |
| Price at the shard floor (Decision 106) | **Holds:** `the_close_is_priced_at_the_shard_floor`. |
| Bootstrap draws no charge | **Holds:** `genesis_charges_nobody`. |

**Two conditions for the merge with FX-17:**
1. **The charge is not wired in production.** Only tests call `BlockExecutor::with_close_charge`. FX-17 must build the `CloseCharge` for every block from consensus data: the bonds at the attested root, the lag as the void predicate defines it, and the floor read at a root that play, validation and replay share. FX-17 must also wire it into the production execute and replay paths.
2. **The cap at the balance.** This commit removes the gate hold, because the hold belongs to lane A2's void mechanism. Without the hold, envelopes of the same block can lower the proposer's balance before the close, and the cap at the balance then leaves close work unpaid. The merge must restore the Decision 97 hold at the gate, before any envelope, together with void V1.

### F47 — still blocking: a linear pre-charge cannot bound the close with the current PoS term

Lane B's measurement (FX-16 revision 2) shows two terms that grow about quadratically:
- The close at an epoch boundary, about 325·n² in the number of bonds n, from `pickActiveValidators`.
- An epoch that releases w withdrawers, about 1,370·w², from folds that carry the whole PoS state.

Decision 102's linear pre-charge therefore does not bound the close above about 20 bonds, and the work above the cap runs unpaid. This is the r20 §6.3 divergence of F47, now measured. **This audit agrees with lane B's label (blocking).**

Lane B's options are:
1. Make the PoS close linear (a native active-set pick, measured at about 4,300 phlo for each bond, and withdrawer folds that touch only the withdrawer maps).
2. Release at most K withdrawers in each close.
3. Add a quadratic term to the pre-charge.

That needs a ruling (question 19). Option 1 changes the PoS term and the genesis root. Option 2 uses Decision 105's deferral.

**Lane B's finding 2 (a shard floor of 0 makes the close free):** accepted as a **known issue.** Decision 106 sets the price, and a price of 0 charges 0. It is not a §6.3 divergence, because the rule charges the close at the floor.

### FX-17 epoch re-measure (`efb0e3daf`)

The peek of the epoch counter adds about 4,300 to 4,700 phlo to a close at 100 bonds. This is recorded in the FX-16 tables. No change to the revision 20 result.

### Gate status

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | The design is signed off except for V4. Open until FX-17 lands. |
| Q-33 V4 | **Blocking** for the void code | Open |
| F40 | **Blocking** | Implemented on `patch/FX-15`. Open until the merge into FX-8b. |
| F45, F49, F50 | **Blocking until merged** | Fixed on `patch/FX-15` |
| F48 | **Blocking** for the close | Fixed for envelope runs. The close needs the proposer-charge assertion. |
| F47 | **Blocking** | Measured. Question 19. |
| D10 | **Blocking** | Mechanism on `patch/FX-16`. Not wired in production, and the hold is removed until FX-17 restores it. |
| FX-15 merge requirement 4 (deploy-log refusal event) | **Blocking** | Open |
| D8b, F39 | **Blocking** | Open |
| F38, F43, F44, the `for` form under `# P`, lane C's remaining known issues, storage growth and client latency under P11, the deploy-lifespan expiry of a void envelope, slashing epoch arithmetic, a shard floor of 0 | Known issue | Open |


## Revision 22 (action A-9): FX-15 charging (F39), and the caps

- **Commits audited:** `patch/FX-15-charging` at `c345b400d` (lane B). It is cut from lane C's `fc3a6ca72` and merges FX-8b `4a74b0e34` at `83a4dd428`.
- **Not in this branch:**
  - Lane C's `e30313108` (the block ledger and storage admission).
  - Lane A2's `patch/FX-15-replay`.
  - Lane B's `patch/FX-15-settle`.
- **Reference:** design r20 (publications `23fb421`).
- **Method:** reading only. No tests were run.
- **Not changed:** the Q-33 memo (V4 is still open), FX-17 code, FX-16, FX-8b, and the `dev` merge.

### The meter meets FX-8b (`83a4dd428`)

**Holds** for revision 19's merge requirement 2. Lane C's `CostManager::charge` is kept:
- A gated record is admitted against its wallet and does not count against the client's limit.
- Every other charge counts against the client's limit.

`FOREIGN_WORK_BUDGET`, `set_own_origin` and the own-origin fields are removed. The constant bound of F40 is gone on this branch. Ungated foreign work, which is work of an owner that the gate committed, now counts against the client's limit. The envelope pays it, so this is correct.

### F39 (Decision 83)

**Fixed in settlement.**
- `own_payers` returns no payer for an own key whose owner the gate did not commit.
- `charges` skips that key, so the key does not use the client's budget.
- `surface_charges` draws the key from the owner's purse at its origin price, like any surface draw.
- A key with no record still goes to the client (Decision 81).
- This matches lane C's gate (`EnvelopeFunding::gates`), so admission and settlement now name the same wallet.
- The unit test `settlement_spec::an_own_region_whose_owner_the_gate_did_not_commit_is_charged_to_that_owner` covers the rule.

**End-to-end path: known issue.**
- Lane B tried three term shapes for a seal that the gate cannot resolve and that resolves at run time to another wallet. The gate refused each one as `ForeignSignature`. So no run is known to reach the F39 path.
- **Reason for the label:** the settlement rule is now correct for any key that reaches it, and no unpaid or misattributed charge is shown. This audit gives no term shape, because the audit works by reading.
- **Request to lane A2:** state whether the gate refuses every seal that it cannot resolve to a committed owner. If it does, record that as the gate's guarantee with a negative test, and F33 closes with it. If it does not, the lane that owns the gate writes the end-to-end test.

### Lane B's finding: two runs together overdraw a gated wallet

This is F49. Lane C's `e30313108` (revision 21) fixes it with one `BlockLedger` for each block, which Decision 103 ruled. That commit is not in this branch. So lane B's evidence test (`two_runs_that_each_fit_a_gated_wallet_but_not_together_pay_in_full`, ignored) shows the state before the fix.

**Merge requirement (added to revision 19's list):**
- When FX-15-charging and `e30313108` meet, the block ledger admits the gross draws of all runs against one allowance. The net work that settlement charges is at most the gross draw, so it cannot pass the wallet's free balance.
- Then remove the surface cap (`owed.min(free)` in `surface_charges`) and enable lane B's test. The test must pass with the cap removed.
- If the test fails, settlement still falls short, and that is a blocking §6.3 divergence.

### The close cap

Lane B keeps the cap in `charge_recorded_work` until lane A2 installs `CloseFunding`. **Agreed** as a merge step. The cap must be removed in the commit that installs `CloseFunding`, together with the Decision 97 hold (revision 21, FX-16 condition 2). Until then it is part of D8b (blocking).

### Gate status

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | The design is signed off except for V4. Open until FX-17 lands. |
| Q-33 V4 | **Blocking** for the void code | Open |
| F40 | **Blocking until merged** | The constant bound is removed on `patch/FX-15-charging`. |
| F45, F49, F50 | **Blocking until merged** | Fixed on `patch/FX-15` (`e30313108`). F49 needs the surface cap removed at the merge. |
| F39 | **Blocking until merged** | Fixed in settlement on `patch/FX-15-charging`. The end-to-end path is a known issue. |
| F48 | **Blocking** for the close | Open |
| F47 | **Blocking** | Question 19 |
| D10, D8b | **Blocking** | Open. Wiring, the hold and `CloseFunding` wait for FX-17 and the FX-8b integration. |
| FX-15 merge requirement 4 (deploy-log refusal event) | **Blocking** | Open |
| F38, F43, F44, the `for` form under `# P`, lane C's remaining known issues, storage growth and client latency under P11, the deploy-lifespan expiry of a void envelope, slashing epoch arithmetic, a shard floor of 0, the F39 end-to-end path | Known issue | Open |


## Rulings of 2026-11-04 (plan r8.23, design r21): Decisions 107 and 108, the naming convention, Q-34

Design r21 is not yet in publications (the latest commit there is `23fb421`, r20). This section records the rulings as the plan states them. A-9 reads the r21 text when it is committed.

**Naming convention, from this section on:**
- V1 … Vn are validators.
- u1 … um are deployers.
- d1 … di are deployments.
- B1 … Bj are blocks.
- The void reasons are (a) to (d). They were V1 to V4 in Q-33 revision 2 and in revisions 20 and 21 of this audit:
  - (a) The close is not covered.
  - (b) Slash records before B6.
  - (c) Terms that the gate cannot read.
  - (d) The close fails.

Earlier sections keep their text. Where they say V1 to V4, read (a) to (d).

| Ruling | Effect on the audit |
|---|---|
| **Decision 107:** the PoS close is made linear. It is flagged for review. | **Answers question 19 (F47).** F47 stays **blocking** until the linear close lands and lane B's `the_close_stays_inside_its_pre_charge` is enabled and passes, with rows at an epoch boundary at the largest bonded set of the test genesis and with an epoch that releases many withdrawers. The change touches the PoS term and the genesis root, so A-8 reads it in full when it lands. |
| **Decision 108:** the close's pre-charge is evaluated only at execution, on the block's true pre-state. There is no check at acceptance. An uncovered close makes the block void for reason (a). That other deployers can cause such voids is a recorded open issue. | **The D10 test plan changes.** Test 4 ("a block whose proposer cannot cover the pre-charge is refused at acceptance") is withdrawn. In its place: a block whose proposer cannot cover the pre-charge at its true pre-state is void for reason (a), and replay accepts the void record and rejects a normal one. The acceptance check (`check_close_cover`) and the matching check in `create_block` must be removed. The Decision 97 hold at the gate, before any envelope, must be restored, so that the block's own deployments cannot drain the proposer before the close (revision 21, FX-16 condition 2). The griefing case is a **known issue** by ruling. |

**Q-34 (Meredith's void-attack questions) is added to lane A2's memo revision.** A-8 reviews the revision when it lands, and checks the following:
1. **Identifiable.** A void block stays in the justification structure by its hash. "Void" is a property of the canonical record, checked in both directions in replay and attested.
2. **Reason (c).** A deployment that causes reason (c) is discarded, not returned to the pool.
3. **Reason (d).**
   - Replay decides (d) only after it replays the record's deployments and close (revision 20).
   - The memo proves, with a test, that no deployment can make the close fail. If it cannot prove this, a deployment that has been through one void block is dropped.
4. **Counting.** A count of voids for each proposer is kept.
5. **Griefing.** The Decision 108 case is stated as an open issue.
6. **Earlier conditions still apply:**
   - The amount for reason (a) by Decisions 102 and 106.
   - The check against one deployment twice in one block.

Meredith rules on the repeat check moving to execution (`AlreadyIncluded`) after that revision.


## Revision 23 (actions A-8 and A-9): Q-33 revision 3 (Q-34), the FX-15 integration into FX-8b, and Decision 107

- **Documents and commits reviewed:**
  - `docs/rev-stacks/memos/Q-33.md` on `patch/Q-32` at `191f74ccb` (lane A2, revision 3).
  - `patch/FX-8b` merge `efa511e93`, with parents `4a74b0e34` and `02f70434e`. The FX-15-replay chain includes `b0fe54608` (lane C's `e30313108`), `cc4df0211` (lane B's `c345b400d`), `5e92b0d6b` (surface cap) and `02f70434e` (FX-16, Decision 108, the close).
  - `patch/FX-17-epoch` at `dcb3ce695` (Decision 107) and `patch/FX-16` at `bee4c599f` (the re-measured bound).
  - `patch/FX-15` at `432dc390f` and `patch/FX-15-close` at `34e6c842d` (F48 in the close).
- **Reference:** design r21 (publications `81599ff`). r21 adds Decisions 107 and 108, the naming convention, and the open issue of Decision 108 to §6.3.
- **Method:** reading only. No tests were run.

### Q-34 against the review list ("Rulings of 2026-11-04")

| Item | Result |
|---|---|
| 1. Identifiable | **Met.** A void B stays in the DAG and in every justification by its hash. "Void" is the outcome and reason in B's canonical record entry. Replay checks it in both directions, and P5 attests the post-root at B's position, which for a void B is the pre-root. |
| 2. Reason (c) | **Met.** The deployment is discarded and is not returned to the pool. (c) is slashable, because the proposer could see the terms (Decision 101). |
| 3. Reason (d) | **Met.** Replay accepts a record with reason (d) only after it runs the record's deployments and the close, and the close fails. The memo gives an argument and a test (`close_charge_spec::no_deployment_makes_the_close_fail`). It states that this does not prove (d) unreachable for every PoS state. So the drop rule applies: a deployment that has been through one void block for reason (d) is dropped. (d) is not slashable. |
| 4. Counting | **Met.** A count of voids for each proposer, by reason, is derived from canonical records only, persisted with them, rebuilt on restart, and exposed as a metric and to B6. |
| 5. Griefing | **Met.** The Decision 108 case is stated as an open issue: stored receives that the proposer installed and that others fire, and transfers that the proposer authorized earlier. r21 states the same issue. |
| 6a. Reason (a) amount | **Met in the memo.** The hold is FX-16's `CloseCharge::pre_charge` (Decisions 102 and 106). The production wiring is an FX-17 condition (below). |
| 6b. One deployment twice in one block | **Not addressed.** This stays an FX-17 condition from revision 20. |

**The test found and fixed a High safety defect.** The close ran without seal records, so a continuation that the close fired ran with no record and was not gated. An endless loop then used memory until the process was killed. `SystemGate::apply` now turns seal records on for the gated close (FX-15-replay decision 18). **Closed.**

**Two observations on the drop rules:**
1. **Known issue.** A void for reason (d) drops every deployment of the block, including deployments that did not cause it. They are not charged, and their deployers can submit them again. (d) is not expected to occur, and dropping them has no authority effect.
2. **Known issue.** "Dropped" and "discarded" act on each node's pool. A proposer that includes such a deployment again makes only its own block void again, and the blocks above it run normally. If the same rule is wanted across proposers, the gate can refuse a deployment that appears in an earlier canonical record that is void for reason (c) or (d), as it does for `AlreadyIncluded`. That belongs with Meredith's ruling on `AlreadyIncluded`.

**Q-34 sign-off: given.** Meredith can rule on `AlreadyIncluded`. The FX-17 conditions of revision 20 stand: the within-block duplicate check and reason (a) from consensus data.

### The FX-15 integration into FX-8b (`efa511e93`)

| Merge requirement (revisions 19, 21, 22) | Result |
|---|---|
| One replay mechanism, the exact drawn amount | **Holds.** Lane C's meter and `BlockLedger` are taken whole (FX-15-replay decision 11). Replay refuses exactly the logged steps. Lane A2's reached-charge check is kept. |
| Lane B's `settled_work` | **Replaced, and accepted.** FX-15-replay decision 12 drops D − R. Settlement requires replay to reach the recorded work of each stopped surface. Replay is now exact, and under Decision 104 the drawn amount includes reserved storage, so D − R is no longer the net work. F46 stays closed. |
| `FOREIGN_WORK_BUDGET` removed | **Holds** (from `c345b400d`). |
| The surface cap removed with the block ledger, and lane B's two-run test enabled | **Holds** (`5e92b0d6b`). `two_runs_that_each_fit_a_gated_wallet_but_not_together_pay_in_full` is not ignored. A draw above what remains is an error. |
| The deploy-log refusal event | **Holds.** `replay_recorded` reads each run's refusals from its deploy log (`stops::refusals_of`), so `BlockExecutor` replay passes them. Merge requirement 4 is met. |
| `CloseFunding` installed, the close cap removed in the same commit | **Holds** (`02f70434e`, decision 16). |
| D8b: the close's COMM cells | **Holds** (decision 17). `handle_result` records the cells of each COMM, and the close charges the cells of a stored record to its installer, with its work. **D8b is closed** on the integration branch. |
| Decision 108: acceptance checks removed | **Holds** (decision 15). `check_close_cover` and the `create_block` check are gone. |
| F39 | **Holds** (from `c345b400d`). Closed on the integration branch. The end-to-end path stays a known issue. |
| Full suites for the merge | **Not posted.** The FX-15-replay log gives v6 only (100 passed, 0 failed, 10 ignored). Lane A2 must post the full suites before the handoff. |

**Status on FX-8b at `efa511e93`:**
- F40, F45, F46, F49, F50, F39 and D8b are **closed** on the integration branch.
- FX-15 merge requirement 4 is **closed.**

**D10 is still open in production.** `BlockExecutor::with_close_charge` has no caller outside tests, so `close_precharge` is 0. Then the gate holds nothing, and the close is not charged. FX-17 must build the `CloseCharge` for each block from consensus data (the bonds at the attested root, the lag, and the shard floor) and pass it in play, validation and replay, together with void reason (a).

### F48 in the close

- Lane C found the cause. With the storage rule off, `handle_result` credited every storage refund of a COMM to the record that fired it. A continuation's consume refund grows with its body, so the close's PoS code received a refund for storage that the installer had paid.
- `432dc390f` credits each refund to the payer of the stored item, as `left_in_space_match` already does.
- Tests:
  - `seal_wiring_spec::a_storage_refund_at_a_comm_goes_to_the_payer_of_the_stored_item`.
  - `close_charge_spec::substitution_into_a_continuation_fired_in_the_close_is_not_charged_to_the_proposer`, on `patch/FX-15-close`.

**F48 is fixed on `patch/FX-15` and `patch/FX-15-close`.** It closes when they merge into FX-8b. `432dc390f` is not in FX-8b.

This change also narrows **F38** (refund keying). A storage refund at a COMM now goes to the payer of the stored item in both paths. F38 stays a known issue for refunds outside a COMM.

### Decision 107 (`dcb3ce695`, flagged for review)

- **`pickActiveValidators`** is now `allBonds.keys().toList().take(N).toSet()`, in place of `ListOps.map` with `fst` over `allBonds.toList()`.
  - Both forms take the first N keys in the map's sorted key order. So the active set does not change, provided `keys().toList()` and `toList()` order the keys alike.
  - **Condition:** a test that asserts the same active set from the old and the new form, for a bond map larger than N. The existing tests check the set for small maps only.
  - The genesis root changes, and the cross-host fixture is written again.
- **An epoch close releases at most two withdrawers** (`quarantined.take(2)`). `an_epoch_close_releases_at_most_two_withdrawers_and_the_next_epoch_takes_the_rest` covers it.
- **The loop inventory** (`docs/rev-stacks/memos/D107-loops.md`) is a reading of every genesis contract. It names the loops that carry state by value. Only PoSV6's loops run in the close. The others are paid by the deployer that calls them.

### F47 after Decision 107 (`bee4c599f`)

- **Bonds:** `close_bound_spec::the_close_stays_inside_its_pre_charge` is enabled and passes. Every measured close, with 1 to 200 bonds and epoch lengths 1 and 1,000, is under 200,000 + 9,000 × bonds, with a margin of about 2 at 100 bonds and above. **Closed for bonds.**
- **Withdrawers: still blocking.** `movePendingWithdrawer` folds over every pending withdrawer and carries the whole PoS state. `removeQuarantinedWithdrawers` walks every withdrawer in quarantine. The pre-charge counts bonds, and withdrawers held back by the cap are no longer bonds. Lane B measures:
  - Epoch 1 with 10 withdrawers: 1.71 million phlo against a pre-charge of 1.1 million.
  - Epoch 2 with 99 held back and 1 bond left: 9.6 million against 209,000.

  The work above the pre-charge runs unpaid, which is a §6.3 divergence. **This audit agrees with lane B's label (blocking).**
- **Lane B's options:**
  - The withdrawer rewrite (a release schedule keyed by epoch, with native map operations), about 2.5 days.
  - Or interim caps on moves and on the quarantine window, with withdrawers in quarantine counted as bonds for the pre-charge.

  The choice is Meredith's (question 20). `an_epoch_with_many_withdrawers_stays_inside_its_pre_charge` is the test, and it is ignored until one option lands.

### Gate status

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | The design is signed off (Q-33 revisions 2 and 3). Open until FX-17 lands. |
| D10 | **Blocking** | The mechanism is on FX-8b. It is not wired in production. FX-17 builds the `CloseCharge` from consensus data. |
| F47 (withdrawers) | **Blocking** | Question 20 |
| F48 | **Blocking until merged** | Fixed on `patch/FX-15` and `patch/FX-15-close` |
| Full suites for the FX-15 merge | Merge condition | Not posted |
| `dev` merge into FX-8b | Watch item | Not merged. Lane A2's dry-run memo is on FX-8b. |
| F40, F45, F46, F49, F50, F39, D8b, FX-15 merge requirement 4, the gated-close seal-records defect | — | Closed on FX-8b |
| F38 (refunds outside a COMM), F43, F44, the `for` form under `# P`, lane C's remaining known issues, storage growth and client latency under P11, the deploy-lifespan expiry of a void deployment, slashing epoch arithmetic, a shard floor of 0, the F39 end-to-end path, the Decision 108 griefing case, the two drop-rule observations above | Known issue | Open |


## Revision 24 (action A-9): FX-15 charging revision 2 (the F39 negative test)

- **Commit audited:** `patch/FX-15-charging` at `dea48f13d` (lane B), with lane C's `e30313108` merged in at `42ecf44bc`.
- **Reference:** design r21 (publications `81599ff`).
- **Method:** reading only. No tests were run.
- **Not changed:**
  - The Q-33 memo.
  - FX-8b (`efa511e93`), with no `dev` merge and no full-suite results.
  - FX-15, FX-15-close, FX-16 and FX-17-epoch.

  No FX-17 code has landed, and there is no ruling yet on `AlreadyIncluded` or question 20.

**What the commit does:**
- It removes the surface cap and enables the cross-run test. FX-8b already did the same in `5e92b0d6b`, so this adds nothing to the integration branch.
- **It adds a negative test for F39's run-time path.** `region_spec::the_gate_refuses_a_seal_that_resolves_only_at_run_time` asserts that the gate refuses (`ForeignSignature`) the three term shapes that lane B tried.

**Result:**
- The test records what the gate does for three shapes. It does not show that the gate refuses every seal that resolves only at run time. So F39's end-to-end path and F33 stay **known issues** until lane A2 states that guarantee (revision 22).
- **Merge requirement:** this commit is not in FX-8b. Bring the negative test into FX-8b with the F48 merge, so that the integration branch keeps the record of the gate's behavior.

The gate status of revision 23 does not change.


## Revision 25 (action A-8): the FX-15-close merge into FX-8b

- **Commit audited:** `patch/FX-8b` merge `f7af68ca8`, with parents `efa511e93` and `34e6c842d` (`patch/FX-15-close`). It brings in:
  - `patch/FX-15` at `432dc390f` (a storage refund at a COMM goes to the payer of the stored item).
  - FX-16 at `bee4c599f` (the bound test on the linear close).
  - Lane B's epoch fix and Decision 107 (`efb0e3daf`, `dcb3ce695`).
  - Lane C's close substitution test.
- **Reference:** design r21 (publications `81599ff`).
- **Method:** a reading of the merge and its log. No tests were run.
- **Not changed:**
  - The Q-33 memo.
  - Lane B's `dea48f13d` (the F39 negative test, still not in FX-8b).
  - FX-17 code.
  - The `dev` merge.

  There is no ruling yet on `AlreadyIncluded` or question 20.

| Check | Result |
|---|---|
| The client's limit has no foreign-work budget | **Holds.** Lane A2 kept `CostManager::charge` as on `efa511e93`. The auto-merge had brought back `own_origin` and the `foreign` budget. On `f7af68ca8` the tree has no `FOREIGN_WORK_BUDGET`, `set_own_origin` or `own_origin` in `rholang/src` or `casper/src`. |
| F48 in the close | **Closed on FX-8b.** `handle_result` credits each storage refund at a COMM to the payer of the stored item. Two tests come in with the merge: `seal_wiring_spec::a_storage_refund_at_a_comm_goes_to_the_payer_of_the_stored_item` and `close_charge_spec::substitution_into_a_continuation_fired_in_the_close_is_not_charged_to_the_proposer`. `no_deployment_makes_the_close_fail` is kept beside them. |
| Decision 107 and the epoch fix | **In FX-8b.** The test that the old and new active-set pick agree for a bond map larger than N (revision 23) is still not added. **Condition.** |
| F47 for bonds | **Closed on FX-8b.** `the_close_stays_inside_its_pre_charge` runs. |
| F47 for withdrawers | **Blocking.** `an_epoch_with_many_withdrawers_stays_inside_its_pre_charge` is ignored. It waits for question 20. |
| Full suites | **Not posted.** The FX-8b log gives v6 only (104 passed, 0 failed, 10 ignored), plus `seal_wiring_spec` (3) and `rev_bond_spec` (9). |
| The F39 negative test (`dea48f13d`) | **Not merged.** The merge requirement of revision 24 stands. |

### Gate status on FX-8b at `f7af68ca8`

| Item | Label | State |
|---|---|---|
| D11 (P11 seam) | **Blocking** | Open until FX-17 lands |
| D10 | **Blocking** | Not wired in production. FX-17 builds the `CloseCharge` from consensus data. |
| F47 (withdrawers) | **Blocking** | Question 20 |
| Full suites | Merge condition | Not posted |
| The active-set equivalence test, the F39 negative test | Merge conditions | Open |
| `dev` merge | Watch item | Not merged |
| F39, F40, F45, F46, F48, F49, F50, D8b, F47 for bonds | — | Closed on FX-8b |
| F38 (refunds outside a COMM), F43, F44, the `for` form under `# P`, lane C's remaining known issues, storage growth and client latency under P11, the deploy-lifespan expiry of a void deployment, slashing epoch arithmetic, a shard floor of 0, the F39 end-to-end path, the Decision 108 griefing case, the two drop-rule observations | Known issue | Open |


## Open questions for review

1. **F6:** answered on 2026-10-31 with the seal reading (b). Every draw on a wallet must sit inside a region that the wallet's owner sealed. F14 to F17 use this ruling.
2. **F4:** should a cosignature cover the envelope (the deployer key and the deploy signature, or the hash of the whole signed envelope less the cosignatures)? Binding to the deployer key is enough to stop reuse under another deployer. Binding to the deploy data alone is not.
3. **F7:** should release and slashing move behind PoS-held unforgeable names, as §6.9 says, with the system auth token kept only for the system deploys' own calls into PoS?
4. **F14 and F15:** may the payer of a stack come only from the enclosing seal, with the deployer no longer a default? This changes charging and the state root of blocks that hold stack literals.
5. **F17:** may a region that A sealed in A's envelope run later in an envelope that A did not sign? Should the record of a region's origin be a field of `CostSignedTerm` or a wrapper that only the reducer can build?
6. **F16:** should the hook's spending authority be the seal `# H` of the installed contract, the compound seal of the hook authority, or both?
7. **F27:** answered on 2026-11-02 (Decision 63). Should the deployer's signed message name the required cosigner keys, so that a relay cannot strip cosignatures from an envelope without changing its identity?
8. **F31:** answered on 2026-11-02 (Decision 64). Until region metering, should a draw whose writer is a `Process` owner be refused, or journaled as pro-rata share draws?
9. **Q-31 / D76:** answered on 2026-11-03 (Decision 80: the same owner and the same origin). For Decision 76, does "surfaces under one seal" mean one owner, or one owner at one origin envelope and price?
10. **D79 decision 5:** answered on 2026-11-03 (Decision 85: two cells, both charged to the client). A side with no record and the client's own record are two keys, so a COMM between the client's send and genesis code counts two cells. Is one cell the intent of Decision 80 for this case?
11. **F40:** answered on 2026-11-03 (Decision 86: work under the membrane stops). When a funded stored receive fires a continuation whose work costs more than its purse holds, who pays the shortfall?
12. **F39 and F33:** answered on 2026-11-03 (Decision 83: the wallet at the seal pays). Is an own key whose owner the gate did not commit charged to the client, refused at the gate, or added to the settlement check?
13. **D8b:** answered on 2026-11-03 (Decision 94: the close is charged, its undecorated code to the proposer and stored continuations to their installers). Is the close-block system deploy exempt from the one-cell charge of r12 §6.3, or must a COMM that the close fires pay its cells?

14. **F45 and F46:** answered on 2026-11-03 (the drawn amount must be exact, as r14 says). Replay does not reproduce the running total of a stopped membrane exactly, because interleavings differ before the next IO step. Must replay compute the exact running total at each refusal, as r16 §6.3 says? Or does the design admit a bounded window for the drawn amount and for the recorded work, and if so, what bound?
15. **Q-33, Q4:** answered on 2026-11-03 (Decision 101). May a block be voided without slashing its sender, when its invalidity depends on the pre-state at its linear position, which the proposer could not see? Or is `VoidBlock` slashed only for invalidity that the proposer could decide from its own justifications?
16. **F47:** answered on 2026-11-03 (Decision 102). The close's work at an epoch boundary grows with the number of bonds. Is the bonded set capped, or does the close's pre-charge scale with the number of bonds?
17. **F49:** answered on 2026-11-03 (Decision 103). The meter admits each step against what t drew in its own run. Parallel runs in one block can then each draw the same wallet up to its whole free balance, and settlement caps the total. Is the meter block-wide, is the wallet's balance split between runs, or does the settlement cap across runs stand?
18. **F50:** answered on 2026-11-03 (Decision 104). Are storage bookkeeping charges under a gated record (restores of persistent items, post-COMM charges, the event charge of a stored datum) admitted with the step that caused them, or exempt from the gate?
19. **F47 (measured):** answered on 2026-11-04 (Decision 107: the PoS close is made linear). The PoS close grows about quadratically in the bonds at an epoch boundary, and in the withdrawers that one epoch releases. Is the PoS close made linear (a native active-set pick, and withdrawer folds over the withdrawer maps only), is the number of releases in each close capped, or does the pre-charge gain a quadratic term?
20. **F47 (withdrawers):** the PoS close still grows with the withdrawers of one epoch, and the pre-charge counts bonds only. Is the withdrawer loop rewritten now (a release schedule keyed by epoch, about 2.5 days), or do interim caps on moves and on the quarantine window apply, with withdrawers in quarantine counted as bonds for the pre-charge?

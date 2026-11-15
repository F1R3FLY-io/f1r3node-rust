# Leaky-bucket spec — amendments and corrections

For the author of the leaky-bucket spec (v0.1, branch `leakybucket`). The spec
document is not edited: each entry says what our build does, and asks the
author where the spec leaves a choice. Checked against `dev` at `5ff390e15` and
#216 at `fe6cce76e`; built on local `dev` through `c1cfb6e6f`, which merges
`dev` at `c8794b9df`.

**Amendments**: the spec is silent or contradicts itself. **Corrections**: it
misstates `dev`.

## Amendments

### Forced by a spec defect

The spec contradicts itself or is unsound here.

**A1 — two quantities are stated in two units.** *The leak and the waiting
period before an ejection are counted in blocks this node has seen, because the
spec gives them in time in one place and in blocks in another.* §3.3 and §3.5
measure `λ` and `Δ_eject_observation` in wall-clock time; §12 in blocks.
**Done:** blocks, as §12 states, counted as this node's observations: block
numbers are not monotone in admission order (verified by test), and elapsed
time cannot be rebuilt after a restart. §9.1's `last_update_ts` then holds
nothing. **Question:** are `λ` and `Δ_eject_observation` in blocks, as §12
states?

**A2 — §3.6 debits `B` from every honest view, including one holding less.**
*A view that credited less than a paid claim takes the difference out of what
it credits later, so every view is debited exactly what the chain paid.* §3.6
debits `o_V^U` by `B` for every honest `U`, a nay voter included, and §9.1
types overflow as unsigned. **Done:** a view holds overflow as what it
credited less what was paid since it began crediting the validator, never
below zero. Each view is then debited exactly what was paid (§8.3), whatever
order a node learns of credits and payments in.

**A3 — §3.7's topup becomes everyone's reward on `dev`.** *A rejoin topup is
held to pay overflow claims, because `dev` would otherwise share it out as
epoch reward.* §3.7 pays the topup into `posVault`, and §3.9's reward pool is
fees and mint only. On `dev` everything the vault holds beyond bonds,
withdrawals and committed rewards is the epoch's reward, so the next close
shared the topup among the committee (verified by test). **Done:** paid
topups accumulate in a PoS total, `heldTopups`, that the reward computation
leaves out, and successful overflow claims are paid from it (A20).

**A4 — §3.11's retention window is not derived from what the rules read.**
*How much history every node keeps is computed from the chain's own
parameters: the furthest back any rule must look to judge a block.* §12 gives
"≈ 1 week" with no derivation, and §3.11 asks for the span over which
misbehaviour can still be proven. **Done:** `block_retention_window =
max_parent_depth + max(deploy_lifespan, Δ_claim_deadline, epoch_length)`
(10 015 at shipped values), derived on every node; a restore fetches back
through it.

**A5 — §9's floating-point types cannot implement §3.14.** *All reward and
bucket arithmetic uses whole numbers, because every node must reach exactly the
same result when judging slashing evidence, and floating point cannot guarantee
that.* Adjudication compares a recomputed `Σ R(B)` exactly, so every node must
reach the same bits: floating-point sums depend on walk order, and
`(1 − x)^p` with a real `p` is not exactly specified. **Done:** integer
arithmetic in ppm; level and overflow in `1/PPM` of a bond unit (`u128`);
`p_breadth` an integer ≥ 1, narrowing §12's `Real`. §9.1's entry also lacks the
below-threshold counter §3.5 needs. **Question:** may the arithmetic be
integer, with `p_breadth` an integer?

**A6 — §11.2 leaves claim effects out of block creation.** *The proposer
applies each claim its floor decides and validation checks it did, because a
block's post-state is fixed when it is signed.* §11.1 has validation "apply the
claim's effect … to the post-state", and §11.2 gives creation no matching step.
**Done:** the proposer carries each decided claim and its outcome; validation
recomputes them and requires an exact match.

#### The §3.14 chain

**A7 — §3.6 and §3.7 change every view on success, but on `dev` a success
can go unpaid.** *A view refills a rejoining validator, and takes a paid claim
off its overflow, only from what the chain records as paid.* §3.7 refills `V`'s
bucket when its rejoin passes, and §3.6 debits every view when a claim passes.
A pass can go unpaid (A20, A22), so a resolution does not say what was paid.
§3.14 also subtracts an on-chain sum of payments that §9.3 never defines.
**Done:** a paid rejoin closes `freezeHistory[V][F] = {to, topup}` (A8), and a
view refills once, before the first block numbered above `to`. Each payment is
recorded in `claimedOverflow[V] = {total, payments}`, keyed by the paying
block, so a view that begins mid-history takes off only the payments inside
the span it credits.

**A8 — §3.5's freeze over `[F, F + Δ_grace]` cannot be applied as stated.**
*Every view freezes an ejected validator over the same blocks, including any
block it signs after `F`.* `F` precedes the ejecting block, so views have
already drained blocks above it (verified by test). Block numbers alone leak:
a block built on old parents is numbered below `F` and refilled the bucket
(verified by test). **Done:** the freeze covers blocks from `F` through the
paying block, recorded on chain (A7), plus the validator's own blocks with a
sequence number above its last one at `F`. A frozen block neither drains nor
credits, and a view replays when it learns of a freeze.

**A9 — §3.14 does not say under which record a vote is judged.** *A nay
carries the record its block's floor held, and is judged against that.*
Freezes and payments change as floors advance, and an adjudicator cannot read
an old floor's state (verified by test). **Done:** a nay on an overflow claim
carries its floor's amount, paid total and freezes. Validation requires them
exactly, and the bound (A10) is computed from them.

**A10 — §3.14's lower bound on `o_W^V` is not a lower bound.** *A nay is
slashed only against overflow every honest view must hold.* Tests show honest
views holding less than the bound: a bucket absorbs far more than `C(W)` over
time, a frozen block credits nothing, a restored node holds only the window,
and a windowed payment total falls short. **Done:** the bound is
`Σ max(0, R(P) − C_P(W)) − ClaimedOverflow_W`, over W's valid blocks in the
vote block's ancestry, within the reach (A12) and outside the vote floor's
freezes (A9). `C_P(W)` is W's stake in `P`'s bonds. A 4 096-case proptest
finds it never above an honest view.

**A11 — §3.14's rule for dishonest `EjectV` yeas cannot be made sound.** *A
yea to eject a validator is never slashed, because no public evidence can prove
it dishonest.* The rule calls a yea dishonest when the voter's block builds on
a fresh W-block worth more than `(1 − θ_eject) · C(W)`, but from an empty
bucket, reaching the threshold takes `θ_eject · C(W)`. No reward size works
anyway. The voter's view drains with every block it accepts before voting, and
nothing bounds how many, not even block numbers (verified by test). **Done:**
omitted. §6.4's Attack C is then held off only by the 2/3 threshold, and a
wrongful ejection is reversed by a rejoin.

**A12 — §3.14's evidence cannot travel `dev`'s slashing pipeline.**
*Dishonest-vote evidence is its own system deploy, judged by every node
against block ancestry.* `dev` authorizes a slash only on a current-epoch
invalid block (correction 13). The spec's `testimony_block` and `accuser`
duplicate the vote block and the signer, and its closure needs a bound.
**Done:** `DishonestVote {accused, vote_block, claim}`. A block whose evidence
proves nothing is invalid, and a node missing a block the evidence needs
fetches it before judging. Counted blocks reach `block_retention_window −
2·max_parent_depth − 1` below the higher of the two floors. A proposer checks
its own nays the same way and fetches any block the check needs before
signing, so a node missing part of its history may not propose until it holds
that block or the claim's deadline passes. Until Component 4 exists, no credit
absorbs a dishonest-vote slash (§3.8, §7.3).

**A13 — §3.3 drains in proportion to stake, but §12 pays a flat reward.** *A
block credits its proposer a share of its own stake, so a validator's level does
not depend on its bond.* §3.3 drains `λ · C(V)` per block while §12 splits
`R_base` equally, so no single emission holds every bond: at the old values,
honest validators of stake 500 to 10 000 drained within three rounds while one
of stake 100 held (verified by test). **Done:** `R = f_b · f_r · ρ · n · C(V)`,
`ρ` a per-block rate replacing `epoch_phlogiston`. A member proposing a fraction
`p` of its share holds iff `p · q · ρ ≥ λ`, whatever its bond, the committee's
size or the total stake (verified with mixed bonds, a joiner and a leaver).
**Question:** is a reward proportional to stake what §12 intends?

**A14 — §3.4's breadth does not measure breadth.** *Breadth is the stake share
of the other members whose new work a block acknowledges since its proposer's
previous block.* Read as written, "normalized so that `Σ w_i = 1`" makes breadth
1 for every block, so citing more pays nothing, and the Key Invariant's
`f_breadth(1) · f_recency(1)` holds under no reading. Read as a share of total
stake over the block's parents, it fails the other way: in a chain built one
block at a time every block has a single parent, so breadth is the share of
whoever proposed last (verified by test). **Done:** breadth counts the members
whose justification moved since the proposer's previous block, each counted
once, as a share of committee stake. That previous block counts only within
`max_parent_depth + 1`, so every node computes the same reward. A validator
producing while every other member is silent earns nothing and drains too;
ejection still needs two thirds of the electorate. **Question:** should
breadth measure acknowledged work rather than parents?

**A27 — An unbonded voter cannot vote but still counts (§11.1 vs §10,
§3.13).** *A member with no seat that has not voted is left out of the
tally.* Otherwise its stake is a permanent nay, and a paid rejoin can fail
(verified by test). **Question:** drop such stake from the denominator, or
give unbonded members a vote that needs no block?

### Filling a gap the spec leaves

Each is put to the author as a question, unless the spec's own rule settles it.

**A15 — the initial bucket level is undefined.** *A validator the node starts
tracking begins full, so it cannot be accused before it has had a chance to
produce; one first seen while ejected begins at the threshold its ejection
found it below.* §3.2 inserts on first observation; §9.1 names no starting
value. Starting at 0 makes a new member ejectable without having failed.
Starting an ejected member full backs any rejoin topup, though its ejection
showed it drained (verified by test). **Done:** start at capacity, except
inside a freeze, where it starts at `θ_eject · C(V)`. **Question:** what level
should a new bucket start at?

**A16 — §3.2 inserts every bonded validator, including one outside the
committee.** *Only validators seated on the block's committee get a bucket,
because a bonded validator without a seat cannot produce blocks and would be
punished for something it cannot do.* On `dev` the committee is
`allBonds ∩ activeValidators` (correction 1) and the active set is capped. A
bonded validator outside it cannot author a valid block, so its bucket can only
drain, while §3.9, §3.10 and §7.2 leave it unpaid, voteless and uncredited.
**Done:** insert on the block's own `bonds` field, which equals `allBonds`
while the cap does not bind; a validator enters full when first seated (A15),
not when it bonds. **Question:** does the spec assume bonded == active?

**A17 — nothing says what a bond change does to the level.** *When a
validator's stake changes, its bucket scales with it, so changing stake neither
pushes it towards ejection nor rescues it from one.* `C(V) = bond(V)` changes,
and §3.3's `min(R, C − b)` is undefined once `b > C`. Carrying the level
unchanged breaks §3.3's scale-free threshold: a larger bond pushes a validator
towards ejection, and a smaller one refills a draining validator. **Done:** the
level scales with capacity, so `b / C` is unchanged; overflow is untouched.
**Question:** should the level keep its share of capacity when the bond
changes?

**A18 — §3.5's `proposer_view_snapshot` is partly undefined.** *An ejection
request carries no hash of the accuser's view, because the spec leaves part of
what that hash covers undefined.* `recent_block_witnesses` and the "…" are
never defined, nor who keeps the contents. **Done:** omitted; no outcome
changes, but no accuser is on record with its view. **Question:** what do they
hold, and who keeps them?

**A19 — `round: EpochRoundNumber` has no stated use.** *Claim requests carry
no round number, because the spec gives the field no use and the chain already
knows each request's epoch.* §3.5–§3.7 list it, and no rule reads it or defines
its type. Deploy signatures and lifespans already prevent replay, and the
opened-at block tells claims apart. **Done:** omitted. **Question:** what is it
for?

**A20 — §3.6 pays a claim by minting, and `dev` has no mint.** *A successful
overflow claim is paid only if the PoS vault holds the money.* §3.6 mints `B`
on success, offset in §11.4 against an epoch mint; `dev` mints nothing
(correction 3). **Done:** a successful claim is paid from held topups (A3),
then the epoch's undistributed fees; one they cannot cover goes unpaid, owes
no fee, and may be claimed again. **Question:** should Component 0 add the
mint §11.4 describes?

**A21 — §3.6's failed-claim fee has no size or source.** *A failed overflow
claim costs its claimant a share of its bond, because the spec names the fee
but not its size or the account it comes from.* §12 lists no parameter, and
"from `V`" names no account. **Done:** a genesis parameter in ppm of the bond
(placeholder 1 000), rounded up, moved from the bond, or the quarantined bond,
to the coop vault, as a slash moves stake. "Failed" (§3.6) covers a claim
refused by more than 2/3 nay stake and one expiring undecided. **Question:**
what size, and from which account?

**A22 — §3.7 says nothing of a rejoin its validator cannot pay.** *A rejoin
that passes but cannot be paid fails and leaves its validator ejected, because
the spec reinstates a validator only when its topup is paid.* The rejoin
resolves in a later block, and `V` can empty its vault in that same block
before the resolution runs. **Done:** on success the resolution debits `V`'s
vault; unpaid, the rejoin fails and its slot clears, so `V` may rejoin again
within grace (§3.12) or be slashed when it ends (§3.7).

**A23 — §3.11's default quarantine breaks its own rule.** *An unbonding
validator stays slashable through any ejection's grace and the whole evidence
window, and genesis refuses a quarantine too short for that.* §3.11 requires
`quarantine_length ≥ Δ_grace + evidence_horizon + safety_margin` but defaults
to `2 · block_retention_window`. With the window about one epoch (A4), that is
about `Δ_grace` alone. **Done:** genesis and startup refuse a quarantine below
`Δ_grace + block_retention_window + epoch_length` (40 015 at shipped values;
the shipped 50 000 passes), taking one epoch as the margin. **Question:** how
large a margin? §3.11 says only "a few epochs".

### Departing from what the spec states

**A24 — nothing in the spec triggers the grace slash or the failed-claim
fee.** *Deadlines are applied automatically once the finalized floor passes
them.* §3.7 and §3.6 name no deploy to carry either. **Done:** the
claim-resolution system deploy applies both. Deadlines are measured by the
floor, since votes count only once a floor shows them (§3.10), so a claim
holds its slot until its floor decides or expires it, departing from §3.12's
block-number gate.

**A25 — §3.11's retention attestation has no mechanism.** *Committee
membership does not depend on attesting to block retention, because every
honest node holds the retention window anyway.* §3.11 excludes a node that
does not attest to holding the window and leaves the mechanism to the
finalized-floor spec, which has none. An attestation is unverifiable, and an
exclusion would change the finality denominator §8.2 relies on. **Done:** not
built. A node synced from genesis deletes no block, and a restored node
fetches the whole window (A4). **Question:** is a checkable attestation
wanted, and what would exclude a member?

**A26 — §13.3's formal artifacts are not built for Component 0.** By decision,
the TLA+ model and the Rocq theorems are replaced by integration tests in the
system-integration repository. §14 makes proofs precede a mainnet-blocking
upgrade, so this is recorded as a deviation from §13.3, not as a substitute
proof.

## Corrections

The spec's companion anchors describe the #216 tree it branched from, not `dev`.

| # | Spec says | On dev, and what we did |
|---|---|---|
| 1 | §3.10: `finality/floor_context.rs::authority_stakes` | No `authority_stakes`. dev's equivalent is `finality/floor.rs::floor_committee` — bonds ∩ active at the floor, pinned into the block's `bonds` field by `Validate::bonds_cache_from_floor`. Used that; PoS cannot read the floor, so the deploys that open claims read the block's `bonds` through a new system process, `rho:block:committee`, set from the block at creation and at replay alike, and it is both the seat check and the frozen electorate |
| 2 | §10: extend `RhoBody` in `RhoTypes.proto` | It is `BodyProto` in `CasperMessage.proto`. The votes ride a new `leakyBucketVotes` field there, hashed as correction 9 describes |
| 3 | §3.4, §7.2, §11.4: `system_deploy_util.rs::generate_epoch_mint_deploy_random_seed` | Does not exist, and there is **no epoch-mint system deploy**. The epoch step is inside `PoS(@"closeBlock", …)`, gated on `blockNumber % epochLength`. §11.4's design has nothing to attach to; the epoch step carries our retention sweep of freezes and payments instead |
| 4 | §11.4: test `epoch_mint_seed_differs_per_validator` | Does not exist |
| 5 | §3.4: `Estimator::build_scores_map` supplies the floor stake weights | Private, and returns per-*block* fork-choice scores. Per-validator stake at the floor is the block's own `bonds` field. Used that |
| 6 | §4.6: D3 removed `DeployData.phlo_limit` / `phlo_price` | Both fields are still in `DeployData` on dev and on #216's head. dev still precharges and refunds (`costacc/pre_charge_deploy.rs`, `costacc/refund_deploy.rs`); #216's head has neither file, replacing them with vault-cost deploys (`costacc/vault_cost_deploy.rs`). §4.5's "the existing D3 funding mechanism is unchanged" needs re-checking before Component 1 |
| 7 | §12: `R_base = epoch_phlogiston / epoch_length / num_active` | There are **no per-block rewards**. `PoS(@"getCurrentEpochRewards")` distributes PoS-vault surplus pro-rata **by bond share** at epoch close, which contradicts §12's equal split, and `epoch_phlogiston` existed nowhere on dev. The bucket credits a per-block rate on the proposer's stake instead (A13), set at genesis as `leaky-bucket-reward-rate-ppm` and read back from chain |
| 8 | §9.1: `capacity_of // = current bond` | No state named. The natural read is the block's own `bonds` field. Used that, filtered to positive stake |
| 9 | §10: the body placement is "required so that the votes are part of `B`'s hash preimage" | Body placement does not achieve it. `dev`'s block hash is a Merkle scheme (`proto_util.rs::header_preimage`) that commits to chosen body fields one by one, so a new body field is unhashed until the preimage names it. Added a votes root (leaf tag `0x17`, one leaf per vote, in body order; `0x16` is `dev`'s justifications leaf) to the header preimage. Every block hash changes, empty blocks included: a hash hard fork, with genesis regenerated and node state wiped |
| 10 | §9.3: the claim records are tuples | Tuple indexing appears nowhere in `PoS.rhox` while `.get(key)` is used throughout. Recorded each claim as a map keyed by name instead; a claim is named by (family, target, opened-at block, proposer), which serves as §10's `ClaimId` |
| 11 | §3.2: a slashed validator's entry drops "when a slashing quarantine finalises" | dev has no slashing quarantine: `PoS.slash` moves the bond to the coop vault and zeroes it at once. The entry drops at the first floor showing zero stake; the outcome is the spec's, since a slashed validator forfeits its overflow either way |
| 12 | §11.2: `PreparedUserDeploys` gains `proposer_votes` | `PreparedUserDeploys` is filled before the parents are merged, and what a block owes depends on the merged pre-state's open claims. Built the votes in `create_with_progress` from that pre-state, straight into `Body` |
| 13 | §3.11: honest nodes keep "roughly one week" of blocks, and that window is the slashing evidence horizon | `dev` deletes no block: a node synced from genesis keeps everything, and only a restored node holds a bounded window, fixed at its restore. A slash is authorized only by evidence from the current epoch (`casper/src/rust/slashing_authorization.rs`, not §3.8's and §7.5's `util/` path), and validating one needs the invalid block in the node's own DAG (`ReferencesUnknownBlock` otherwise), so the evidence horizon is one epoch. Sized the window from that (A4) |
| 14 | §11.4: the epoch mint is reduced by the expected overflow, so the two reward channels together keep emission fixed | `dev` mints nothing (correction 3), so there is no mint to reduce. The reward rate enters PoS only as a chain parameter the node reads, so what a view credits per block is backed by no money anyone pays. Overflow claims are paid from the vault's undistributed fee surplus and held topups (A20), and a successful claim they cannot cover goes unpaid, so a view can credit overflow the chain will never be able to pay. Emission stays fixed only trivially. The spec should say what funds the reward on a chain with no mint |

## Parameter note

§12 calls these values placeholders, so none of this is a defect. It is what
the defaults do together.

**The shipped values.** Under A13 and A14 a member holds iff `p · q · ρ ≥ λ`.
For a shard of about ten validators at about 5 s a block: `λ` = 140 ppm, so a
silent validator crosses `θ_eject` = 0.9 after about 714 blocks (an hour, past
the 35-minute catch-up measured on testnet02); `Δ_eject_observation` = 60
blocks; `ρ` = 630 ppm, so a member proposing half its share holds while no
member holds more than a third of the stake.

**In whole bond units, the drain and the reward round to nothing.** *Small
bonds and partial rewards vanish.* `λ_base · C` is zero below a bond of 100,
and the reward is zero for any block that does not saturate both factors. We
count below the bond unit (A5), which fixes the rounding but not the
magnitudes.

**The recency factor stops discriminating.** *At `y_ref = 1`, any block with
two or more parents earns full recency.* Two parents one height back already
sum to 1.0, so the factor separates only single-parent blocks.

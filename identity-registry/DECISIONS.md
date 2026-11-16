<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->

- [Decision Log for the Identity Registry](#decision-log-for-the-identity-registry)
  - [IR-D1. The name of an identity is the hash of its registration payload](#ir-d1-the-name-of-an-identity-is-the-hash-of-its-registration-payload)
  - [IR-D2. A new identity carries a 32 byte random nonce supplied by the client](#ir-d2-a-new-identity-carries-a-32-byte-random-nonce-supplied-by-the-client)
  - [IR-D3. An identity record has a current key, a history, and a sequence number](#ir-d3-an-identity-record-has-a-current-key-a-history-and-a-sequence-number)
  - [IR-D4. The identities store is backed by the system TreeHashMap at depth 3](#ir-d4-the-identities-store-is-backed-by-the-system-treehashmap-at-depth-3)
  - [IR-D5. Rotate and admin relink require a signature by the new key](#ir-d5-rotate-and-admin-relink-require-a-signature-by-the-new-key)
  - [IR-D6. Register serializes through a global lock](#ir-d6-register-serializes-through-a-global-lock)
  - [IR-D7. Admin relink takes M-of-N admin signatures in one call](#ir-d7-admin-relink-takes-m-of-n-admin-signatures-in-one-call)
  - [Notes on defaults from the spec](#notes-on-defaults-from-the-spec)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

# Decision Log for the Identity Registry

Decisions taken during the identity registry implementation. Entries are
numbered IR-D1, IR-D2, and so on, scoped to this crate. If any of them is
later promoted into the project wide Decision Log, it will be given a
global D number at that point.

Each entry covers a choice where more than one reasonable option existed.
Spec requirements that leave no real room for choice are not repeated.

## IR-D1. The name of an identity is the hash of its registration payload

Context. The spec says that registration mints a name but does not say
where the name comes from. The options we considered were hashing just the
public key, having the contract generate a UUID, or letting the caller pick
a string.

Decision. The name is the blake2b256 hash of the full byte string the
client signed to register. That byte string is the operation tag plus the
public key plus a nonce.

Reasoning. If someone replays the same registration, the derived name is
the same, so the contract treats the replay as a no-op rather than minting
another identity. Hashing also spreads names evenly across the storage
layout and does not need any central counter or allocator.

Impact. A name is always 32 bytes. If the hash function ever changes,
every existing name becomes a different name.

## IR-D2. A new identity carries a 32 byte random nonce supplied by the client

Context. IR-D1 ties the name to the public key and a nonce together.
Without a nonce, the name would be fully determined by the key, and one
key could only ever back one identity.

Decision. The client generates a 32 byte random nonce for each new
identity and includes it in the registration payload. The contract does
not read the nonce other than to include it in the hash.

Reasoning. A random nonce is the simplest way to let one key back more
than one identity. It also means an attacker cannot predict the name a
user is about to register, because they would have to guess the random
nonce too.

Impact. Clients have to keep the nonce next to the key, or they will not
be able to recompute the name later. The CLI prints the nonce on
registration for that reason.

## IR-D3. An identity record has a current key, a history, and a sequence number

Context. The spec says that rotations accumulate and old keys stay
readable, but it does not say how the record is shaped. Options included a
flat list of keys with a convention about which one is current, a split
between active and retired keys, or a current pointer alongside a full
history list.

Decision. Each record is a map with three fields.

```
{
  "current": the public key that can sign for this identity right now,
  "history": a list of every key ever linked to this identity, in order,
             each entry carrying the key, the operation that added it,
             the sequence number at that point, the height of the block
             that recorded it, and admin evidence and admin signers when the
             operation was an admin relink,
  "seq":     the sequence number, incremented once per mutation
}
```

Only one key is ever current at a time. Every previous key is still
readable from the history.

Reasoning. Reading the current key and reading the full history are the
two things callers do, so keeping them in separate fields makes each read
simple. Having exactly one current key makes rotation unambiguous, because
there is one key whose signature authorises a rotation.

Impact. The spec allows a name to have more than one current key, and we
are narrower than that. If a later milestone needs more than one current
key per identity, the shape grows but nothing has to be removed. Existing
history stays valid.

## IR-D4. The identities store is backed by the system TreeHashMap at depth 3

Context. The first version of the contract held the identities inside a
single Rholang map on a channel. Every write to any identity had to go
through that channel one at a time, so writes to different identities
could not run in parallel. Reads were fine. The storage layout is the kind
of decision Rule 4 of the spec says must be recorded.

Decision. The contract now keeps identities inside the system TreeHashMap,
initialised to depth 3. The handle for the store is held on the same
channel the plain map used to live on.

Reasoning. The TreeHashMap lets writes to different identities proceed in
parallel, because they land in different parts of the tree and the node
can merge them without conflict. Depth 3 gives roughly 16 million storage
slots, which is far more than the prototype will ever need, so two
different identities almost never collide in the same slot.

Impact. The outward behaviour of the contract is unchanged. The public
operations take the same arguments and return the same shapes, so the
Rust client, the CLI, and the operation templates were not affected.

Known limitation. There is a corner case where two blocks try to create
the very first entry at the same storage slot at exactly the same time.
The node currently does not catch that as a conflict, so both writes could
succeed and leave the slot in a split state. Mitigated inside the contract
by IR-D6. A reproducer and an upstream fix are tracked on a separate
branch.

## IR-D5. Rotate and admin relink require a signature by the new key

Context. A rotation names a new public key. If only the current key
signs, nothing in the request proves that the submitter actually holds the
new key. An attacker who had the current key, or an administrator who made
a mistake, could point an identity at a key that the intended party does
not control.

Decision. Rotate and admin relink both take a second signature over the
same payload. That second signature must verify under the new public key.
If either signature fails to verify, the operation returns a bad
signature error.

Reasoning. Requiring a signature by the new key proves the submitter has
the matching private key. It adds one more signature per call, which is a
very small cost.

Impact. The Rust client takes the new secret key instead of the new
public key, and derives the public key itself. The CLI uses `--new-sk`
instead of `--new-pk`. When an administrator performs a relink on behalf
of someone else, that other person has to supply their signature out of
band.

## IR-D6. Register serializes through a global lock

Context. IR-D4 moved the identities store to the TreeHashMap so writes to
different identities could proceed in parallel. We later found a corner
case where two blocks that both try to create the very first entry for the
same name can both succeed and leave the storage slot in a split state.
The merger does not catch this today because neither block consumes a
shared base value on the leaf channel. The case only comes up on register,
because rotate and admin relink both return unknown_name when the record
is missing and never create a new entry themselves.

Decision. The contract holds a global registerLock channel seeded with a
single token. Every register call takes the token before checking the
store and puts it back after setting. Rotate and admin relink are
untouched and still run in parallel with each other and with ongoing
registers on different names.

Reasoning. Only register creates new entries, so only register needs the
lock. A single global token is the simplest shape that cannot race on its
own creation. A per-name lock would hit the same fresh-leaf race the
moment the lock entry itself is created for the first time, which would
just move the problem.

Impact. All register calls are now serialized. For a prototype this is
acceptable because registration is rare compared to resolve and rotate.
The lock should be removed once the fresh-leaf race is caught by the
merger upstream.

## IR-D7. Admin relink takes M-of-N admin signatures in one call

Context. A single admin key is a single point of failure. The spec asks
for a quorum that changes the signer set and nothing else. A quorum can
collect its signatures in one call, or it can collect them over several
calls with pending state in the contract.

Decision. The genesis config gives a set of admin public keys and a
quorum M. One admin relink call carries a list of `(admin_pk, sig)`
pairs. Each admin signs the same payload as before, so the payload and
the signing scheme do not change. The contract counts the set of admin
keys that are in the admin set and whose signature verifies. The call
succeeds when that set has at least M keys and the new key signature
verifies. The new history entry records the signers in `admin_signers`.

Reasoning. One call keeps the contract free of pending state. Each
signature comes with its key, so the contract verifies each signature
once, against that key only. A set counts a repeated key once, so one
admin cannot meet the quorum alone. A key outside the admin set counts for
nothing. The `admin_signers` field shows who authorized each relink.

Impact. The admins collect their signatures off chain. Each admin runs
`admin-sign` with their own key, which prints `<admin_pk>:<sig>` and does
not deploy. The submitter passes each pair to `admin-relink --admin-sig`,
so no machine needs more than one admin secret key. `--admin-sk` signs
locally and can be mixed with `--admin-sig`. The default is 1-of-1.

## Notes on defaults from the spec

These are not independent decisions, but recording them here keeps
surprise off the page.

- The administrator is a quorum of M of N keys set at genesis (IR-D7).
  The contract has no operation that changes the admin set.
- The administrator evidence supplied with an admin relink is stored
  exactly as the caller sent it. The contract does not validate it, per
  the spec.
- Signatures use the secp256k1 scheme in DER encoding. The node already
  provides a built in verifier for that scheme, which the contract calls
  directly.
- All signing happens off chain in the client. The contract only ever
  sees public keys and signatures.

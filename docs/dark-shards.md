# Dark shards for Morph profiles

Status: design notes, 2026-11-13. Not built. Extends section 3 of
[identity-recovery.md](identity-recovery.md) with what Weirdnet 0.1.0 already
provides and what Morph still lacks.

A profile's identity is a 32-byte seed (24 words). Dark shards let a person
lose every copy of the words and every backup and still get the identity
back from people they trust, the way Muninn does
(`mnnn net shard entrust` / `restore`, Muninn `docs/dark-shards.md`).

## What already exists in weirdnet-keys 0.1.0

| Piece | API | Notes |
|---|---|---|
| Shamir k-of-n split of a 32-byte key | `shards::split(&Profile::WEIRDNET, &seed, k, n)` | Text form `weirdnet-shard-v1:<set>:<k>:<i>:<hex>:<blake3 check>`; the check catches typos, not forgery. 2 ≤ k ≤ n ≤ 255. |
| Recombine | `shards::combine(&profile, &[texts])` | A foreign or forged share gives a wrong key: the caller must check it (Morph: the user id derived from the seed must equal the profile's). |
| Seal each shard for a holder | `dark::seal(&profile, &seed, &shard_password, k, n)` | Argon2id + XChaCha20-Poly1305; AAD `weirdnet/keys/v1/dark-shard|<set>|<index>`, so a seal moved to another set or index will not open. |
| Open one | `dark::open(&profile, &pw, set, index, &sealed)` | Wrong password, tampering or a moved seal all read as "wrong shard password". |
| Claim tag | `password::claim_tag(&profile, &pw)` | Argon2id with the profile's fixed claim salt, then keyed BLAKE3: a fresh install that knows only the shard password can ask holders for "what you hold under this tag". Anyone holding a tag can test guesses at Argon2id cost, so the shard password must be strong. |
| Paper shards | `keyring::export(.., ExportItem::Shards)` | `shard-N.age` files, 3 of 5, each age-sealed with the export password. |

So Morph needs no new cryptography. The earlier design's claim derivation
(HKDF over the password, salted by the set id) and AAD over threshold and
user id are **not** what 0.1.0 does; Morph uses 0.1.0 as it is, so Muninn and
Morph shards stay interchangeable. Any change goes into weirdnet-keys first.

## Two kinds of export

1. **Paper or file shards (no network).** "Split into shards" in the identity
   dialog, behind the profile password: k-of-n (default 3 of 5), each shard
   saved as its own age file under a **shard password**, or shown as text to
   print. Handing them out is up to the person. Restore in onboarding accepts
   k shard files ("Restore from shards"), next to words, backup file and
   another device. This needs only weirdnet-keys and the file plumbing that
   identity backups added, so it can ship before any of the network parts.
2. **Dark shards (held by peers).** The same sealed shards, sent to trusted
   peers' nodes, which store a blob they cannot open. Restore asks them.

## Dark shards: what Morph is missing

- **Trusted connections.** Only nodes of users you exchanged keys with and
  marked trusted (or your own linked devices) may hold a shard. Morph has no
  key exchange or trust list yet (ARCHITECTURE §5 "Trust"); dark shards wait
  for it.
- **The holder protocol.** Muninn's entrust, list, drop and restore messages
  (`/weirdnet/shard/1`), holder storage and claim lookups belong in a shared
  crate (`weirdnet-shards`, planned) so Morph and Muninn interoperate; Morph
  must not grow its own copy. Messages travel inside the exchange's sealed
  channel over libp2p.
- **Reachability.** Restore needs `threshold` holders online at once. Phones
  are reachable only while open (or with the opt-in relay wake path), so the
  UI shows who answered and waits rather than failing.

## Flows (when built)

**Entrust** (identity dialog → "Dark shards", profile password first):
pick holders from trusted connections of this profile only (other
profiles' contacts are never offered); threshold defaults to a majority, at
least 2; enter the shard password twice (eye toggles on both). Entrust checks
every holder is online first so a set is never half placed, then shows
"✓ Ben holds shard 1 of 3". History records set id → holders, never shard
contents.

**Restore** (onboarding → "Restore from peers"): enter your user id or let a
linked device fill it, enter the shard password; Morph asks connections for
shards under the user id or the claim tag, opens them, combines, and checks
the user id before sealing the seed under a new profile password.

**Lifecycle:** re-entrusting makes a new set and asks holders to drop the old
one; holders can drop what they hold; deleting a profile asks its holders to
drop its sets.

## Rules carried over

- Never log or show shard texts, the seed, the shard password or claim tags.
- The shard password should differ from the profile and backup passwords;
  the UI says so and refuses one equal to the profile password.
- Holders must be different users (or different own nodes); Morph warns when
  two shards would sit with the same person.
- Tests: split/seal/open/combine against weirdnet-keys, wrong passwords,
  moved seals, foreign shards rejected by the user-id check, and a cross-check
  that Muninn-made shard files restore in Morph and the reverse.

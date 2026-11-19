# Device identity for calls between linked devices

Status: on `main` (2026-11-18), in `morph-services/src/device_calls.rs`.

## Two callers

| | `app_calls::NetAppCaller` | `device_calls::DeviceCaller` |
|---|---|---|
| For | `weirdnet-<app>://` addresses typed in the browser (public apps) | the user's own linked devices (Node companions, inference offload, phone to desktop) |
| Key | a throwaway key, a node of its own | the device's node key: the profile's running sync node |
| Says who it is | nothing (anonymous on purpose) | its peer id (Noise-authenticated) and its cert from the linked-devices roster, in the `weirdnet-device-cert` header |
| Far end | any peer that serves the app | only an active linked device of the same identity |

The anonymous path is unchanged. Nothing moved from `NetAppCaller` to the device path by
itself: callers choose `DeviceCaller` when the far end is one of the user's devices.

## The check (both ends)

`check_caller(profile, roster, user_key, peer, presented) -> CallerCheck`:

1. `peer` is the transport-authenticated peer id, never something the caller sent.
2. A presented cert must be for `peer` and for this user and verify under the user key
   (else `WrongUser`), and must not be revoked (else `Revoked`).
3. The roster must hold an active cert for `peer` (else `Unknown`, or `Revoked`). A
   presented cert alone never admits a caller: anyone holding the seed, a revoked device
   included, can mint a cert for a new peer id.
4. Once the key chain has a head (after the first rotation), the peer must be one of its
   members (else `NotAdmitted`).

Refusal reasons on the wire: `unknown-device`, `revoked`, `wrong-user`, `needs-admission`.

The same rule is in `weirdnet-sync` main as `Roster::check_caller` / `CallerCheck`
(unreleased). Morph keeps its own copy until it moves to that release, then calls the crate's.

## API sketch

```rust
use morph_services::device_calls::{DeviceCaller, DeviceCallError, LinkedDevice, CallerCheck, CALL_TIMEOUT};

// After unlock, once the profile's sync node runs.
let dc = DeviceCaller::for_profile(&home, profile_id)?;   // Err(NotSyncing) before that

dc.peer_id();                  // this device's peer id (node key)
dc.cert();                     // this device's NodeCert from the roster
dc.linked_devices();           // Vec<LinkedDevice { peer_id, label, this_device }>, unrevoked only
dc.check(peer, presented);     // CallerCheck against the live roster

// Calling a linked device (async; refuses to dial anything but an active linked device):
let resp = dc.call(peer, weirdnet_net::AppRequest::new("morph-agent", "POST", "/infer"), CALL_TIMEOUT).await?;
let stream = dc.open_stream(peer, "/morph/agent/1", weirdnet_net::stream::Limits::default()).await?;

// Serving linked devices only (others get AppError::Refused(reason); the handler never sees them):
dc.serve("morph-agent", Arc::new(handler))?;
let mut incoming = dc.accept_streams("/morph/agent/1", Limits::default())?;
while let Some(stream) = incoming.next().await { /* stream.peer() is a linked device */ }
```

Errors: `NotSyncing`, `NotLinked { peer, reason }` (our side refused to dial), `Refused(reason)`
(the far end refused us, e.g. this device was revoked there), `Unreachable`, `Failed`.

### For `morph-ai` `NodeRouter`

- `nodes()`: map `linked_devices()` (plus platform, online and serves-models, which Morph Dev
  fills from discovery and companion settings).
- `open(peer, protocol)`: `open_stream(peer, protocol, limits)`, then frame messages on the
  stream for `Link { tx, rx }`. `NotLinked` maps to `RouteError::NotLinked`, `Unreachable` to
  `RouteError::Unreachable`, the rest to `RouteError::Failed`.
- Serving side: `accept_streams(protocol, limits)` and run the session for each stream.

The stream path checks the caller by peer id only (no cert header on raw streams); the
roster is authoritative either way.

## Not done yet

- Morph stays on `weirdnet-net` 0.6 and `weirdnet-sync` 0.3.1. Moving to `weirdnet-net` 0.7
  needs a `weirdnet-sync` release on it (main has the bump and `check_caller`).
- No Morph feature calls `DeviceCaller` yet; `NodeRouter` is the first user.

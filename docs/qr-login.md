# Linking a device with a QR code

Moves a profile's Weirdnet identity (the 24-word seed) from a device that has
it to a new one, without typing the words. Design: `identity-recovery.md`,
section 4. Built 2026-11-12.

The **new device** shows the code; the **existing device** scans it (or
pastes the link) and sends the identity after you compare six digits. Both
devices must reach each other directly, for now: the same Wi-Fi or LAN, no
relay yet.

## On the new device

1. Open Morph. In the identity prompt for the profile, choose
   **Link from another device**. (Skipped it? Menu, then **Identity**.)
2. Morph shows a QR code and its `weirdnet-morph://link/…` text, with a
   **Copy** button. The code is good for two minutes; afterwards choose
   **Show a new code**.

## On the existing device

1. Menu (on Android, tap the address bar and scroll the sheet), then
   **Identity**, then **Add device**.
2. Enter this profile's password.
3. Android: **Scan QR code** and point the camera at the new device's
   screen (Morph asks for the camera the first time). Desktop: paste the
   link into the field. Then **Connect**.
4. Both screens show the same six digits, e.g. `652 503`, and the existing
   device names the new one ("Link Morph on alien-blue?").

## Finish

1. If the digits match, choose **Codes match** on the new device and
   **Codes match, send identity** on the existing one. If they differ, choose
   **They are different** on either: nothing is sent and the code dies.
2. The new device asks for a password for the identity there (it can differ
   from the other device's), then **Finish**.
3. Identity on the new device now reads "Linked from another device." with
   the same user id and fingerprint as the existing one.

## Good to know

- The QR holds only public keys, addresses and a signature. The identity
  travels sealed with a key from that session, only after the digits are
  confirmed, and only once. An offer dies after two minutes, three pairing
  attempts, a cancel or one transfer.
- A wrong password on the new device does not spend the transfer; try again.
- "network: …" errors usually mean the devices cannot reach each other: put
  them on the same network (guest Wi-Fi often isolates devices).

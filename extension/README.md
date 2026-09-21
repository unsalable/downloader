# Universal Downloader Connector

The browser half of the app's browser link. It reads the YouTube cookies of the
Chrome profile it is installed in and pushes them to Universal Downloader over
Chrome's native messaging channel, so the app can use a sign-in the user already
has. It exists because Chrome 127 encrypts its cookie database against other
programs on the same machine, which is what stopped the download engine reading
it directly.

Nothing here listens on a port. Chrome starts `ud-bridge.exe` itself, only for
this extension's id, and only because the app wrote a registry value under the
user's own hive naming it. The wire format is
`src-tauri/src/bridge/protocol.rs`; every field name in `background.js` comes
from there.

No host reply ever carries cookie material. The channel is write-only towards
the app: the popup can learn that a session exists and how old it is, never what
is in it.

## Loading it for development

1. `node scripts/extension-key.mjs` once, if `key.pem` is not already there. It
   writes the private key, puts the matching `key` into `manifest.json` and
   prints the extension id.
2. Set `EXTENSION_ID_DEV` in `src-tauri/src/bridge/protocol.rs` to that id, and
   rebuild. The host compares the origin Chrome passes it against that constant
   and refuses anything else, so an id it does not know is an extension it will
   not answer.
3. Run the app once so it writes the native messaging registry values.
4. `chrome://extensions`, turn on Developer mode, **Load unpacked**, and pick
   this `extension` directory.
5. The welcome tab opens by itself. Press Connect in the popup.

`node extension/make-icons.mjs` regenerates `icons/` if the mark changes.

### Why the manifest carries a key

An unpacked extension's id is normally derived from the path it was loaded
from, so it changes when the checkout moves and the host stops recognising it.
The `key` field pins it: Chrome derives the id from that public key instead --
SHA-256 over the DER SubjectPublicKeyInfo, the first sixteen bytes, hex, each
digit shifted from `0`-`f` onto `a`-`p`.

The Chrome Web Store ignores the manifest `key` and issues its own id at
publication, so the published copy has a different id from every development
copy. That is why `protocol.rs` holds two constants and accepts both; setting
only one of them breaks either development or the published extension the day
the other starts being used.

Regenerating `key.pem` changes the id. `scripts/extension-key.mjs` refuses to
overwrite an existing key for that reason, and prints the id it already implies.

## Packaging for the store

Everything in this directory ships except `key.pem` (which must never leave the
machine that publishes), `README.md` and `make-icons.mjs`.

## What the store listing has to say

A single-purpose extension is the rule the review process applies hardest to
anything touching `cookies`, and this extension's purpose has to be stated in
those terms: *it hands the user's own YouTube session to Universal Downloader,
a program running on the same computer, so that program can use it.* That is one
purpose, and the permissions map onto it one for one:

- `cookies` with `host_permissions` restricted to `https://*.youtube.com/*` --
  the session itself. Nothing else is read, and the narrow host pattern is the
  claim. Requesting `<all_urls>` or `*://*/*` here is the usual reason an
  extension like this is rejected.
- `nativeMessaging` -- the only channel out. There is no remote endpoint, no
  analytics and no network request of any kind; the listing should say so
  plainly, because a reviewer's first assumption about a cookie-reading
  extension is that the cookies are going somewhere.
- `storage` -- one generated profile id and the last outcome, for the popup.
- `alarms` -- a daily refresh, so a stored session does not go stale unnoticed.

`https://*.google.com/*` and `identity.email` are optional and are requested
from the popup, after a connection is working and has not produced what the user
wanted. Asking for either at install would widen the stated purpose from "this
site" to "this account", which is the line the narrow-purpose rule draws.

The listing must not promise what YouTube will allow. The phrase throughout this
extension is "lets the app use your YouTube sign-in"; whether a given video then
downloads is between the user's account and YouTube.

## Publishing it

`node scripts/pack-extension.mjs` writes `extension-upload.zip` at the repository
root. It removes the `key` from the manifest -- Google's guidance is to strip it,
since the store issues an id of its own and ignores that field -- and leaves out
`key.pem`, `make-icons.mjs` and these notes. It prints what it packed, so a file
that should not ship is visible in the output rather than in the listing.

`STORE-LISTING.md` holds every field the dashboard asks for, including the
permission justifications, and `PRIVACY.md` is the policy the `cookies`
permission obliges the listing to link to.

The published extension has a different id from an unpacked one. The last step
after approval is setting `EXTENSION_ID_STORE` in
`src-tauri/src/bridge/protocol.rs`, without which the host refuses the store
build; `STORE-LISTING.md` ends with that.

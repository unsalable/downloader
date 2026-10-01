# Universal Downloader Connector

The browser half of Universal Downloader. It does two things, both over Chrome's
native messaging channel to the app on the same computer:

- **It sends the videos playing in the browser to the app.** It notices the
  video and audio the open tabs fetch, lists them in its popup, and when the
  user presses **İndir** hands that one item to the app, which comes to the
  front and downloads it at the user's default quality. The extension never
  downloads anything itself.
- **It lends the app the profile's YouTube session**, when the user turns the
  switch on, so members-only videos the user pays for can be downloaded. Chrome
  127 encrypts its cookie database against other programs on the same machine,
  which is what stopped the download engine reading it directly.

Nothing here listens on a port. Chrome starts `ud-bridge.exe` itself, only for
this extension's ids, and only because the app wrote a registry value under the
user's own hive naming it. The wire format is
`src-tauri/src/bridge/protocol.rs`; every field name in `background.js` comes
from there. No host reply ever carries cookie material.

## How the pieces fit

| File | What it does |
|---|---|
| `media.js` | Pure helpers, no `chrome.*`: what a response is (`classify`), HLS and DASH manifests (`parseHls`, `parseDash`), and the popup's list (`rows`). Shared by the worker and the popup. |
| `background.js` | The service worker (an ES module). Watches `webRequest`, keeps a list per tab in `chrome.storage.session`, draws the badge, answers the popup, talks to the host. |
| `popup.*` | The list, the **İndir** buttons and the YouTube session switch. |
| `welcome.*` | Opened once, on install. |
| `theme.css`, `fonts/` | The app's tokens and its typeface (Inter, OFL; the licence ships beside it). |

1. `webRequest.onHeadersReceived` sees every media, XHR and fetch response of
   every tab. `media.classify` sorts it into a stream (HLS, DASH), a file
   (video, audio), or something that is not worth a row: a segment, an advert,
   YouTube's `videoplayback`. Those still mark the tab as playing something.
2. Rows worth keeping go into `tab:<id>` in session storage, with the frame and
   origin that asked for them. A navigation drops them; closing the tab deletes
   the record.
3. Opening the popup runs a scan: a small function in every frame reports the
   page's `<video>`/`<audio>` elements and its title, and each manifest is read
   again inside the frame that first fetched it, so the request carries the
   same Referer, Origin and cookies the player's did. `media.rows` turns all of
   it into the list.
4. **İndir** sends `{ type: "download", …peer, url, kind, title, pageUrl,
   referer, origin, userAgent, thumbnail }` (Contract A). The host writes it to
   the app's inbox and starts the app with `--handoff`.

On YouTube and the other sites the app reads by address (`PAGE_SITES` in
`media.js`), the popup offers the page itself rather than the pieces the player
fetched. DRM is never worked around: an element with `mediaKeys`, an HLS key
naming a DRM system or a DASH `ContentProtection` makes the row "Protected",
with no button, and the services the app refuses by name (`PROTECTED`, a mirror
of `src-tauri/src/providers/detect.rs`) list nothing at all.

The tests are `node --test scripts/extension/`, outside this folder so the
store package never carries them. They also check that `PROTECTED` still
matches `detect.rs`.

## Loading it for development

1. `node scripts/extension-key.mjs` once, if `key.pem` is not already there. It
   writes the private key, puts the matching `key` into `manifest.json` and
   prints the extension id.
2. Set `EXTENSION_ID_DEV` in `src-tauri/src/bridge/protocol.rs` to that id, and
   rebuild. The host compares the origin Chrome passes it against its two ids
   and refuses anything else.
3. Run the app once so it writes the native messaging registry values.
4. `chrome://extensions`, turn on Developer mode, **Load unpacked**, and pick
   this `extension` directory.
5. The welcome tab opens by itself. Play a video and open the popup.

The toolbar icons are the application's own six-blade aperture, drawn from the
same proportions as `scripts/generate_icon.py`. `node extension/make-icons.mjs`
regenerates `icons/` when the mark changes; `welcome.html` carries the same
shape inline and has to be changed with it.

### Why the manifest carries a key

An unpacked extension's id is normally derived from the path it was loaded
from, so it changes when the checkout moves and the host stops recognising it.
The `key` field pins it: Chrome derives the id from that public key instead --
SHA-256 over the DER SubjectPublicKeyInfo, the first sixteen bytes, hex, each
digit shifted from `0`-`f` onto `a`-`p`.

The Chrome Web Store ignores the manifest `key` and issues its own id, so the
published copy (`oikcjjcihkfmgmmmjagilnfgnfilghic`) has a different id from
every development copy. That is why `protocol.rs` holds two constants and
accepts both; an app built without the store id is what a store user meets as
"Universal Downloader'ı güncelleyin".

Regenerating `key.pem` changes the development id. `scripts/extension-key.mjs`
refuses to overwrite an existing key for that reason, and prints the id it
already implies.

## Packaging for the store

`node scripts/pack-extension.mjs` writes `extension-upload.zip` at the
repository root. It removes the `key` from the manifest -- Google's guidance is
to strip it -- and leaves out `key.pem` (which must never leave the machine that
publishes), `make-icons.mjs`, `.gitignore`, these notes, `PRIVACY.md`,
`STORE-LISTING.md` and any test file. Everything else ships, `media.js` and
`fonts/` included. It prints what it packed, so a file that should not ship is
visible in the output rather than in the listing.

`node scripts/pack-extension.mjs --firefox` writes `extension-firefox.zip` for
addons.mozilla.org from the same files. Only the manifest differs: the
background runs as a module script instead of a service worker, the id is
`connector@universaldownloader.app` (`FIREFOX_EXTENSION_ID` in
`src-tauri/src/bridge/protocol.rs`), and it declares that no data is collected.
The app registers a second host manifest for Firefox under
`HKCU\Software\Mozilla\NativeMessagingHosts`, with `allowed_extensions` where
Chrome's has `allowed_origins`, and the host accepts Firefox's way of naming
the caller -- the manifest path first, the extension id second. `npx web-ext
lint` on the unpacked zip reports no errors or warnings.

`node scripts/build-store-assets.mjs` photographs the real popup for the
listing images in `store-assets/`, with headless Chrome and no user
interaction.

## What the store listing has to say

The single purpose is *sending the videos playing in the user's browser to
Universal Downloader, a program on the same computer*. Lending the YouTube
session is part of the same purpose: it is what lets the app download the
members-only videos the user can already watch. The permissions map onto it:

- `webRequest` with `<all_urls>` -- seeing which videos a tab plays. There is
  no narrower pattern: the videos come from whatever server the site uses.
- `scripting` -- reading the page's video elements and title when the popup
  opens, and re-reading a stream's playlist inside the page for its quality.
- `nativeMessaging` -- the only channel out. No remote endpoint, no analytics.
- `cookies` -- youtube.com only, only while the switch is on.
- `storage` -- a profile id, the switch, and the per-tab lists (session
  storage, gone when the browser closes).
- `alarms` -- a daily refresh, so a lent session does not go stale unnoticed.

Two things to know before every upload:

- **The permissions grew in 1.0.3.** Chrome disables an installed extension
  whose update asks for new permissions until the user accepts them, so 1.0.2
  users see it switched off with a prompt.
- **YouTube is a policy risk.** The Chrome Web Store has refused and removed
  extensions that download from YouTube. The popup offers YouTube pages because
  the user chose that knowing the risk; if review objects, YouTube's hosts go
  into `UNLISTED_SITES` in `media.js`. Taking them out of `PAGE_SITES` is not
  enough: the page fallback would still offer YouTube's player as a "Page" row.

`STORE-LISTING.md` holds every field the dashboard asks for, and `PRIVACY.md`
is the policy the listing links to. Both have to describe exactly the
permissions `manifest.json` asks for.

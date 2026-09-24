# Universal Downloader

An app for Windows and Android that downloads media from a link. Paste a URL,
it reads what the source publishes, shows the real qualities on offer, and
downloads the one you pick. No ads, no account, no telemetry.

Built with Tauri 2, React 19, TypeScript and Rust.

## Download

Everything is on the [releases page](https://github.com/unsalable/downloader/releases/latest).

### Windows

- [`UniversalDownloader_x64-setup.exe`](https://github.com/unsalable/downloader/releases/latest/download/UniversalDownloader_x64-setup.exe) -- normal install, per user, no admin rights needed.
- [`UniversalDownloader_x64.msi`](https://github.com/unsalable/downloader/releases/latest/download/UniversalDownloader_x64.msi) -- for managed or scripted deployment.

These links always point at the current build. Installing over an existing copy
updates it in place; settings, history and the downloaded tools are kept.

Windows 10/11, 64-bit. The installers are not code-signed, so SmartScreen warns on first
run: choose **More info -> Run anyway**.

### Android

- [`UniversalDownloader_android_arm64.apk`](https://github.com/unsalable/downloader/releases/latest/download/UniversalDownloader_android_arm64.apk) -- practically every phone from the last several years. Pick this one if unsure.
- [`UniversalDownloader_android_armv7.apk`](https://github.com/unsalable/downloader/releases/latest/download/UniversalDownloader_android_armv7.apk) -- older 32-bit phones.
- [`UniversalDownloader_android_x86_64.apk`](https://github.com/unsalable/downloader/releases/latest/download/UniversalDownloader_android_x86_64.apk) -- emulators and x86 Chromebooks.

Android 7.0 or newer. The app is not on Google Play: open the APK on the phone
and allow installs from that source when Android asks. Every build is signed
with the same key, so a newer APK installs over the old one and keeps settings
and history.

The app looks for a newer release each time it is opened. When there is one it
offers to update: the APK for the phone's processor is downloaded, checked
against the checksum on the release page, and handed to Android's installer.
Nothing is shown when the app is up to date or offline. **About** has a button
to check by hand.

Downloads land in `Download/Universal Downloader`, where the gallery and file
manager find them. To download from another app, use its **Share** button and
pick Universal Downloader.

---

## What it does

- **Reads a link and shows what is actually there** — title, creator, duration,
  thumbnail, and the genuine list of renditions the source offers. Nothing in
  the quality menu is aspirational: a 720p video does not show a 4K option.
- **Picks the best combination for you**, merging separate video and audio
  streams when that is what "best quality" requires, and telling you *before*
  you start if that merge needs a tool you have not installed yet.
- **Downloads photos, not just videos** — Instagram photos and carousels, X
  photos, TikTok photo posts, Reddit images and galleries, and Pinterest pins,
  each at the largest size the platform publishes. A carousel downloads item by
  item, videos and photos alike, each as the file it actually is.
- **Downloads fast.** Transfers are issued as bounded ranged requests rather
  than one long connection, which is what several large hosts throttle. On one
  measured CDN this is the difference between 35 kB/s and 15 MB/s.
- **Pauses, resumes and retries** without losing progress. Partial files live
  outside your Downloads folder until they are complete.
- **Queues** as many links as you like, with a configurable number running at
  once, and survives a restart.
- **Keeps a history** with the file, its size, and a one-click re-download.
- **Converts files you already have.** Drop in a video or an audio file and
  write it out as MP4, MKV, WebM, MOV, AVI, MP3, M4A, AAC, WAV, FLAC, Opus or
  Ogg. When the target container can already hold the streams it repackages
  rather than re-encodes -- seconds instead of minutes, and nothing is lost.

Recognised sources include YouTube, TikTok, Instagram, X, Reddit, Facebook,
Twitch, Pinterest, Vimeo, Dailymotion and SoundCloud, plus direct media files
and any page that publishes its media in Open Graph tags.

"Recognised" is the honest word. Whether a particular link yields anything is
decided by the platform, not by this app: several now gate parts of their
catalogue behind a signed-in session, and Vimeo currently requires one for most
videos. The app does not work around that — it reports it in plain language and
says why. Nothing in the interface is shown as available when it is not.

## What it deliberately does not do

The app only downloads what a source makes available to you. It contains no DRM
circumvention, no paywall or authentication bypass, and no watermark removal.
Where a platform publishes both a watermarked and a clean rendition, the clean
one can be selected; where it publishes only a watermarked one, the app says so
rather than pretending otherwise.

---

## Architecture

```
src/                     React front end
  components/ui/         Design-system primitives
  components/home/       Analyse, preview and download options
  components/downloads/  Queue cards
  components/settings/   Tool cards, hotkey recorder
  components/convert/    Conversion job rows
  components/intro/      The phone's first-run film (Remotion)
  pages/                 Home, Downloads, Convert, History, Settings, About
  stores/                Zustand stores (settings, queue, convert, tools, analysis)
  services/ipc.ts        The only place that calls `invoke`
  lib/                   Pure helpers (formatting, URLs, option derivation)
  i18n/                  English source dictionary + Turkish translation;
                         a first launch takes the system's language

src-tauri/src/           Rust core
  providers/             Source adapters and URL classification
  downloader/            Planning, HTTP transfer, control, speed smoothing
  queue.rs               Scheduling, retries, persistence, progress events
  ffmpeg.rs              Merging and conversion during a download
  converter.rs           Probing and converting files already on disk
  tools.rs               Discovery and installation of the external tools
  db.rs                  SQLite: settings, history, durable queue
  commands.rs            The IPC surface
```

### Provider architecture

Every source adapter implements the same three-method `MediaProvider` trait
(`id`, `can_handle`, `analyze`) in `src-tauri/src/providers/mod.rs`. Adding a
platform means adding a module and a branch in `analyze`. Resolution order is:
a direct media file → a photo post → the engine → the generic page reader →
unsupported.

The engine is a video tool, and photo posts are where that shows. It is run
with `--ignore-no-formats-error`, so a post with pictures and no stream is
reported rather than refused, and on Instagram and Pinterest the pictures it
lists are offered at full size. For the platforms whose photos it does not
return at all -- X, Reddit and TikTok photo mode -- `providers/photos/` reads
the post from the same public data the platform's website loads for a
signed-out visitor, before the engine is started; anything that turns out to
be a video is left to the engine as before. A gallery is one analysis with
every item in it (`MediaMetadata::entries`), and each queued item names its
position, so it downloads that item and not the first one again.

The trait is used for static dispatch rather than behind `dyn`: its methods are
async and the set of providers is closed, so calling them directly keeps the
path allocation-free while still making the shared contract explicit.

### Android

The phone build is the same Rust core and the same interface. What differs is
confined to `src-tauri/src/android.rs`, a Kotlin plugin in
`src-tauri/gen/android` (`BridgePlugin.kt`, `BackgroundWorkService.kt`) and a
handful of `IS_MOBILE` branches in the front end.

- **Tools.** Android refuses to run a file an app has downloaded, so Python 3,
  FFmpeg, ffprobe and QuickJS ship inside the APK as native libraries -- the
  packaging maintained by [youtubedl-android](https://github.com/JunkFood02/youtubedl-android),
  whose `jni` folders the Gradle build extracts. Their support libraries are
  unpacked once per install. yt-dlp itself is a Python program, so it is still
  fetched on request and kept up to date exactly as on Windows; the bundled
  interpreter runs it, and QuickJS is passed to it as the JavaScript runtime.
- **Files.** Downloads are written to the shared Downloads folder and then
  announced to the media index. Files picked for conversion arrive as content
  URIs, so they are copied into the cache first, and results go to Downloads.
- **Background.** A foreground service runs while anything is downloading or
  converting; without it Android freezes the app as soon as it leaves the
  screen. It stops when the queue is empty.
- **Interface.** A bottom tab bar replaces the sidebar, Back returns to Home,
  and links shared from other apps are analysed on arrival. Settings is a list
  of sections, each opening on its own page, with rows and controls sized for
  touch. Desktop-only settings (tray, autostart, shortcuts, tool paths, folder
  pickers) are hidden. Pressing Download moves to the Downloads tab.
- **First run.** The first launch plays a short film of how the app is used,
  drawn live from the app's own screens and strings with `@remotion/player`
  (`src/components/intro`, loaded only then), so it follows the language and
  theme and adds no video to the APK. The screen is held upright while it
  plays, About can play it again, and with reduced motion or low resource mode
  it is the still welcome the desktop shows. `node scripts/intro-video/render.mjs`
  renders the same film to stills or an MP4 for review.
- **Heat.** A phone pays for effects a desktop does not notice. Frosted glass
  and the moving aurora are replaced by solid panels and a still backdrop,
  progress bars move by transform, list items skip layout animation, and the
  app no longer re-renders every screen on each progress tick. The analysis
  the user just saw is reused when Download is pressed rather than run again
  -- for YouTube that is a Python start and a JavaScript challenge solved
  twice -- and yt-dlp reports progress twice a second instead of per block.
- **Updates.** A build records the commit it came from (`build.rs`). Releases
  are unversioned, so `src-tauri/src/updater.rs` compares that commit with the
  one the `latest` tag points at; a difference means a newer build. The APK is
  verified against the SHA-256 digest GitHub publishes for the asset, and
  `BridgePlugin.installApk` asks for the install permission if needed and opens
  the system installer. Always build release APKs from a committed tree, and
  move the tag only after the new files are uploaded.

### Two external tools

Neither is bundled. Both are fetched once, on request, into the app's own data
directory — and a copy already on your `PATH` is preferred over downloading
anything.

- **yt-dlp** reads public metadata and stream URLs. It changes weekly, so a
  bundled copy would be stale the month after release.
- **FFmpeg** merges separate audio and video streams and handles conversion.
  It is GPL, which is cleanest to keep as a separate process the user opts into.

Both run as separate processes with an argument vector. No user-supplied URL
ever reaches a command line as text that could be re-parsed.

### The download path

`downloader/plan.rs` turns a request into concrete streams — this is where
"Best quality" stops being a word. It is pure, so the selection rules are
tested directly. The UI asks this same code what a selection resolves to
(`summarize_plan`), which is why the quality, container and estimated size it
shows always match what the download does.

Progressive streams are fetched by `downloader/http.rs` in bounded chunks.
Segmented protocols (HLS, DASH) are handed to the engine, whose progress is
read back through a machine-readable template so the UI shows the same real
byte counts either way.

### The conversion path

`converter.rs` starts from a local file rather than a URL, but reuses the same
machinery: FFmpeg is driven through `ffmpeg::run_with_progress`, and a job is
interrupted through the same `TaskControl` a download uses.

Each job runs up to three passes, stopping at the first that produces a file:

1. **Repackage.** Attempted when the target container can already hold the
   source codecs -- H.264 and AAC moving from MKV into MP4, say. No encoder is
   named at all, so it is I/O bound and lossless. Asking for a smaller frame or
   a specific bitrate rules this pass out, since a copy would ignore it.
2. **GPU encode.** Attempted when hardware acceleration is enabled. It is absent
   on most machines without an NVIDIA card, so it is only ever an attempt.
3. **CPU encode.** Always last, and what actually guarantees a result.

Conversions run one at a time: encoding saturates every core it is given, so a
second job alongside finishes neither any sooner. Nothing is written over the
source -- the output name is made unique in its directory at the moment the job
runs, so converting `clip.mkv` to MP4 beside itself yields `clip.mp4`, and doing
it again yields `clip (2).mp4`.

The job list is deliberately not persisted. An interrupted encode has no
resumable state, and re-running one nobody asked for would burn a CPU at sign-in.

### Single source of truth

Two rules the codebase holds to:

- Platform detection lives only in `providers/detect.rs`. The front end asks
  over IPC rather than keeping a second copy of the host patterns.
- Format selection lives only in `downloader/plan.rs`. The options panel calls
  it rather than re-deriving the same rules in TypeScript.
- The set of conversion targets lives only in `converter.rs`. The Convert screen
  fetches it (`convert_formats`) rather than listing formats a second time, so
  it cannot offer one the backend would refuse.

---

## Development

Requirements: Node 20+, Rust 1.77+, and the MSVC build tools.

```bash
npm install
npm run app:dev      # Vite + Tauri, hot reload
```

Other scripts:

```bash
npm run build          # type-check and build the front end
npm run app:build      # production build + NSIS installer
npm run android:build  # signed release APKs for arm64, armv7 and x86_64
npm run android:dev    # run on a connected phone or emulator
npm run test:rust      # Rust unit and hermetic integration tests
npm run test:python    # the DNS fallback the phone app loads into yt-dlp
npm run test:online    # network tests (installs the engine, downloads real files)
npm run lint:rust      # clippy, warnings denied
```

The Android build needs JDK 17, the Android SDK with NDK r29, and the Rust
targets `aarch64-linux-android`, `armv7-linux-androideabi` and
`x86_64-linux-android`, with `JAVA_HOME`, `ANDROID_HOME` and `NDK_HOME` set.
Release APKs are signed from `src-tauri/gen/android/keystore.properties`, which
is not in the repository:

```properties
storeFile=C:/path/to/universal-downloader.jks
storePassword=...
keyAlias=universal-downloader
keyPassword=...
```

Without it the release build is produced unsigned. Keep the keystore safe: an
APK signed with a different key cannot update an installed copy.

There is no JavaScript linter configured. `typescript-eslint` does not yet
support TypeScript 7, and forcing it past its peer range would leave a parser
running against a compiler API it does not understand. The TypeScript config
carries the checks that would otherwise be delegated to lint rules — `strict`,
`noUnusedLocals`, `noUnusedParameters`, `noUncheckedIndexedAccess`,
`noFallthroughCasesInSwitch` — and should be revisited once the plugin
catches up.

### Tests

- `src-tauri/src/**` — unit tests next to the code they cover: URL
  classification, format selection, file naming, speed smoothing, pause/cancel
  signalling, engine output parsing, FFmpeg argument construction.
- `src-tauri/tests/resume.rs` — chunking and resume, against a purpose-built
  local HTTP server. Hermetic: no network, and the server's range behaviour is
  a controlled variable rather than a guess about some CDN.
- `src-tauri/tests/pipeline.rs` — `#[ignore]`d online tests that install the
  engine, read live metadata, and download a real file end to end.
- `src-tauri/tests/photos.rs` — `#[ignore]`d online tests that read photo posts
  on each platform and download real pictures through the queue's own path,
  plus a check that videos on the same platforms still go to the engine.

### Driving the UI during development

`scripts/drive-ui.mjs` talks to the running app's WebView over the Chrome
DevTools Protocol, so real flows can be exercised against the real backend:

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
  ./src-tauri/target/debug/universal-downloader.exe &

node scripts/drive-ui.mjs type "https://..."
node scripts/drive-ui.mjs press Enter
node scripts/drive-ui.mjs wait "text==Download" 30000
node scripts/drive-ui.mjs click "text==Download"
node scripts/drive-ui.mjs text
```

`scripts/capture_window.ps1` captures the window to a PNG (via `PrintWindow`
with `PW_RENDERFULLCONTENT`, which a plain screen copy would render black).

### Generated assets

```bash
python scripts/generate_icon.py       # the mark, flat on transparency, at 1024
npx tauri icon src-tauri/icons/app-icon.json -o src-tauri/icons
python scripts/generate_licenses.py   # third-party licence list for About
```

The mark is a six-blade aperture and nothing else: no tile, no plate, no
backing. It is the same shape in three hands — `scripts/generate_icon.py`,
`src/components/layout/Logo.tsx` and `extension/make-icons.mjs` — which have to
be changed together.

The Tauri CLI is pointed at `app-icon.json` rather than straight at a PNG for
two reasons. A bare-PNG run writes its own default background into
`gen/android/app/src/main/res/values/ic_launcher_background.xml`, losing the
colour the adaptive icon sits on; and Android's launcher mask crops a
foreground that reaches the canvas edge, so the manifest points at the inset
`icon-source-fg.png` instead. Every run also drops a `src-tauri/icons/ios/`
directory this project has no target for; delete it afterwards. There is nothing
separate to generate for the notification area — the tray takes the window icon
the bundle already carries.

---

## Where things are stored

Everything the app writes lives under `%APPDATA%\UniversalDownloader` (on
Android, in the app's private storage):

```
library.db     settings, history and the durable queue (SQLite, WAL)
tools/         managed copies of yt-dlp and ffmpeg
cache/         thumbnails, bounded by the configured cache limit
temp/          in-flight downloads; a finished file is moved out
logs/          app.log and error.log, rotated by size
```

Downloads themselves go to your own Downloads folder by default.

## Privacy

No account, no ads, no analytics, no telemetry. Links are resolved by tools
running on this machine, and the only network requests made are to the media
source itself and — when you ask for it — to GitHub to fetch the two tools.

The phone app also asks GitHub whether a newer build exists. And when the
phone's own DNS cannot find a site's address — a VPN or Private DNS server that
does not answer, or a filter that refuses the name — the app asks a public
DNS-over-HTTPS resolver (Cloudflare's 1.1.1.1 or Google's 8.8.8.8) instead.
Only the site's host name is sent, and nothing is asked of it while the phone's
DNS answers.

Thumbnails are fetched once, cached on disk and handed to the interface as data
URLs, so the content-security policy can forbid remote image origins outright
and re-opening History contacts nobody.

## Licences

Universal Downloader is released under the [MIT License](LICENSE).

Third-party licences are listed in the app's About screen, generated into
`src-tauri/resources/licenses.json`. Inter is used under the SIL Open Font
License; its licence text ships in `src/assets/fonts/`. The phone's intro film
uses [Remotion](https://www.remotion.dev/license), which is free for
individuals, non-profits and companies of up to three people; a larger company
building this app needs a Remotion company licence.

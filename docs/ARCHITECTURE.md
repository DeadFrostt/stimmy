# Architecture

Two crates. `stimmy-core` is everything that isn't drawing, with no dependency on GPUI.
`stimmy-desktop` is the app.

| Area | Where |
| --- | --- |
| Stremio protocol, add-ons, search, streams | `stimmy-core/src/client.rs`, `model.rs`, `protocol.rs` |
| Accounts, cloud library, watch history | `stimmy-core/src/account.rs` |
| Choosing what Play does and which source | `stimmy-core/src/playback.rs` |
| Downloads | `stimmy-core/src/downloads.rs` |
| Artwork cache, ambient light, mattes | `stimmy-core/src/artwork.rs` |
| Application state | `stimmy-desktop/src/app.rs` |
| Interface | `stimmy-desktop/src/ui/` |
| Video engines | `stimmy-desktop/src/player/` |

## Video

GPUI draws the interface through [GPUI Kit](https://github.com/longbridge/gpui-kit), pinned
exactly because both make breaking changes between releases. `player/mod.rs` holds the `Engine`
trait and the `Frame` type; `player/macos/` and `player/linux/` implement it.

**macOS** owns a native view inserted directly beneath GPUI's view in the window, and GPUI's
layer goes transparent while the player is open. Controls composite over real video with no
copying, and hardware decoding and libass stay intact. Two things have to hold for this:

- **The root must stay transparent.** Kit's root fills the window with the theme's background,
  which hides the video while audio plays on. `apply_theme` clears it, and every page paints
  its own canvas.
- **libmpv must not load a file before its render context exists.** `vo=libmpv` can't start
  without one, and the context is created in the layer's first GL draw, so `loadfile` waits.

`prefers_compatibility` sends MKV, AVI, WebM, TS, and FLV to libmpv. Everything else tries
AVFoundation first and falls back to libmpv if that fails. libmpv is opened at runtime, so the
app runs without it and says what's needed if a video requires it. `build.rs` sets the search
order: bundled, Homebrew, then the copy inside IINA.

**Linux** can't use that approach: Wayland has no subwindows, so there is no native view to
put beneath GPUI's. libmpv is instead driven through its software render API
(`MPV_RENDER_API_TYPE_SW`), which writes BGRA into a buffer that the player hands to GPUI as a
`RenderImage` and draws with `img()`. A dedicated `stimmy-video` thread renders, because a
1080p frame takes long enough that doing it on the interface thread would stutter the
controls. The cost is software decoding (`hwdec=no`) and a copy per frame; the benefit is that
it works on any compositor. mpv renders at the video's own size shrunk to fit the viewport and
never enlarged, so GPUI's GPU scaling does the rest. `MPV_RENDER_PARAM_SW_POINTER` takes the
buffer itself rather than a pointer to it, and both the pointer and the stride must be 64-byte
aligned or mpv's scaler takes a slow path.

## Play

Play never asks. `play_video` is the only way in: the player opens at once with the title art
and finds its source behind it. A finished download comes first. Otherwise `rank_sources` orders
the add-ons' results by what played last for this title (a `bingeGroup` match, then add-on and
label, then add-on), falling back to the last source used for anything, then the add-ons' own
order. A failed source falls through to the next, including one that hangs past 40 seconds with
no picture. A source is only remembered once it has shown a frame.

## Downloads

Partial files live on disk under the record's id, so pausing, losing the network, and quitting
all resume from the bytes already there through HTTP Range. Two run at once, while the app is
open.

- **File type comes from the bytes**, since the player picks its engine from the extension and
  debrid links often have none.
- **Expired links ask the add-on again**, keeping partial data if the same source comes back.
  That includes dead links answered with an HTML page, which are never saved as video.
- **Subtitles** are saved once per language, and only ever read back from the downloads folder,
  since track URLs can come from add-ons.
- **HLS** is limited to unencrypted streams with muxed audio, joined into one file. Encrypted,
  live, and separate-audio streams say so rather than failing midway.

## Accounts

The account is read, not written: add-ons and library come down from Stremio, and nothing
overwrites the remote collection. Progress recorded here stays on this Mac and merges by recency.
Removing a cloud title hides it locally.

Sessions are in the Keychain, never in JSON. Add-on URLs can carry tokens, so files holding them
are mode 0600, and errors never quote a URL: `reqwest`'s error text prints them, so `Error`
discards it. A guest never touches the Keychain. If it refuses a save, sign-in still works for
that session and the app says why it won't persist.

Ad-hoc signed builds can prompt for Keychain access again after each rebuild, since macOS ties
"Always Allow" to the code signature.

Linux has no Keychain, so sessions go to the Secret Service through `keyring` when a provider is
running, and otherwise to a mode 0600 `accounts.json` in the data folder.

## Not built yet

Media keys and Now Playing, AirPlay, Picture in Picture, opening a direct stream or local file,
`stremio://` links from outside the app, and notarized packaging. Controls carry AccessKit roles
and labels, but haven't been checked with VoiceOver itself. Torrent sources and sources needing
custom request headers show as unsupported.

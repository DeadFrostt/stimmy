<img src="assets/icon.svg" width="88" height="88" alt="Stimmy icon">

# Stimmy

A native [Stremio](https://www.stremio.com) client for macOS, written in Rust with
[GPUI](https://www.gpui.rs). No browser engine and no sidebar: the window is lit by whatever
you're looking at.

- **Play means play.** Continue Watching and Play start straight away, from where you stopped,
  on the source you last used. If a source fails, it moves on to the next by itself.
- **Plays what you throw at it.** AVFoundation for MP4 and HLS, libmpv for MKV and the rest,
  with embedded and add-on subtitles and audio track selection.
- **Downloads that survive.** Pausing, losing the network, and quitting all resume where they
  stopped. Expired links refresh themselves, and everything plays offline.
- **Your Stremio account.** Sign in to bring your add-ons, library, and history. Sessions stay
  in the Keychain, and nothing is written back to your account.
- **Fast and quiet.** Drawn directly on the GPU, with no CPU use at rest. It honors Reduce Motion.

## Run it

You need macOS, Xcode's command line tools, and [rustup](https://rustup.rs), which installs the
pinned Rust version by itself.

```sh
cargo run --release -p stimmy-desktop
```

MP4 and HLS play with nothing else installed. MKV and similar formats need libmpv:
`brew install mpv`, or have [IINA](https://iina.io) installed.

Stimmy comes with public catalogs from Cinemeta. To watch anything, add a stream add-on in
**Settings › Add-ons**, or sign in to use your account's.

## Keyboard

| Keys | Action |
| --- | --- |
| ⌘1 to ⌘4, ⌘, | Home, Search, Library, Downloads, Settings |
| ⌘R | Refresh |
| ⌘D | Save the open title |
| Arrows, Return | Move between posters and open one |
| `[` `]` | Previous or next featured title |
| Space, ←, →, ↑, ↓ | In the player: pause, seek, volume |
| F, M, C | In the player: full screen, mute, subtitles |
| Esc | Back |

## Learn more

- [Architecture](docs/ARCHITECTURE.md): how the crates, video, and downloads work
- [Design](DESIGN.md): the visual system and motion

## Legal

Stimmy is an independent project, not affiliated with Stremio. It hosts no content and includes
no stream sources: what you watch comes from add-ons you choose, and you're responsible for
using them lawfully.

[MIT](LICENSE). See also the [third-party notices](THIRD_PARTY_NOTICES.md).

# Third-party notices

Stimmy is built on [GPUI Kit](https://github.com/longbridge/gpui-kit) and GPUI, from
[Zed](https://github.com/zed-industries/zed), both under the Apache License, Version
2.0. A copy is in `crates/stimmy-desktop/LICENSE-GPUI-APACHE`. Icons include GPUI
Kit's set, from Lucide (ISC license). Other Rust dependencies keep their own licenses,
which Cargo can list.

Stimmy opens [libmpv](https://mpv.io) at runtime if it's on the system, for formats
AVFoundation can't play. It isn't included in this repository. If you bundle libmpv
with a build you distribute, the license of that libmpv build applies to the bundle:
Homebrew's is GPL, and LGPL builds carry their own obligations.

The light that travels around controls is an original implementation inspired by
[Border Beam](https://libraries.dev/beam). No code from it is included.

Film and series data and artwork come from the Cinemeta add-on and any others you
install. Copyright stays with the respective rights holders, and none of it is bundled.

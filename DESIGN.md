# Design

## The idea

The window is lit by whatever you're looking at. The interface itself is neutral;
all colour comes from the artwork. Hover a poster and the room's light shifts to
that title.

## Colour

| Role | Value |
| --- | --- |
| Canvas | `#0a0a0b`, deliberately with no hue |
| Text | white at 96% / 74% (secondary) / 60% (tertiary) |
| Hairlines | white at 13% |
| Quiet control | white at 11%, 18% on hover |
| Primary control | white at 94%, with canvas-coloured text |
| Error | `hsl(10, 90%, 80%)` |

Two images are derived from artwork in `stimmy-core`, since GPUI has no element
blur or mask:

- **Ambient**: heavily blurred, saturated, and capped in brightness so text keeps
  its contrast. Colourless art gets a lower ceiling, so it never reads as a flat
  grey slab. Dithered to stop dark gradients banding.
- **Matte**: the backdrop with real alpha falling off downward and toward the
  lower left, so it dissolves into the light. Draw it with `motion::backdrop_image`, at its own
  aspect ratio: `Cover` would crop the fade, and a relative width boxes large art at its
  file height, and either leaves a hard edge.

Title logos are trimmed to their visible bounds so they align with text. Dark ones
are recoloured white, judged by brightness rather than luma so a red logo stays
red. Logos with no transparency fall back to type.

## Type

The system sans (`.SystemUIFont`). Hero title 44, section 16 semibold, body 14,
controls and metadata 13, pill labels 12 medium. Studio title art is the display
voice wherever a catalog has it.

## Layout

No sidebar. A full-bleed stage starts behind the navigation, and content runs the
full window width with 48px gutters, 28px below 900px wide. Home is the stage, with Play as its main action, then
Continue Watching (wide cards with progress), then horizontal shelves. Search and
Library are poster grids with an All/Films/Series pill. Titles are a stage, then
Sources for a film, or a season pill and episode rows for a series.

Settings has its own small pill: General, Account, Add-ons, About. Refresh and About
live there, never beside the main navigation.

## The pill

One component everywhere: main navigation, Settings, seasons, filters. No fill and
no blur, with a hairline outline. Segment widths come from measured text, so the
space around every label matches. One indicator marks the selection, and a fainter
one follows the pointer. Both are stateful springs, and the edge moving toward its
destination leads, so the indicator stretches between tabs and redirects mid-flight.

It's also where brief status appears: "Saved Nocturne" widens the pill for a moment,
with nothing to dismiss.

## Controls

Every control comes from `ui/widgets.rs`: 32px tall, or 28px for chips, fully
rounded, 13px medium. Text fields use `field()`, the same height as the buttons
beside them. Use these rather than sizing a control by hand, because hand-sized
controls are how rows stop lining up. Icons come from Kit's set, plus `assets/` for
the few it lacks. Saving is always "Save" and "In Library".

## Play

Pressing Play plays. Continue Watching cards, Play buttons, and episode rows go straight
into the player with no source list, resuming where they stopped. The title art holds
the screen while a source is found, so there's no step in between. Choosing by hand is
always a second, smaller action: the chevron on an episode row, or Source in the player.

## Downloads

A tab of its own, with its count in the pill while anything is running. Plain rows, not
cards: poster, title, one line of status, and a thin progress bar. Download controls show
their state wherever they appear, so a row reads Download, 42%, or a check.

## The player

The film fills the window. Controls appear over it on soft scrims with no panel,
and fade with the traffic lights and cursor after 2.6s. Pausing shows the title art
and synopsis. While a video starts, its backdrop and title art hold the screen, so
opening a film feels like the lights going down.

The timeline thickens under the pointer and shows the time at the cursor. The only
solid surface is the track menu, because text over moving pictures needs one.

## Motion

The idea is light, which is what the whole design is built on: it arrives with a title,
travels around whatever is working, and settles. `ui/motion.rs` holds the vocabulary.

- **Lap of light:** Beam's border beam, rebuilt for GPUI (which has no shaders) in the
  title's own accent colour, drawn from its artwork. One lap acknowledges something new:
  a hovered poster, Play when a title arrives. It loops only while work is under way,
  around search or a download's poster, and stops when the work does.
- **The stage:** a new backdrop settles from slightly larger while the last one fades
  beneath it, and the title, details, text, and actions arrive in sequence. As the page
  scrolls, the backdrop falls behind it and the text fades.
- **Tabs** slide from the direction you moved along the pill. A title rises into place.
- **Posters** grow from their centre into their own margin, so nothing around them moves,
  and relight the room after 140ms so sweeping across a shelf doesn't strobe. Results
  cascade once as they arrive, with the wait capped.
- **Player:** a flash in the middle of the picture says what just happened (pause, seek,
  volume), and the controls sink away as they fade. The timeline takes the film's colour.
- **Small acknowledgements:** the Save check pops, download bars move on a spring, and the
  pill's status fades in.

Springs on anything that can be interrupted; short eased fades otherwise. Nothing animates
at rest except a loop tied to real work, and at rest the app uses no CPU. Reduce Motion removes
movement, laps, and hover relighting, and keeps fades and the player's flash, since those
confirm actions.

Scrolling: every scroll container uses `restrict_scroll_to_axis()`. Without it a
sideways trackpad swipe over a shelf also moves the page.

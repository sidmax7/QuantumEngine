<p align="center">
  <img src="packaging/assets/logo-128.png" width="96" alt="QuantumEngine logo">
</p>

# QuantumEngine

Control a **JBL Quantum 810 Wireless** headset on Linux: noise cancelling,
TalkThru, sidetone, lighting and EQ. There's a desktop app, a tray icon, a
Plasma widget and a command-line tool, and no Windows or vendor software is
needed.

A small background service talks to the USB dongle (`0ecb:2069`) directly;
everything else talks to the service. The protocol was reverse-engineered from
the device and every command was checked on real hardware.

## Features

| | GUI | CLI |
|---|---|---|
| Battery level | ✓ | ✓ |
| Noise control: Off / ANC / TalkThru | ✓ | ✓ |
| Sidetone: Off / Low / Mid / High | ✓ | ✓ |
| Lights on/off | ✓ | ✓ |
| Lighting colours, speed, brightness, effect (logo and ring) | ✓ | ✓ |
| Microphone live/muted, game/chat dial | ✓ (live) | ✓ |
| Live event stream | — | ✓ `watch` |
| Serial, firmware, paired host name | ✓ | ✓ |
| EQ: JBL's headset tuning (810), 19 sound profiles, your own and imported ones | ✓ | ✓ |
| Volume of the headset's output, up to 150% | ✓ | ✓ `volume` |
| Virtual 7.1 surround | ✓ | ✓ |
| Updates from GitHub releases | ✓ | ✓ |

**Not available:** these settings aren't exposed over the dongle, or haven't
been found yet.
- Spatial sound on the headset itself (QuantumEngine renders
  [virtual surround](#virtual-surround) on the PC instead, as JBL's app does)
- Auto-off timer
- Charging state
- The headset's own hardware volume over the dongle. The [volume](#volume)
  control sets the PC's volume for the headset's output instead.

**Only the Quantum 810 Wireless is verified.** Other Quantum models may use a
different protocol, so QuantumEngine never sends them anything until you say so:
it shows an unverified Harman dongle, lets you check it with reads only, and
only then lets you use it and allow noise control, sidetone and lights one at a
time. Open the **Device** tab, or run `quantumenginectl unverified`. The
report it makes is what you paste into an issue.

Every model QuantumEngine can recognise, with what's known to work on each, is
in `crates/jbl-quantum/src/models.rs`; `quantumenginectl models` lists them, and
the hardware table in the top-level README is generated from the same list.

## Install

### Arch / CachyOS / Manjaro

Download the `.pkg.tar.zst` from the
[latest release](https://github.com/sidmax7/QuantumEngine/releases/latest) and
install it:

```sh
sudo pacman -U quantumengine-*-x86_64.pkg.tar.zst
```

Or build it yourself (run `makepkg` as your normal user, not with `sudo`):

```sh
cd packaging
makepkg -si
```

The package installs the udev rule for you. Unplug the dongle and plug it back
in afterwards. Once installed, the app updates itself (see [Updates](#updates)).

### Anywhere else

You need a Rust toolchain (1.88 or newer), Node.js with npm, and WebKitGTK 4.1
(`webkit2gtk-4.1` on Arch, `libwebkit2gtk-4.1-dev` on Debian and Ubuntu).

```sh
npm --prefix ui ci
npm --prefix ui run build
cargo build --release --workspace --features quantumengine/custom-protocol
sudo install -Dm755 target/release/quantumengine target/release/quantumengined \
  target/release/quantumenginectl -t /usr/local/bin/

sudo install -Dm644 udev/70-jbl-quantum.rules /etc/udev/rules.d/70-jbl-quantum.rules
sudo udevadm control --reload
```

Then replug the dongle. To get a menu entry, copy
`packaging/quantumengine.desktop` to `~/.local/share/applications/`. To start
the background service at login, copy `packaging/quantumengine-tray.desktop`
to `~/.config/autostart/`. For the Plasma widget:

```sh
kpackagetool6 --type Plasma/Applet --install packaging/plasmoid/org.quantumengine.plasmoid
```

The udev rule gives the logged-in user access to the dongle. Without it, both
tools report a permission error.

## Desktop app

Run `quantumengine`, or use **QuantumEngine** in your app menu.

The window is built around a drawing of the headset that shows its state:
noise control, lights, the microphone and the game/chat dial, with the
battery above it. It follows the headset live, so pressing the ANC button,
moving the dial or muting the microphone shows up straight away. Below the
drawing are four sections: Sound, Lights, EQ and Device. While a change is
being applied, the controls are locked until the headset confirms it.

**RGB lighting is still under development.** It works, but the lighting editor
and the drawing may not match what the headset is showing: the headset can't
report its lighting colours, so the app remembers what you last applied. While you edit them, the drawing previews the colours and
effect; nothing reaches the headset until you press **Apply to headset**,
which also switches the lights on.

## Background service and tray

`quantumengined` runs in the background. It is the only program that opens
the dongle, and it also runs the EQ, checks for updates and shows a tray
icon: left-click opens the window, and the menu has noise control, sidetone,
the lights and the EQ without opening it. The window, `quantumenginectl` and
the widget all talk to it over D-Bus and start it if it isn't running, so
running several of them at once never means several programs fighting over
the headset. (It used to be `quantumengine --tray`, which still starts it.)

The package starts it automatically at login. To stop that, disable
"QuantumEngine (tray)" in your desktop's autostart settings.

## Plasma widget

On KDE Plasma, add **QuantumEngine** from *Add Widgets…* for battery, noise
control, lights and the EQ on the panel or desktop, without opening the
window. It needs the `plasma5support` package, which most Plasma installs
already have.

## Equalizer

The headset has no EQ of its own; JBL's QuantumENGINE applies it on the PC,
and so does this. Turn it on in the window, the tray menu, the widget or with
`quantumenginectl eq on`. It has two layers:

- **Headset tuning**: JBL's own correction curve for the Quantum 810. *JBL
  stereo* is the one QuantumENGINE uses with surround off (a gentle bass cut);
  *JBL surround* is the one it puts under its spatial audio. Other models are
  left flat: JBL's files give the Quantum 400 and 800 no stereo correction at
  all, and their surround curves aren't built in yet (the 400's differs
  between its ears).
- **Profile**: one of 19 ten-band sound profiles: Flat, Bass Boost,
  Cinematic, FPS, MOBA, RPG, and presets for specific games (CS2, Valorant,
  Apex Legends and more).

- **Your own profiles**: under *Yours* in the EQ section, **New** copies the
  current profile into an editor. There you name it and shape up to 18 bands
  (bell, low or high shelf, high- or low-pass) by dragging their points on the
  graph or typing exact values. With the EQ on you hear changes as you make
  them; **Save and use** keeps them, **Cancel** goes back. **Import…** adds a
  curve from an [AutoEQ](https://github.com/jaakkopasanen/AutoEq) or
  Equalizer APO `ParametricEQ.txt`, or an EasyEffects preset.

It runs as a PipeWire filter in front of the headset's output: the **game**
output on the 810, whose chat output is left alone, or the single output of a
model like the Quantum 400. The headset is found by its USB ID, so the EQ
follows whichever listed model is plugged in, and waits on the 810's output
when none is. Your default output device doesn't change. Switching profiles is
instant, with no gap in the audio. Curves that boost are turned down by just
enough that they can never clip. It needs PipeWire and WirePlumber 0.5 or
newer, which current desktops already use.

If EasyEffects is also processing your output, the sound is equalized twice;
use one or the other.

The curves come from JBL QuantumENGINE's configuration files, as extracted by
[alessandromrc](https://github.com/alessandromrc/jbl-quantum-4-linux) and
tested by [DefSteam](https://github.com/DefSteam). Thank you both.

## Volume

The window's Sound section and the widget have a volume slider for the
headset's output, and `quantumenginectl volume 80` sets it from a terminal. It
is the same volume your desktop's own slider and volume keys move, so they
always agree.

The slider goes up to **150%**. Past 100% is a boost: it makes quiet sound
louder, but loud passages can distort, and the EQ already turns the whole
output down a little to keep its own boosts from clipping, so a boost can give
that back.

## Virtual surround

On Windows, you set the headset to 7.1 in the sound settings and JBL's
software turns the eight channels into 3D sound for headphones. QuantumEngine
does the same with PipeWire. Switch on **Virtual surround 7.1** (in the
window's Sound section, the tray menu, or `quantumenginectl surround on`) and
it adds a 7.1 output called "QuantumEngine Surround 7.1" and makes it your
default. Switching it off makes the headset the default again.

Games then see a 7.1 device: pick 7.1 or surround in their audio settings
where they offer it. Stereo plays from two virtual speakers in front of you.

How it renders follows the structure of JBL's own HX3D virtualiser, worked out
from QuantumENGINE's configuration and binaries without reusing any of their
code or data:

- Each speaker channel is played through a binaural room impulse response:
  what each ear hears from a speaker at its 7.1 position (front ±30°, centre,
  sides ±90°, rear ±150°) in a small, furnished room. The service builds these
  when surround starts, from MIT's KEMAR dummy-head measurements (which
  libmysofa ships), early reflections from a simulated room, and a short
  reverb tail that's different in each ear. The room is what puts sound
  outside your head.
- The head measurements are diffuse-field equalised: their average response
  over every direction is removed, so the dummy head's own ear and microphone
  colouring goes and only the directional cues remain.
- Bass below 120 Hz skips all that and goes to both ears, and the LFE channel
  is added to both ears at half level, so the low end stays clean.
- On the 810, the EQ's headset tuning switches to JBL surround while surround
  is on and back to JBL stereo when it's off, as QuantumENGINE does. (A tuning
  set to off stays off.)

It needs PipeWire's convolver, which comes with `pipewire-audio`, and
libmysofa. It won't sound exactly like JBL's processing, whose head-model data
and DTS stage are theirs and stay out of this project.

## Updates

Once a day the tray service asks GitHub whether there's a newer release, and
shows a notification when there is. If you installed the package, **Install**
(in the notification, the window, the tray menu or the widget, or
`quantumenginectl update install`) downloads it, checks it against the
release's checksum, and installs it with pacman after asking for your
password. The service then restarts on the new version.

That daily check is the app's only network traffic. Switch it off in the
window under *About and updates*, or with `quantumenginectl update checks off`.
The checksum guards against a broken download, not against a compromised
GitHub account; it trusts this repository exactly as much as building it with
`makepkg` does.

## Command line

```console
$ quantumenginectl
battery:   50%
anc:       on
sidetone:  off
mic:       active
dial:      16 (full game)
lights:    off
paired to: PC-myhost #1
serial:    XXXXXX-XXXXXXXXX
firmware:  0.7.2, 0.8.2, 1.0.2, 0.7.2
eq:        on, CS2 (headset tuning: stereo)
volume:    85%

$ quantumenginectl anc talkthru
talkthru

$ quantumenginectl eq bass boost
on, Bass Boost (headset tuning: stereo)

$ quantumenginectl eq import ~/Downloads/ParametricEQ.txt "My target"
imported as "My target"

$ quantumenginectl volume 120
volume:    120% (boosted)

$ quantumenginectl update
QuantumEngine 0.3.0 is up to date

$ quantumenginectl rgb ring spin-clockwise '#ff0000' '#0000ff' --speed 75 --brightness 60
applied

$ quantumenginectl --json battery
{"battery_percent":50}

$ quantumenginectl watch
14:02:11  anc: off
14:02:15  dial: 12 (game)
```

Run `quantumenginectl --help` for the full list. Setting commands wait until the
headset confirms the change, then print the new state. They exit non-zero if
the headset is off or doesn't respond.

**Lighting effects** (JBL's own seven, by the numbers its app uses):
- solid, breathing, wave, glitch: logo and ring
- spin-clockwise, spin-counter-clockwise, rise: ring only

`--speed` (25-100, higher is faster; JBL's app offers 25, 50, 75 and 100) sets how fast the
animation runs, and `--brightness` dims the colours (solid only: the headset
runs its animated effects at full brightness). Up to ten colours per
zone. The names earlier versions used (fade, instant, chase, strobe,
chase-strobe-logo, chase-flash-logo) still work.

The two zones are the **logo** and the **ring** around each earcup. Both
earcups always show the same thing.

## Troubleshooting

- **"no JBL Quantum 810 dongle found"**: the dongle isn't plugged in. Check with
  `lsusb | grep 0ecb:2069`.
- **"cannot access the dongle"**: the udev rule is missing, or the dongle wasn't
  replugged after you installed it.
- **"the dongle is connected but the headset is not"**: the dongle is fine,
  but the headset is off or out of range. Switch it off and on again.
- **Occasional "Connection timed out"**: the wireless link is weak or dropping.
  Try again, and avoid running several tools against the headset at once.

## Working on the window

The window is a [Tauri](https://tauri.app) app: a small Rust shell in
`crates/quantumengine` and a Vue interface in `ui/`. To work on the interface
with made-up data and no headset, run the Vite dev server and open it in a
browser:

```sh
npm --prefix ui ci
npm --prefix ui run dev     # http://localhost:1420/?scenario=ready
```

Other scenarios are `update`, `low`, `off`, `nodongle`, `noaccess` and
`down`. Add `&section=lights`, `eq` or `device` to open another section. To
run the real window against the dev server, leave that running and use
`cargo run -p quantumengine` (debug builds load the dev server).

The TypeScript bindings in `ui/src/bindings.ts` are generated from the Rust
commands. After changing those, regenerate them with
`QUANTUMENGINE_UPDATE_BINDINGS=1 cargo test -p quantumengine`.

## Releasing

1. `scripts/bump-version.sh X.Y.Z` sets the version in `Cargo.toml`,
   `Cargo.lock`, the PKGBUILD and the widget.
2. Commit, then `git tag -a vX.Y.Z -m "vX.Y.Z" && git push --follow-tags`.

Pushing the tag runs `.github/workflows/release.yml`, which builds the package
on Arch, runs the tests, and publishes a GitHub release with the package, its
`.sha256`, and a PKGBUILD with the source checksum filled in (that copy is the
one for the AUR). Installed copies pick the release up within a day. Every
push to `master` is also linted and tested by `.github/workflows/check.yml`.

Both can also run on your own machine, for when GitHub Actions can't. They
use podman (or docker) and the same clean `archlinux:base-devel` container:

- `scripts/check-local.sh` runs the checks on a copy of the working tree.
- `scripts/release-local.sh vX.Y.Z` builds the package from the pushed tag
  and publishes the release with `gh`; add `--build-only` to stop before
  publishing. Building in the container matters: a host `makepkg` on CachyOS
  builds for that CPU only (`-march=native`), which crashes on older ones.

## Library

The `jbl-quantum` crate in `crates/jbl-quantum` has everything the two tools
use. It has no dependencies apart from `libc`.

```rust
use jbl_quantum::{Anc, Headset, CONFIRM_TIMEOUT};

let headset = Headset::open_first()?;
println!("{}%", headset.battery()?);
headset.set_anc_confirmed(Anc::TalkThru, CONFIRM_TIMEOUT)?;
```

Its crate docs cover how the device behaves. A few points matter to anyone
writing a client:
- Some reads make the headset send events.
- Switching between ANC and TalkThru passes briefly through Off.
- The headset ignores lighting writes sent too close together.

## Safety

The library only uses reports that were mapped and tested. The firmware-update
reports aren't defined anywhere in the code, so no bug in these tools can reach
them.

This is an unofficial project and isn't affiliated with JBL or Harman. You use
it at your own risk.

## License

Licensed under either the [MIT license](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), whichever you prefer.

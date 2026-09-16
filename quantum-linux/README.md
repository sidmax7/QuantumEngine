<p align="center">
  <img src="packaging/assets/logo-128.png" width="96" alt="QuantumEngine logo">
</p>

# QuantumEngine

Control a **JBL Quantum 810 Wireless** headset on Linux: noise cancelling,
TalkThru, sidetone and lighting. There's a desktop app and a command-line tool,
and no Windows or vendor software is needed.

It talks to the USB dongle (`0ecb:2069`) directly and needs no background
service. The protocol was reverse-engineered from the device and every command
was checked on real hardware.

## Features

| | GUI | CLI |
|---|---|---|
| Battery level | ✓ | ✓ |
| Noise control: Off / ANC / TalkThru | ✓ | ✓ |
| Sidetone: Off / Low / Mid / High | ✓ | ✓ |
| Lights on/off | ✓ | ✓ |
| Lighting colours, brightness, effect (logo and ring) | ✓ | ✓ |
| Microphone live/muted, game/chat dial | ✓ (live) | ✓ |
| Live event stream | — | ✓ `watch` |
| Serial, firmware, paired host name | ✓ | ✓ |

**Not available:** these settings aren't exposed over the dongle, or haven't
been found yet.
- EQ
- Spatial sound
- Auto-off timer
- Charging state
- Volume

For volume, use your system mixer.

**Only the Quantum 810 Wireless is supported.** Other Quantum models may use a
different protocol, so the tools refuse to talk to them.

## Install

### Arch / CachyOS / Manjaro

```sh
cd packaging
makepkg -si
```

The package installs the udev rule for you. Unplug the dongle and plug it back
in afterwards.

### Anywhere else

You need a Rust toolchain (1.88 or newer) and the usual GUI libraries (OpenGL,
xkbcommon, and Wayland or X11).

```sh
cargo install --path crates/quantumenginectl
cargo install --path crates/quantumengine

sudo install -Dm644 udev/70-jbl-quantum.rules /etc/udev/rules.d/70-jbl-quantum.rules
sudo udevadm control --reload
```

Then replug the dongle. To get a menu entry, copy
`packaging/quantumengine.desktop` to `~/.local/share/applications/`.

The udev rule gives the logged-in user access to the dongle. Without it, both
tools report a permission error.

## Desktop app

Run `quantumengine`, or use **QuantumEngine** in your app menu.

The window follows the headset live. Pressing the ANC button, moving the
game/chat dial or muting the microphone updates it straight away. While a change
is being applied, the controls are locked until the headset confirms it.

The headset can't report its lighting colours, so the app remembers what you
last applied. Applying lighting switches the lights on, because the headset only
takes new colours at that moment.

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

$ quantumenginectl anc talkthru
talkthru

$ quantumenginectl rgb both chase '#ff0000' '#0000ff' --brightness 60
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

**Lighting effects:**
- fade
- instant
- chase
- strobe
- chase-strobe-logo
- chase-flash-logo
- fast
- chase-blink-logo
- rainbow-logo
- spectrum

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

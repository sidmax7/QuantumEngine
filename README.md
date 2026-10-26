<p align="center">
  <img src="quantum-linux/packaging/assets/logo-128.png" width="96" alt="QuantumEngine logo">
</p>

# QuantumEngine for Linux

Unofficial Linux control software for the **JBL Quantum 810 Wireless** headset.
You get noise cancelling, TalkThru, sidetone and RGB lighting without Windows or
JBL's QuantumENGINE app.

It includes:

- **QuantumEngine**, a desktop app that follows the headset live.
- **`quantumengined`**, a background service with a system tray menu.
- **Plasma widget**, for KDE Plasma panel and desktop control.
- **`quantumenginectl`**, a command-line tool with JSON output for scripts.
- **`jbl-quantum`**, a Rust library for anyone building their own tools.

A background service (`quantumengined`) talks to the USB dongle directly;
the app, widget and CLI talk to it over D-Bus. No Windows or vendor software needed.

## What you can do

- See battery level, microphone state and the game/chat dial position
- Switch noise control: **Off**, **ANC** or **TalkThru**
- Set sidetone (hearing your own voice): **Off**, **Low**, **Mid** or **High**
- Turn the lights on or off
- Set colours, brightness and animation for the logo and ring lighting (RGB works, but is still under development; see below)
- Equalizer: JBL headset tuning and 19 sound profiles
- Virtual 7.1 surround sound

Auto-off timer and hardware volume aren't supported over the dongle.

> **RGB lighting is still under development.** Setting colours, brightness and
> effects on the headset works, but the app's lighting editor and the headset
> drawing don't always show what the headset is doing. The headset can't report
> its lighting, so the window only knows what it last applied itself. Expect
> rough edges here, and treat what the window shows as a preview, not a
> readout. Fixes are planned.

## Supported hardware

<!-- models:start (generated from quantum-linux/crates/jbl-quantum/src/models.rs; run QUANTUMENGINE_UPDATE_README=1 cargo test -p jbl-quantum to refresh) -->
| Model | USB IDs | Family | Battery and status | Noise control | Sidetone | Lights | Lighting effects | EQ |
|---|---|---|---|---|---|---|---|---|
| Quantum 810 Wireless | `0ecb:2069` | Wireless flagship | ✅ | ✅ | ✅ | ✅ | ✅ Effects, speed and colours as in JBL's own app. | ✅ |
| Quantum 800 | `0ecb:1f47` `0ecb:203e` | Wireless flagship | 🟢 Every read answers; it reports no serial number. (issue #2) | 🟡 Only with the mic boom raised; with it down the headset plays an error tone. (issue #2) | 🟢 (issue #2) | 🟢 (issue #2) | ⚪ Rebuilt in 0.4.5 to work the way JBL's own app drives the 800; not tried yet. | ⚪ |
| Quantum 850 Wireless | `0ecb:219a` `0ecb:219b` `0ecb:219c` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 910 Wireless | `0ecb:2088` `0ecb:209c` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 910P Console Wireless | `0ecb:20a2` `0ecb:20a4` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 910X Wireless for Xbox | `0ecb:209d` `0ecb:209f` `0ecb:2147` `0ecb:214a` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 950 Wireless | `0ecb:2127` `0ecb:2128` `0ecb:2129` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 950X Wireless | `0ecb:2141` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 950X Wireless for Xbox | `0ecb:213e` `0ecb:2140` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum One | `0ecb:1f48` `0ecb:203a` `0ecb:20c2` | Wireless flagship | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 350 Wireless | `0ecb:2047` `0ecb:206b` `0ecb:20b2` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 360 Wireless | `0ecb:2115` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 360P Console Wireless | `0ecb:20ab` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 360X Wireless for Xbox | `0ecb:20a5` `0ecb:2142` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 520 Wireless | `0ecb:218d` `0ecb:218e` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 520X Wireless for Xbox | `0ecb:21a7` `0ecb:21a9` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 600 | `0ecb:1f46` `0ecb:203c` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 610 Wireless | `0ecb:205c` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 650 Wireless | `0ecb:2125` `0ecb:2126` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 650X Wireless for Xbox | `0ecb:213b` `0ecb:213d` | Mid-range | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 50C | `0ecb:2112` | Wired | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 300 | `0ecb:1f52` | Wired | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ | ⚪ |
| Quantum 400 | `0ecb:1f4c` `0ecb:2039` `0ecb:20ee` | Wired | 🟢 Mic, dial, lights, serial and firmware answer; being wired, it has no battery or wireless link. (issue #4) | ➖ (JBL's app has none for it) | ⚪ | ⚪ | ⚪ JBL's own app drives its logo the same way as the 810's lights. | 🟡 Choose QuantumEngine as the sound output yourself; it only attaches to the 810 on its own. (issue #4) |
| Quantum TWS | `0ecb:208a` `0ecb:209b` | Earbuds | not a headset | — | — | — | — | — |
| Quantum TWS Air | `0ecb:20be` | Earbuds | not a headset | — | — | — | — | — |
| Quantum Stream | `0ecb:205f` | Microphone | not a headset | — | — | — | — | — |
| Quantum Stream Studio | `0ecb:20b0` | Microphone | not a headset | — | — | — | — | — |
| Quantum Stream Talk | `0ecb:20af` | Microphone | not a headset | — | — | — | — | — |
| Quantum Stream Wireless | `0ecb:20ed` | Microphone | not a headset | — | — | — | — | — |

✅ verified by the project · 🟢 reported working · 🟡 works, with a catch · ⚪ untested · ❌ doesn't work · ➖ not on this model
<!-- models:end -->

Own a model that isn't verified? Run the check in the window's **Device** tab
and send the results with the
[model report form](https://github.com/sidmax7/QuantumEngine/issues/new?template=model-report.yml).
Each report updates that model's row.

Only the 810 has been tested. Other models might share the protocol, share part
of it, or not share it at all, and there is no way to know without trying. So
QuantumEngine doesn't ignore them and doesn't guess either: if it finds another
Harman headset dongle, the window's **Device** tab offers to check it, safely:

1. **Check what it answers.** Reads only. Nothing is written to the headset.
   You get a plain-text report to paste into an issue, which is how a model
   moves from unverified to supported.
2. **Use this headset.** Shows what the headset reports, read-only.
3. **Allow changes.** Noise control, sidetone and lights, each off until you turn
   it on, because they send commands built for the 810.

Opening the device may need permission first. The window's **Allow access…**
button (or `quantumenginectl unverified access`) adds a udev rule for that one
device ID after you enter your password. The same steps are available as
`quantumenginectl unverified`, `unverified probe`, `unverified use on` and
`unverified allow anc sidetone lights`.

## Install

### Arch / CachyOS / EndeavourOS / Manjaro (Recommended)

Download the `.pkg.tar.zst` from the
[latest release](https://github.com/sidmax7/QuantumEngine/releases/latest) and
install it:

```sh
sudo pacman -U quantumengine-*-x86_64.pkg.tar.zst
```

Or build it from source with `makepkg`:

```sh
git clone https://github.com/sidmax7/QuantumEngine.git
cd QuantumEngine/quantum-linux/packaging
makepkg -si
```

The package installs the udev rule, desktop launcher, tray service and Plasma widget for you.
Unplug the dongle and plug it back in afterwards. Once installed, the app updates itself.

### Anywhere else

You need a Rust toolchain (1.88 or newer), Node.js with npm, and WebKitGTK 4.1
(`webkit2gtk-4.1` on Arch, `libwebkit2gtk-4.1-dev` on Debian and Ubuntu).

```sh
git clone https://github.com/sidmax7/QuantumEngine.git
cd QuantumEngine/quantum-linux
npm --prefix ui ci
npm --prefix ui run build
cargo build --release --workspace --features quantumengine/custom-protocol
sudo install -Dm755 target/release/quantumengine target/release/quantumengined \
  target/release/quantumenginectl -t /usr/local/bin/

sudo install -Dm644 udev/70-jbl-quantum.rules /etc/udev/rules.d/70-jbl-quantum.rules
sudo udevadm control --reload
```

Then replug the dongle. Now start **QuantumEngine** from your app menu, or run a command:

```sh
quantumenginectl                 # show everything
quantumenginectl anc talkthru    # switch to TalkThru
quantumenginectl lights off
```

Full install notes, command reference and troubleshooting are in
[quantum-linux/README.md](quantum-linux/README.md).

## Repository layout

```
quantum-linux/
├── crates/
│   ├── jbl-quantum/          library: device protocol and headset API
│   ├── quantumengine-dbus/   D-Bus IPC interface
│   ├── quantumengined/       background service and system tray
│   ├── quantumenginectl/     command-line tool
│   └── quantumengine/        desktop app (Tauri)
├── packaging/                PKGBUILD, desktop entries, Plasma widget
├── udev/                     device permission rule
└── ui/                       Vue 3 frontend
```

## How it works

The dongle exposes a vendor-specific HID interface. Settings are read and
written with HID feature reports, and the headset sends events when you use its
physical controls. The protocol was worked out on Linux from the device itself,
and every command the tools send has been tested on real hardware.

The firmware-update commands aren't defined anywhere in the code, so no bug in
these tools can reach them.

## Disclaimer

This is an independent project and isn't affiliated with or endorsed by JBL or
Harman International. "JBL" and "Quantum" are their trademarks. You use this
software at your own risk.

## Credits

The curves come from JBL QuantumENGINE's configuration files, as extracted by
[alessandromrc](https://github.com/alessandromrc/jbl-quantum-4-linux) and
tested by [DefSteam](https://github.com/DefSteam). Thank you both.

## License

Licensed under either the [MIT license](quantum-linux/LICENSE-MIT) or the
[Apache License 2.0](quantum-linux/LICENSE-APACHE), whichever you prefer.

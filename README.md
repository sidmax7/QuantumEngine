# QuantumEngine for Linux

Unofficial Linux control software for the **JBL Quantum 810 Wireless** headset.
You get noise cancelling, TalkThru, sidetone and RGB lighting without Windows or
JBL's QuantumENGINE app.

It includes:

- **Quantum Linux**, a small desktop app that follows the headset live.
- **`quantumctl`**, a command-line tool with JSON output for scripts.
- **`jbl-quantum`**, a Rust library for anyone building their own tools.

Everything talks to the USB dongle directly. It needs no background service,
no root and no vendor software.

## What you can do

- See battery level, microphone state and the game/chat dial position
- Switch noise control: **Off**, **ANC** or **TalkThru**
- Set sidetone (hearing your own voice): **Off**, **Low**, **Mid** or **High**
- Turn the lights on or off
- Set colours, brightness and animation for the logo and ring lighting

EQ, spatial sound and the auto-off timer aren't supported yet.

## Supported hardware

| Headset | USB ID | Status |
|---|---|---|
| JBL Quantum 810 Wireless | `0ecb:2069` | ✅ tested |
| Other JBL Quantum models | — | ❌ not supported |

Other models may use a different protocol, so the tools refuse to talk to them
rather than guess. If you own one and want to help add it, open an issue.

## Quick start

On Arch-based systems (Arch, CachyOS, EndeavourOS, Manjaro):

```sh
git clone https://github.com/sidmax81/quantum-linux.git
cd quantum-linux/quantum-linux/packaging
makepkg -si
```

On other distributions, with a Rust toolchain installed:

```sh
cd quantum-linux
cargo install --path crates/quantum-gui
cargo install --path crates/quantumctl
sudo install -Dm644 udev/70-jbl-quantum.rules /etc/udev/rules.d/70-jbl-quantum.rules
sudo udevadm control --reload
```

Then unplug the dongle and plug it back in. Now start **Quantum Linux** from
your app menu, or run a command:

```sh
quantumctl                 # show everything
quantumctl anc talkthru    # switch to TalkThru
quantumctl lights off
```

Full install notes, command reference and troubleshooting are in
[quantum-linux/README.md](quantum-linux/README.md).

## Repository layout

```
quantum-linux/
├── crates/
│   ├── jbl-quantum/    library: device protocol and headset API
│   ├── quantumctl/     command-line tool
│   └── quantum-gui/    desktop app (egui)
├── udev/               device permission rule
└── packaging/          PKGBUILD and desktop entry
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

## License

Licensed under either the [MIT license](quantum-linux/LICENSE-MIT) or the
[Apache License 2.0](quantum-linux/LICENSE-APACHE), whichever you prefer.

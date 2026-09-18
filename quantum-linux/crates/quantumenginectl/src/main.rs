//! Command-line control for JBL Quantum 810 Wireless headsets.
//!
//! Every command except `devices` (a plain hidraw enumeration, with no
//! device access) goes through the `quantumengine --tray` service over
//! D-Bus, starting it if it is not already running — this CLI never opens
//! the headset itself. See `quantumengine-dbus` for the shared contract.

mod dbus;

use jbl_quantum::{Colour, Effect, Headset, Zone, MAX_SLOTS};
use quantumengine_dbus::{HeadsetProxyBlocking, ZoneLightingArg};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
quantumenginectl — control a JBL Quantum 810 Wireless headset

USAGE:
    quantumenginectl [--json] <COMMAND>

COMMANDS:
    status                      everything at once (default)
    battery                     battery percentage
    anc [off|on|talkthru]       show or set noise cancelling
    sidetone [off|low|mid|high] show or set microphone monitoring
    mic                         whether the microphone is live
    dial                        game/chat dial position, 0-16
    lights [on|off]             show or set the lights
    rgb <zone> <effect> <colour>...
                                configure lighting and switch it on
    watch                       stream state changes until interrupted
    devices                     list detected dongles

    rgb zones:    logo, ring, both
    rgb effects:  fade, instant, chase, strobe, chase-strobe-logo,
                  chase-flash-logo, fast, chase-blink-logo, rainbow-logo,
                  spectrum
    rgb colours:  1-5 hex colours, e.g. '#ff0000'
    rgb options:  --brightness N   (0-100, default 100)

OPTIONS:
    --json      machine-readable output
    -h, --help  this help
";

/// How often `watch` re-reads state looking for a change.
const WATCH_POLL_EVERY: Duration = Duration::from_millis(150);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut json = false;
    let mut rest: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => rest.push(arg),
        }
    }

    match run(&rest, json) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("quantumenginectl: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String], json: bool) -> Result<(), String> {
    let (command, params) = args
        .split_first()
        .map_or(("status", &[] as &[String]), |(c, p)| (c.as_str(), p));

    if command == "devices" {
        let nodes = Headset::discover().map_err(|e| e.to_string())?;
        if json {
            let entries: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "{{\"path\":\"{}\",\"name\":\"{}\"}}",
                        n.path.display(),
                        escape(&n.name)
                    )
                })
                .collect();
            println!("[{}]", entries.join(","));
        } else if nodes.is_empty() {
            println!("no JBL Quantum 810 dongle found");
        } else {
            for n in &nodes {
                println!("{}  {}", n.path.display(), n.name);
            }
        }
        return Ok(());
    }

    let proxy = dbus::connect()?;

    match command {
        "status" => cmd_status(&proxy, json)?,
        "battery" => {
            require_ready(&proxy)?;
            let pct = proxy.battery().map_err(|e| e.to_string())?;
            if json {
                println!("{{\"battery_percent\":{pct}}}");
            } else {
                println!("{pct}%");
            }
        }
        "anc" => match params.first() {
            None => {
                let mode = proxy.anc().map_err(|e| e.to_string())?;
                emit(json, "anc", &mode);
            }
            Some(value) => {
                parse_anc(value)?;
                let mode = proxy.set_anc(value).map_err(|e| e.to_string())?;
                emit(json, "anc", &mode);
            }
        },
        "sidetone" => match params.first() {
            None => {
                let level = proxy.sidetone().map_err(|e| e.to_string())?;
                emit(json, "sidetone", &level);
            }
            Some(value) => {
                parse_sidetone(value)?;
                let level = proxy.set_sidetone(value).map_err(|e| e.to_string())?;
                emit(json, "sidetone", &level);
            }
        },
        "mic" => {
            let active = proxy.mic_active().map_err(|e| e.to_string())?;
            if json {
                println!("{{\"mic_active\":{active}}}");
            } else {
                println!("{}", if active { "active" } else { "inactive" });
            }
        }
        "dial" => {
            let pos = proxy.dial().map_err(|e| e.to_string())?;
            if json {
                println!("{{\"dial\":{pos},\"label\":\"{}\"}}", dial_label(pos));
            } else {
                println!("{pos} ({})", dial_label(pos));
            }
        }
        "lights" => match params.first() {
            None => {
                let on = proxy.lights_on().map_err(|e| e.to_string())?;
                emit(json, "lights", if on { "on" } else { "off" });
            }
            Some(value) => {
                let on = parse_on_off(value)?;
                let on = proxy.set_lights(on).map_err(|e| e.to_string())?;
                emit(json, "lights", if on { "on" } else { "off" });
            }
        },
        "rgb" => {
            require_ready(&proxy)?;
            cmd_rgb(&proxy, params)?;
            emit(json, "lighting", "applied");
        }
        "watch" => cmd_watch(&proxy, json)?,
        other => return Err(format!("unknown command '{other}' (try --help)")),
    }
    Ok(())
}

/// Fails with the same wording `Headset::open_first` used to give when
/// there was no live link, instead of silently showing default values.
fn require_ready(proxy: &HeadsetProxyBlocking) -> Result<(), String> {
    match proxy.link().map_err(|e| e.to_string())?.as_str() {
        "ready" => Ok(()),
        "no_dongle" => Err(jbl_quantum::Error::NotFound.to_string()),
        "no_access" => Err(proxy.link_detail().unwrap_or_default()),
        _ => Err(jbl_quantum::Error::NotConnected.to_string()),
    }
}

fn cmd_status(proxy: &HeadsetProxyBlocking, json: bool) -> Result<(), String> {
    let link = proxy.link().map_err(|e| e.to_string())?;
    let battery = proxy.battery().map_err(|e| e.to_string())?;
    let anc = proxy.anc().map_err(|e| e.to_string())?;
    let sidetone = proxy.sidetone().map_err(|e| e.to_string())?;
    let mic_active = proxy.mic_active().map_err(|e| e.to_string())?;
    let dial = proxy.dial().map_err(|e| e.to_string())?;
    let lights_on = proxy.lights_on().map_err(|e| e.to_string())?;
    let device_name = proxy.device_name().map_err(|e| e.to_string())?;
    let serial = proxy.serial().map_err(|e| e.to_string())?;
    let firmware = proxy.firmware().map_err(|e| e.to_string())?;

    if json {
        let fw: Vec<String> = firmware.iter().map(|f| format!("\"{f}\"")).collect();
        println!(
            "{{\"link\":\"{link}\",\"battery_percent\":{battery},\"anc\":\"{anc}\",\"sidetone\":\"{sidetone}\",\
             \"mic_active\":{mic_active},\"dial\":{dial},\"lights_on\":{lights_on},\"device_name\":\"{}\",\"serial\":\"{}\",\"firmware\":[{}]}}",
            escape(&device_name),
            escape(&serial),
            fw.join(",")
        );
    } else {
        if link != "ready" {
            println!("headset:   {}", link_note(&link));
            println!("           values below are defaults until it reconnects");
        }
        println!("battery:   {battery}%");
        println!("anc:       {anc}");
        println!("sidetone:  {sidetone}");
        println!("mic:       {}", if mic_active { "active" } else { "inactive" });
        println!("dial:      {dial} ({})", dial_label(dial));
        println!("lights:    {}", if lights_on { "on" } else { "off" });
        println!("paired to: {device_name}");
        println!("serial:    {serial}");
        println!("firmware:  {}", firmware.join(", "));
    }
    Ok(())
}

fn link_note(link: &str) -> &'static str {
    match link {
        "no_dongle" => "no dongle plugged in",
        "no_access" => "cannot access the dongle",
        _ => "not connected (dongle present, headset off or out of range)",
    }
}

fn cmd_rgb(proxy: &HeadsetProxyBlocking, params: &[String]) -> Result<(), String> {
    let mut brightness: u8 = 100;
    let mut positional: Vec<&str> = Vec::new();
    let mut it = params.iter();
    while let Some(arg) = it.next() {
        if arg == "--brightness" {
            let value = it.next().ok_or("--brightness needs a value 0-100")?;
            brightness = value.parse().map_err(|_| format!("invalid brightness '{value}'"))?;
        } else {
            positional.push(arg);
        }
    }

    let [zone_name, effect_name, colours @ ..] = positional.as_slice() else {
        return Err("usage: quantumenginectl rgb <zone> <effect> <colour>... (try --help)".into());
    };

    let zones: &[Zone] = match *zone_name {
        "logo" => &[Zone::Logo],
        "ring" => &[Zone::Ring],
        "both" => &Zone::ALL,
        other => return Err(format!("unknown zone '{other}' (logo, ring, both)")),
    };

    let effect = Effect::from_name(effect_name).ok_or_else(|| {
        let names: Vec<&str> = Effect::KNOWN.iter().map(|(_, n)| *n).collect();
        format!("unknown effect '{effect_name}' (one of: {})", names.join(", "))
    })?;

    if colours.is_empty() {
        return Err("give at least one colour, e.g. '#ff0000'".into());
    }
    if colours.len() > MAX_SLOTS {
        return Err(format!("at most {MAX_SLOTS} colours, got {}", colours.len()));
    }

    let palette: Vec<Colour> = colours
        .iter()
        .map(|c| Colour::parse_hex(c).ok_or_else(|| format!("invalid colour '{c}' (expected #rrggbb)")))
        .collect::<Result<_, _>>()?;

    let args: Vec<ZoneLightingArg> = zones
        .iter()
        .map(|&zone| ZoneLightingArg {
            zone: match zone {
                Zone::Logo => "logo".to_string(),
                Zone::Ring => "ring".to_string(),
            },
            brightness,
            colours: palette.iter().map(|c| (c.r, c.g, c.b)).collect(),
            effect: effect.0,
        })
        .collect();

    proxy.set_lighting(args).map_err(|e| e.to_string())
}

/// State `watch` compares between polls, since the service exposes changes
/// as properties rather than raw device events — see `quantumengine-dbus`.
/// A physical Bluetooth reconnect or an unrecognised report, which the old
/// direct-hardware `watch` could show, has no property here and so is not
/// visible over D-Bus.
#[derive(Clone, PartialEq)]
struct WatchState {
    link: String,
    battery: u8,
    anc: String,
    sidetone: String,
    mic_active: bool,
    dial: u8,
    lights_on: bool,
}

impl WatchState {
    fn read(proxy: &HeadsetProxyBlocking) -> Result<Self, String> {
        Ok(Self {
            link: proxy.link().map_err(|e| e.to_string())?,
            battery: proxy.battery().map_err(|e| e.to_string())?,
            anc: proxy.anc().map_err(|e| e.to_string())?,
            sidetone: proxy.sidetone().map_err(|e| e.to_string())?,
            mic_active: proxy.mic_active().map_err(|e| e.to_string())?,
            dial: proxy.dial().map_err(|e| e.to_string())?,
            lights_on: proxy.lights_on().map_err(|e| e.to_string())?,
        })
    }
}

fn cmd_watch(proxy: &HeadsetProxyBlocking, json: bool) -> Result<(), String> {
    let mut prev = WatchState::read(proxy)?;
    loop {
        std::thread::sleep(WATCH_POLL_EVERY);
        let cur = WatchState::read(proxy)?;
        if cur.link != prev.link {
            print_watch_event(json, "link", &format!("\"{}\"", cur.link), &format!("link: {}", cur.link));
        }
        if cur.battery != prev.battery {
            print_watch_event(json, "battery", &cur.battery.to_string(), &format!("battery: {}%", cur.battery));
        }
        if cur.anc != prev.anc {
            print_watch_event(json, "anc", &format!("\"{}\"", cur.anc), &format!("anc: {}", cur.anc));
        }
        if cur.sidetone != prev.sidetone {
            print_watch_event(json, "sidetone", &format!("\"{}\"", cur.sidetone), &format!("sidetone: {}", cur.sidetone));
        }
        if cur.mic_active != prev.mic_active {
            let text = if cur.mic_active { "active" } else { "inactive" };
            print_watch_event(json, "mic_active", &cur.mic_active.to_string(), &format!("mic: {text}"));
        }
        if cur.dial != prev.dial {
            print_watch_event(json, "dial", &cur.dial.to_string(), &format!("dial: {} ({})", cur.dial, dial_label(cur.dial)));
        }
        if cur.lights_on != prev.lights_on {
            let text = if cur.lights_on { "on" } else { "off" };
            print_watch_event(json, "lights", &cur.lights_on.to_string(), &format!("lights: {text}"));
        }
        prev = cur;
    }
}

fn print_watch_event(json: bool, key: &str, json_value: &str, text: &str) {
    if json {
        println!("{{\"event\":\"{key}\",\"value\":{json_value}}}");
    } else {
        println!("{}  {text}", clock());
    }
}

fn emit(json: bool, key: &str, value: &str) {
    if json {
        println!("{{\"{key}\":\"{value}\"}}");
    } else {
        println!("{value}");
    }
}

fn dial_label(pos: u8) -> &'static str {
    match pos {
        0 => "full chat",
        1..=7 => "chat",
        8 => "centre",
        9..=15 => "game",
        _ => "full game",
    }
}

/// Local wall-clock time as HH:MM:SS, without pulling in a date crate.
fn clock() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Validates an ANC value locally, so a typo fails fast without waiting on
/// the service. The service parses it again regardless (see
/// `dbus_service::parse_anc`), since it must trust no other caller either.
fn parse_anc(value: &str) -> Result<(), String> {
    match value {
        "off" | "on" => Ok(()),
        "talkthru" => Ok(()),
        other => Err(format!("unknown anc mode '{other}' (off, on, talkthru)")),
    }
}

fn parse_sidetone(value: &str) -> Result<(), String> {
    match value {
        "off" | "low" | "mid" | "high" => Ok(()),
        other => Err(format!("unknown sidetone level '{other}' (off, low, mid, high)")),
    }
}

fn parse_on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(format!("expected on or off, got '{other}'")),
    }
}

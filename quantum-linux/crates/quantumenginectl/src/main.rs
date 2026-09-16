//! Command-line control for JBL Quantum 810 Wireless headsets.

use jbl_quantum::{Anc, Colour, Effect, Headset, Sidetone, Status, Zone, ZoneLighting, CONFIRM_TIMEOUT, MAX_SLOTS};
use std::process::ExitCode;

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
    watch                       stream events until interrupted
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

fn run(args: &[String], json: bool) -> Result<(), jbl_quantum::Error> {
    let (command, params) = args
        .split_first()
        .map_or(("status", &[] as &[String]), |(c, p)| (c.as_str(), p));

    if command == "devices" {
        let nodes = Headset::discover()?;
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

    let headset = Headset::open_first()?;

    match command {
        "status" => {
            let s = headset.status()?;
            if json {
                println!("{}", status_json(&s));
            } else {
                print_status(&s);
            }
        }
        "battery" => {
            let pct = headset.battery()?;
            if json {
                println!("{{\"battery_percent\":{pct}}}");
            } else {
                println!("{pct}%");
            }
        }
        "anc" => match params.first().map(String::as_str) {
            None => {
                let mode = headset.anc()?;
                emit(json, "anc", mode.as_str());
            }
            Some(value) => {
                let mode = headset.set_anc_confirmed(parse_anc(value)?, CONFIRM_TIMEOUT)?;
                emit(json, "anc", mode.as_str());
            }
        },
        "sidetone" => match params.first().map(String::as_str) {
            None => {
                let level = headset.sidetone()?;
                emit(json, "sidetone", level.as_str());
            }
            Some(value) => {
                let level =
                    headset.set_sidetone_confirmed(parse_sidetone(value)?, CONFIRM_TIMEOUT)?;
                emit(json, "sidetone", level.as_str());
            }
        },
        "mic" => {
            let active = headset.mic_active()?;
            if json {
                println!("{{\"mic_active\":{active}}}");
            } else {
                println!("{}", if active { "active" } else { "inactive" });
            }
        }
        "dial" => {
            let pos = headset.dial()?;
            if json {
                println!("{{\"dial\":{pos},\"label\":\"{}\"}}", dial_label(pos));
            } else {
                println!("{pos} ({})", dial_label(pos));
            }
        }
        "lights" => match params.first().map(String::as_str) {
            None => {
                let on = headset.lights_on()?;
                emit(json, "lights", if on { "on" } else { "off" });
            }
            Some(value) => {
                let on = headset.set_lights_confirmed(parse_on_off(value)?, CONFIRM_TIMEOUT)?;
                emit(json, "lights", if on { "on" } else { "off" });
            }
        },
        "rgb" => {
            if !headset.connected()? {
                return Err(jbl_quantum::Error::NotConnected);
            }
            cmd_rgb(&headset, params)?;
            emit(json, "lighting", "applied");
        }
        "watch" => loop {
            let event = headset.next_event()?;
            if json {
                println!("{}", event_json(&event));
            } else {
                println!("{}  {}", clock(), event_text(&event));
            }
        },
        other => {
            return Err(jbl_quantum::Error::Invalid(format!(
                "unknown command '{other}' (try --help)"
            )))
        }
    }
    Ok(())
}

fn cmd_rgb(headset: &Headset, params: &[String]) -> Result<(), jbl_quantum::Error> {
    let mut brightness: u8 = 100;
    let mut positional: Vec<&str> = Vec::new();
    let mut it = params.iter();
    while let Some(arg) = it.next() {
        if arg == "--brightness" {
            let value = it.next().ok_or_else(|| {
                jbl_quantum::Error::Invalid("--brightness needs a value 0-100".into())
            })?;
            brightness = value.parse().map_err(|_| {
                jbl_quantum::Error::Invalid(format!("invalid brightness '{value}'"))
            })?;
        } else {
            positional.push(arg);
        }
    }

    let [zone_name, effect_name, colours @ ..] = positional.as_slice() else {
        return Err(jbl_quantum::Error::Invalid(
            "usage: quantumenginectl rgb <zone> <effect> <colour>... (try --help)".into(),
        ));
    };

    let zones: &[Zone] = match *zone_name {
        "logo" => &[Zone::Logo],
        "ring" => &[Zone::Ring],
        "both" => &Zone::ALL,
        other => {
            return Err(jbl_quantum::Error::Invalid(format!(
                "unknown zone '{other}' (logo, ring, both)"
            )))
        }
    };

    let effect = Effect::from_name(effect_name).ok_or_else(|| {
        let names: Vec<&str> = Effect::KNOWN.iter().map(|(_, n)| *n).collect();
        jbl_quantum::Error::Invalid(format!(
            "unknown effect '{effect_name}' (one of: {})",
            names.join(", ")
        ))
    })?;

    if colours.is_empty() {
        return Err(jbl_quantum::Error::Invalid(
            "give at least one colour, e.g. '#ff0000'".into(),
        ));
    }
    if colours.len() > MAX_SLOTS {
        return Err(jbl_quantum::Error::Invalid(format!(
            "at most {MAX_SLOTS} colours, got {}",
            colours.len()
        )));
    }

    let palette: Vec<Colour> = colours
        .iter()
        .map(|c| {
            Colour::parse_hex(c).ok_or_else(|| {
                jbl_quantum::Error::Invalid(format!("invalid colour '{c}' (expected #rrggbb)"))
            })
        })
        .collect::<Result<_, _>>()?;

    let request: Vec<ZoneLighting> = zones
        .iter()
        .map(|&zone| ZoneLighting { zone, brightness, palette: palette.clone(), effect })
        .collect();
    headset.set_lighting_zones(&request)
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

fn print_status(s: &Status) {
    if !s.connected {
        println!("headset:   not connected (dongle present, headset off or out of range)");
        println!("           values below are the dongle's last known state");
    }
    println!("battery:   {}%", s.battery_percent);
    println!("anc:       {}", s.anc.as_str());
    println!("sidetone:  {}", s.sidetone.as_str());
    println!(
        "mic:       {}",
        if s.mic_active { "active" } else { "inactive" }
    );
    println!("dial:      {} ({})", s.dial, dial_label(s.dial));
    println!("lights:    {}", if s.lights_on { "on" } else { "off" });
    println!("paired to: {}", s.device_name);
    println!("serial:    {}", s.serial);
    println!("firmware:  {}", s.firmware.join(", "));
}

fn status_json(s: &Status) -> String {
    let firmware: Vec<String> = s.firmware.iter().map(|f| format!("\"{f}\"")).collect();
    format!(
        "{{\"connected\":{},\"battery_percent\":{},\"anc\":\"{}\",\"sidetone\":\"{}\",\
         \"mic_active\":{},\"dial\":{},\"lights_on\":{},\"device_name\":\"{}\",\"serial\":\"{}\",\"firmware\":[{}]}}",
        s.connected,
        s.battery_percent,
        s.anc.as_str(),
        s.sidetone.as_str(),
        s.mic_active,
        s.dial,
        s.lights_on,
        escape(&s.device_name),
        escape(&s.serial),
        firmware.join(",")
    )
}

fn event_json(e: &jbl_quantum::Event) -> String {
    use jbl_quantum::Event;
    match e {
        Event::Anc(m) => format!("{{\"event\":\"anc\",\"value\":\"{}\"}}", m.as_str()),
        Event::Bluetooth(on) => format!("{{\"event\":\"bluetooth\",\"value\":{on}}}"),
        Event::MicActive(on) => format!("{{\"event\":\"mic_active\",\"value\":{on}}}"),
        Event::Lights(on) => format!("{{\"event\":\"lights\",\"value\":{on}}}"),
        Event::Battery(pct) => format!("{{\"event\":\"battery\",\"value\":{pct}}}"),
        Event::Dial(pos) => format!("{{\"event\":\"dial\",\"value\":{pos}}}"),
        Event::Unknown { report_id, data } => {
            let hex: Vec<String> = data.iter().map(|b| format!("\"{b:02x}\"")).collect();
            format!(
                "{{\"event\":\"unknown\",\"report_id\":{report_id},\"data\":[{}]}}",
                hex.join(",")
            )
        }
    }
}

fn event_text(e: &jbl_quantum::Event) -> String {
    use jbl_quantum::Event;
    let on_off = |b: bool| if b { "on" } else { "off" };
    match e {
        Event::Anc(m) => format!("anc: {}", m.as_str()),
        Event::Bluetooth(on) => format!("bluetooth: {}", if *on { "active" } else { "inactive" }),
        Event::MicActive(on) => format!("mic: {}", if *on { "active" } else { "inactive" }),
        Event::Lights(on) => format!("lights: {}", on_off(*on)),
        Event::Battery(pct) => format!("battery: {pct}%"),
        Event::Dial(pos) => format!("dial: {pos} ({})", dial_label(*pos)),
        Event::Unknown { report_id, data } => {
            let hex: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
            format!("unknown report 0x{report_id:02x}: {}", hex.join(" "))
        }
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

fn parse_anc(value: &str) -> Result<Anc, jbl_quantum::Error> {
    match value {
        "off" => Ok(Anc::Off),
        "on" => Ok(Anc::On),
        "talkthru" | "tt" => Ok(Anc::TalkThru),
        other => Err(jbl_quantum::Error::Invalid(format!(
            "unknown anc mode '{other}' (off, on, talkthru)"
        ))),
    }
}

fn parse_sidetone(value: &str) -> Result<Sidetone, jbl_quantum::Error> {
    match value {
        "off" => Ok(Sidetone::Off),
        "low" => Ok(Sidetone::Low),
        "mid" => Ok(Sidetone::Mid),
        "high" => Ok(Sidetone::High),
        other => Err(jbl_quantum::Error::Invalid(format!(
            "unknown sidetone level '{other}' (off, low, mid, high)"
        ))),
    }
}

fn parse_on_off(value: &str) -> Result<bool, jbl_quantum::Error> {
    match value {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(jbl_quantum::Error::Invalid(format!(
            "expected on or off, got '{other}'"
        ))),
    }
}

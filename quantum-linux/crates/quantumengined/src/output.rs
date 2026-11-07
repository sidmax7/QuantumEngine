//! Which sound output is the headset.
//!
//! PipeWire gives each USB sound card as a device carrying the USB vendor and
//! product ID, and each of its outputs as a sink carrying only the card name
//! and the profile. So the headset is found by its USB ID, through the
//! catalogue in `jbl_quantum::models`, and its output by the device it
//! belongs to. The EQ and surround then follow whichever model is plugged in,
//! with no per-model card names to type in.

use crate::pwgraph;
use jbl_quantum::models::{self, Kind, Model};
use serde_json::Value;

/// A headset's sound output, as the EQ and surround attach to it.
#[derive(Debug, PartialEq, Eq)]
pub struct Output {
    pub model: &'static Model,
    /// The ALSA card name, e.g. `JBL Quantum810 Wireless`.
    pub card: String,
    /// The profile of the chosen sink, e.g. `stereo-game-output`.
    pub profile: String,
    pub node_name: String,
    pub node_id: u64,
}

/// The first catalogued headset found in the PipeWire graph, if any.
pub fn find() -> Result<Option<Output>, String> {
    Ok(find_in(&pwgraph::dump()?))
}

/// [`find`] over an already dumped graph.
///
/// A headset with several outputs (the 810's Game and Chat) gets its game
/// output; one with a single output, such as the Quantum 400's, gets that.
pub fn find_in(objects: &[Value]) -> Option<Output> {
    let mut devices: Vec<(u64, &'static Model, String)> = objects
        .iter()
        .filter_map(|o| {
            if o["type"] != "PipeWire:Interface:Device" {
                return None;
            }
            let props = o.pointer("/info/props")?;
            let vendor = hex_id(props["device.vendor.id"].as_str()?)?;
            let product = hex_id(props["device.product.id"].as_str()?)?;
            let model = models::lookup(vendor, product).filter(|m| m.kind() == Kind::Headset)?;
            Some((o["id"].as_u64()?, model, props["alsa.card_name"].as_str()?.to_owned()))
        })
        .collect();
    devices.sort_by_key(|d| d.0);

    devices.into_iter().find_map(|(device, model, card)| {
        let mut sinks: Vec<(u64, &str, &str)> = objects
            .iter()
            .filter_map(|o| {
                if o["type"] != "PipeWire:Interface:Node" {
                    return None;
                }
                let props = o.pointer("/info/props")?;
                if props["media.class"] != "Audio/Sink" || props["device.id"].as_u64() != Some(device) {
                    return None;
                }
                Some((o["id"].as_u64()?, props["node.name"].as_str()?, props["device.profile.name"].as_str()?))
            })
            .collect();
        sinks.sort_by_key(|s| s.0);
        let (id, name, profile) = sinks.iter().find(|s| s.2.contains("game")).or(sinks.first())?;
        Some(Output { model, card, profile: profile.to_string(), node_name: name.to_string(), node_id: *id })
    })
}

/// `0x2069` as the number it is.
fn hex_id(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn device(id: u64, vendor: &str, product: &str, card: &str) -> Value {
        json!({ "id": id, "type": "PipeWire:Interface:Device", "info": { "props": {
            "device.vendor.id": vendor, "device.product.id": product, "alsa.card_name": card,
        } } })
    }

    fn sink(id: u64, device: u64, name: &str, profile: &str) -> Value {
        json!({ "id": id, "type": "PipeWire:Interface:Node", "info": { "props": {
            "media.class": "Audio/Sink", "device.id": device, "node.name": name, "device.profile.name": profile,
        } } })
    }

    /// As PipeWire lists the Quantum 810 (read from a real machine).
    fn quantum_810() -> Vec<Value> {
        vec![
            device(49, "0x0ecb", "0x2069", "JBL Quantum810 Wireless"),
            sink(70, 49, "alsa_output.usb-810.stereo-chat-output", "stereo-chat-output"),
            sink(71, 49, "alsa_output.usb-810.stereo-game-output", "stereo-game-output"),
        ]
    }

    /// The Quantum 400, from the card and sink names in issue #4.
    fn quantum_400() -> Vec<Value> {
        vec![
            device(60, "0x0ecb", "0x20ee", "Quantum 400"),
            sink(80, 60, "alsa_output.usb-JBL_Quantum_400_20160406.1-00.analog-stereo", "analog-stereo"),
        ]
    }

    #[test]
    fn the_810_gets_its_game_output_not_its_chat_output() {
        let o = find_in(&quantum_810()).unwrap();
        assert_eq!(o.model.name, "Quantum 810 Wireless");
        assert_eq!((o.card.as_str(), o.profile.as_str()), ("JBL Quantum810 Wireless", "stereo-game-output"));
        assert_eq!(o.node_id, 71);
    }

    #[test]
    fn the_400_gets_its_only_output() {
        let o = find_in(&quantum_400()).unwrap();
        assert_eq!(o.model.name, "Quantum 400");
        assert_eq!((o.card.as_str(), o.profile.as_str()), ("Quantum 400", "analog-stereo"));
        assert_eq!(o.node_name, "alsa_output.usb-JBL_Quantum_400_20160406.1-00.analog-stereo");
    }

    #[test]
    fn other_sound_cards_microphones_and_unknown_products_are_not_headsets() {
        let mut graph = vec![
            device(30, "0x8086", "0xa170", "HDA Intel PCH"),
            sink(31, 30, "alsa_output.pci.analog-stereo", "analog-stereo"),
            // A Quantum Stream microphone is in the catalogue, but isn't a headset.
            device(40, "0x0ecb", "0x205f", "JBL Quantum Stream"),
            sink(41, 40, "alsa_output.usb-stream", "analog-stereo"),
            // A JBL product the catalogue doesn't know.
            device(50, "0x0ecb", "0xffff", "Mystery"),
            sink(51, 50, "alsa_output.usb-mystery", "analog-stereo"),
        ];
        assert_eq!(find_in(&graph), None);
        graph.extend(quantum_400());
        assert_eq!(find_in(&graph).unwrap().model.name, "Quantum 400");
    }

    #[test]
    fn a_headset_with_no_output_yet_is_not_found() {
        assert_eq!(find_in(&quantum_810()[..1]), None);
    }
}

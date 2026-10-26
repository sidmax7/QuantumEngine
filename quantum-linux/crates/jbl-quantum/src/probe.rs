//! Read-only compatibility probing.
//!
//! This crate is verified against one product only (see [`crate::PRODUCT_ID`]).
//! Other models from the same vendor may share the protocol, may share part of
//! it, or may not share it at all, and there is no way to know without trying.
//! [`probe`](Headset::probe_node) is the safe way to try: it sends nothing but
//! `GET_REPORT` reads of reports this crate already knows, never a write, and
//! reports what answered and whether the answer looked plausible. Its text is
//! meant to be pasted into an issue, so a model's support can be built up from
//! real reports.

use crate::hidraw::{Hidraw, HidrawNode, Transport};
use crate::protocol::feature;
use crate::{Anc, Headset, Sidetone};
use std::fmt;

/// One read, and what came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// What the report is, e.g. `battery`.
    pub name: &'static str,
    pub report: u8,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The device answered. `plausible` says whether the value makes sense for
    /// what the report is supposed to hold; `shown` is how to display it.
    Answered { bytes: Vec<u8>, plausible: bool, shown: String },
    /// No answer, or an error.
    Failed(String),
}

impl Reading {
    pub fn answered(&self) -> bool {
        matches!(self.outcome, Outcome::Answered { .. })
    }

    pub fn plausible(&self) -> bool {
        matches!(self.outcome, Outcome::Answered { plausible: true, .. })
    }
}

/// Everything one probe of one device node found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    pub path: String,
    /// Why the node could not be opened, if it could not.
    pub open_error: Option<String>,
    pub readings: Vec<Reading>,
}

impl ProbeReport {
    /// How many reads answered at all.
    pub fn answered(&self) -> usize {
        self.readings.iter().filter(|r| r.answered()).count()
    }

    /// How many answered with a value that made sense.
    pub fn plausible(&self) -> usize {
        self.readings.iter().filter(|r| r.plausible()).count()
    }

    /// Whether the headset-link read answered: the minimum for treating the
    /// node as this protocol's control interface.
    pub fn answers_link(&self) -> bool {
        self.readings.iter().any(|r| r.report == feature::CONNECTION && r.plausible())
    }

    /// `0ecb:203e`.
    pub fn id(&self) -> String {
        format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
    }
}

impl fmt::Display for ProbeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "QuantumEngine compatibility report (jbl-quantum {})", env!("CARGO_PKG_VERSION"))?;
        writeln!(f, "Device:   {} \"{}\" at {}", self.id(), self.name, self.path)?;
        match crate::models::lookup(self.vendor_id, self.product_id) {
            Some(m) => writeln!(f, "Model:    JBL {} ({}, {})", m.name, m.family.label().to_lowercase(), m.kind().as_str())?,
            None => writeln!(f, "Model:    not in QuantumEngine's list of models")?,
        }
        writeln!(
            f,
            "Verified: no (this library is only verified on {:04x}:{:04x})",
            crate::VENDOR_ID,
            crate::PRODUCT_ID
        )?;
        if let Some(e) = &self.open_error {
            return writeln!(f, "Could not open the device: {e}");
        }
        writeln!(f, "Reads only; nothing was written to the device.")?;
        for r in &self.readings {
            match &r.outcome {
                Outcome::Answered { shown, plausible, .. } => writeln!(
                    f,
                    "  0x{:02X} {:<12} answered   {:<18} {}",
                    r.report,
                    r.name,
                    shown,
                    if *plausible { "plausible" } else { "unexpected value" }
                )?,
                Outcome::Failed(why) => writeln!(f, "  0x{:02X} {:<12} no answer  {}", r.report, r.name, why)?,
            }
        }
        writeln!(
            f,
            "Summary: {} of {} reads answered, {} plausible.",
            self.answered(),
            self.readings.len(),
            self.plausible()
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn printable(bytes: &[u8]) -> bool {
    let text: Vec<u8> = bytes.iter().copied().take_while(|b| *b != 0).collect();
    !text.is_empty() && text.iter().all(|b| (0x20..0x7f).contains(b))
}

/// A one-byte read and a test of its value.
fn byte_read(
    hid: &dyn Transport,
    name: &'static str,
    report: u8,
    sensible: impl Fn(u8) -> bool,
    describe: impl Fn(u8) -> String,
) -> Reading {
    let outcome = match hid.get_feature(report, 1) {
        Ok(bytes) => match bytes.first().copied() {
            Some(b) => Outcome::Answered { plausible: sensible(b), shown: describe(b), bytes },
            None => Outcome::Failed("empty answer".into()),
        },
        Err(e) => Outcome::Failed(e.to_string()),
    };
    Reading { name, report, outcome }
}

/// A multi-byte read and a test of its bytes.
fn bytes_read(
    hid: &dyn Transport,
    name: &'static str,
    report: u8,
    len: usize,
    sensible: impl Fn(&[u8]) -> bool,
) -> Reading {
    let outcome = match hid.get_feature(report, len) {
        Ok(bytes) => Outcome::Answered { plausible: sensible(&bytes), shown: hex(&bytes), bytes },
        Err(e) => Outcome::Failed(e.to_string()),
    };
    Reading { name, report, outcome }
}

impl Headset {
    /// Reads, and only reads, the reports this crate knows, from `node`.
    ///
    /// Never writes. Each read is a plain `GET_REPORT` of a report the 810
    /// answers; the battery read makes the 810 emit a battery event, which is
    /// harmless. Safe to call on a model that has not been verified.
    pub fn probe_node(node: &HidrawNode) -> ProbeReport {
        let mut report = ProbeReport {
            vendor_id: node.vendor_id,
            product_id: node.product_id,
            name: node.name.clone(),
            path: node.path.display().to_string(),
            open_error: None,
            readings: Vec::new(),
        };
        match Hidraw::open(&node.path) {
            Ok(hid) => report.readings = probe_reads(&hid),
            Err(e) => report.open_error = Some(e.to_string()),
        }
        report
    }

    /// The same reads, on an already open headset.
    pub fn probe(&self) -> Vec<Reading> {
        probe_reads(self.hid.as_ref())
    }
}

fn probe_reads(hid: &dyn Transport) -> Vec<Reading> {
    vec![
        byte_read(hid, "link", feature::CONNECTION, |b| b <= 1, |b| format!("{b:02x}")),
        byte_read(hid, "battery", feature::BATTERY, |b| b <= 100, |b| format!("{b:02x} ({b}%)")),
        byte_read(hid, "noise ctl", feature::ANC_MIRROR, |b| Anc::from_byte(b).is_some(), |b| format!("{b:02x}")),
        byte_read(hid, "sidetone", feature::SIDETONE_MIRROR, |b| Sidetone::from_byte(b).is_some(), |b| format!("{b:02x}")),
        byte_read(hid, "mic", feature::MIC_ACTIVE, |b| b <= 1, |b| format!("{b:02x}")),
        byte_read(hid, "dial", feature::DIAL, |b| b <= 16, |b| format!("{b:02x} ({b}/16)")),
        byte_read(hid, "lights", feature::LIGHTS_MIRROR, |b| b <= 1, |b| format!("{b:02x}")),
        bytes_read(hid, "serial", feature::SERIAL, 16, printable),
        // Three bytes per version: the 810 reports four (headset and dongle
        // parts), a wired Quantum 400 just one.
        bytes_read(hid, "firmware", feature::FIRMWARE, 12, |b| !b.is_empty() && b.len() % 3 == 0),
        bytes_read(hid, "host name", feature::DEVICE_NAME, 63, |b| b.first().is_some_and(|n| *n as usize <= 61)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_headset_probes_clean() {
        let h = Headset::open_simulated();
        let reads = h.probe();
        assert_eq!(reads.len(), 10);
        // The simulator doesn't model every report, but what it answers is plausible.
        assert!(reads.iter().filter(|r| r.answered()).all(|r| r.plausible() || r.report == feature::DEVICE_NAME));
        assert!(reads.iter().any(|r| r.report == feature::CONNECTION && r.plausible()));
    }

    #[test]
    fn the_report_reads_as_text_for_an_issue() {
        let report = ProbeReport {
            vendor_id: 0x0ecb,
            product_id: 0x203e,
            name: "Test".into(),
            path: "/dev/hidraw9".into(),
            open_error: None,
            readings: vec![
                Reading {
                    name: "battery",
                    report: feature::BATTERY,
                    outcome: Outcome::Answered { bytes: vec![75], plausible: true, shown: "4b (75%)".into() },
                },
                Reading { name: "dial", report: feature::DIAL, outcome: Outcome::Failed("did not answer".into()) },
            ],
        };
        let text = report.to_string();
        assert!(text.contains("0ecb:203e"));
        assert!(text.contains("Model:    JBL Quantum 800 (wireless flagship, headset)"), "{text}");
        assert!(text.contains("Verified: no"));
        assert!(text.contains("nothing was written"));
        assert!(text.contains("1 of 2 reads answered, 1 plausible"));
        assert_eq!(report.id(), "0ecb:203e");
    }

    #[test]
    fn a_node_that_cannot_be_opened_says_why() {
        let node = HidrawNode {
            path: "/dev/does-not-exist".into(),
            vendor_id: 0x0ecb,
            product_id: 0x203e,
            name: "x".into(),
        };
        let report = Headset::probe_node(&node);
        assert!(report.open_error.is_some());
        assert!(report.readings.is_empty());
        assert!(report.to_string().contains("Could not open"));
    }
}

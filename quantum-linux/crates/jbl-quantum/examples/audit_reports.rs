//! Audit which feature reports actually answer GET_REPORT.
//!
//! A report that does not answer leaves the previously fetched buffer in place,
//! so the only reliable test is whether the returned report-ID byte matches
//! what was asked for. Between each probe this reads a known-good sentinel
//! report, so a stale answer is always visibly the sentinel rather than
//! coincidentally the right value.

const SENTINEL: u8 = 0x49; // battery — always answers, and has a distinctive value

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node = jbl_quantum::Headset::discover()?
        .into_iter()
        .next()
        .ok_or("no dongle")?;
    let headset = jbl_quantum::Headset::open(&node)?;

    // Every feature report the research recorded, with its declared length.
    let reports: &[(u8, usize)] = &[
        (0x45, 1), (0x46, 1), (0x47, 1), (0x49, 1), (0x4A, 1), (0x4B, 1),
        (0x4C, 3), (0x4D, 7), (0x50, 2), (0x51, 12), (0x52, 2), (0x53, 1),
        (0x59, 1), (0x5A, 16), (0x5B, 63), (0x5C, 1), (0x5D, 1), (0x60, 1),
        (0x61, 16), (0x62, 1), (0x63, 1), (0x64, 1), (0x65, 1), (0x66, 1),
        (0x67, 1), (0x68, 1), (0x69, 1), (0x71, 63), (0x75, 1),
    ];

    let mut answered = Vec::new();
    let mut silent = Vec::new();

    for &(report, len) in reports {
        // Prime the buffer with the sentinel so a non-answering report is
        // unmistakable.
        let _ = headset.raw_feature_debug(SENTINEL, 1);
        let (_, buf) = headset.raw_feature_debug(report, len)?;
        let returned = buf[0];
        if returned == report {
            answered.push(report);
            println!("  0x{report:02X}  answers   {:02x?}", &buf[1..]);
        } else {
            silent.push(report);
            println!("  0x{report:02X}  SILENT    (returned 0x{returned:02X} instead)");
        }
    }

    println!("\nanswered ({}): {}", answered.len(),
             answered.iter().map(|r| format!("0x{r:02X}")).collect::<Vec<_>>().join(" "));
    println!("silent   ({}): {}", silent.len(),
             silent.iter().map(|r| format!("0x{r:02X}")).collect::<Vec<_>>().join(" "));
    Ok(())
}


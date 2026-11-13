//! Raw hidraw access: device discovery and the two ioctls this device needs.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const IOC_NRSHIFT: u64 = 0;
const IOC_TYPESHIFT: u64 = 8;
const IOC_SIZESHIFT: u64 = 16;
const IOC_DIRSHIFT: u64 = 30;

const fn ioc(dir: u64, ty: u8, nr: u64, size: usize) -> u64 {
    (dir << IOC_DIRSHIFT)
        | ((ty as u64) << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size as u64) << IOC_SIZESHIFT)
}

const fn hidioc_set_feature(len: usize) -> u64 {
    ioc(IOC_WRITE | IOC_READ, b'H', 0x06, len)
}

const fn hidioc_get_feature(len: usize) -> u64 {
    ioc(IOC_WRITE | IOC_READ, b'H', 0x07, len)
}

/// `HID_MAX_DESCRIPTOR_SIZE` in linux/hid.h.
const MAX_DESCRIPTOR: usize = 4096;

/// `struct hidraw_report_descriptor` in linux/hidraw.h.
#[repr(C)]
struct ReportDescriptor {
    size: u32,
    value: [u8; MAX_DESCRIPTOR],
}

const fn hidioc_get_rdesc_size() -> u64 {
    ioc(IOC_READ, b'H', 0x01, std::mem::size_of::<libc::c_int>())
}

const fn hidioc_get_rdesc() -> u64 {
    ioc(IOC_READ, b'H', 0x02, std::mem::size_of::<ReportDescriptor>())
}

/// The report IDs a HID report descriptor declares Feature items for.
pub(crate) fn feature_ids(descriptor: &[u8]) -> BTreeSet<u8> {
    let mut ids = BTreeSet::new();
    let (mut id, mut pushed) = (0u8, Vec::new());
    let mut i = 0;
    while i < descriptor.len() {
        let prefix = descriptor[i];
        if prefix == 0xFE {
            // A long item; its data size is in the next byte.
            i += 3 + descriptor.get(i + 1).copied().unwrap_or(0) as usize;
            continue;
        }
        let size = [0, 1, 2, 4][(prefix & 3) as usize];
        let data = descriptor.get(i + 1..i + 1 + size).unwrap_or(&[]);
        let value = data.iter().rev().fold(0u32, |v, &b| v << 8 | u32::from(b));
        match prefix & 0xFC {
            0x84 => id = value as u8,                // Report ID
            0xA4 => pushed.push(id),                 // Push
            0xB4 => id = pushed.pop().unwrap_or(id), // Pop
            0xB0 => drop(ids.insert(id)),            // Feature
            _ => {}
        }
        i += 1 + size;
    }
    ids
}

/// A hidraw node belonging to a matching device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidrawNode {
    pub path: PathBuf,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
}

/// Find every hidraw node matching `vid:pid`.
///
/// The node number is not stable — it depends on enumeration order and which
/// minors happen to be free — so callers must always discover rather than
/// hardcode a path.
pub fn discover(vid: u16, pid: u16) -> io::Result<Vec<HidrawNode>> {
    scan(|node_vid, node_pid| node_vid == vid && node_pid == pid)
}

/// Every hidraw node whose vendor is `vid`, whatever the product.
pub fn discover_vendor(vid: u16) -> io::Result<Vec<HidrawNode>> {
    scan(|node_vid, _| node_vid == vid)
}

fn scan(wanted: impl Fn(u16, u16) -> bool) -> io::Result<Vec<HidrawNode>> {
    let base = Path::new("/sys/class/hidraw");
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut found = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(base)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let uevent = entry.path().join("device/uevent");
        let Ok(text) = fs::read_to_string(&uevent) else {
            continue;
        };

        let Some((node_vid, node_pid)) = parse_hid_id(&text) else {
            continue;
        };
        if !wanted(node_vid, node_pid) {
            continue;
        }

        found.push(HidrawNode {
            path: Path::new("/dev").join(entry.file_name()),
            vendor_id: node_vid,
            product_id: node_pid,
            name: parse_field(&text, "HID_NAME").unwrap_or_default(),
        });
    }
    Ok(found)
}

/// Parse `HID_ID=0003:00000ECB:00002069` into `(0x0ECB, 0x2069)`.
fn parse_hid_id(uevent: &str) -> Option<(u16, u16)> {
    let value = parse_field(uevent, "HID_ID")?;
    let mut parts = value.split(':');
    let _bus = parts.next()?;
    let vid = u32::from_str_radix(parts.next()?, 16).ok()?;
    let pid = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some((vid as u16, pid as u16))
}

fn parse_field(uevent: &str, key: &str) -> Option<String> {
    uevent
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('=').map(str::to_owned))
}

/// An open hidraw file descriptor.
#[derive(Debug)]
pub struct Hidraw {
    fd: OwnedFd,
    /// The feature reports the device's report descriptor declares, or
    /// `None` when that couldn't be read, in which case nothing is refused.
    features: Option<BTreeSet<u8>>,
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        let fd: OwnedFd = file.into();
        // A descriptor with no report IDs puts everything under ID 0, which
        // this protocol never uses; don't gate on that.
        let features = read_descriptor(&fd).ok().map(|d| feature_ids(&d)).filter(|ids| !ids.contains(&0));
        Ok(Self { fd, features })
    }

    /// Refuses a report the device doesn't declare, before anything reaches
    /// it. Asking a Quantum 400 for reports it doesn't have (the wireless
    /// link, battery and host name) is one of the two things QuantumEngine
    /// did that JBL's app and a plain script don't, while its lights stopped
    /// following commands until a replug (issue #4).
    fn declared(&self, report_id: u8) -> io::Result<()> {
        match &self.features {
            Some(ids) if !ids.contains(&report_id) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("report 0x{report_id:02X} isn't one this device has; its report descriptor doesn't list it"),
            )),
            _ => Ok(()),
        }
    }

    /// `GET_REPORT(Feature)`. Returns the payload with the leading report-ID
    /// byte stripped.
    ///
    /// Verifies that the device answered with the report we asked for. Some
    /// report IDs on this device are write-only and simply do not respond; the
    /// kernel then leaves the previously fetched report in the buffer, so an
    /// unchecked read silently returns another report's value. That is a
    /// genuinely dangerous failure mode — it looks like valid data — so it is
    /// an error here rather than something callers must remember.
    pub fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let (n, buf) = self.get_feature_debug(report_id, len)?;
        let n = n as usize;
        if n < 1 || buf[0] != report_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "report 0x{report_id:02X} did not answer (device returned report 0x{:02X}); \
                     it is probably write-only",
                    buf.first().copied().unwrap_or(0)
                ),
            ));
        }
        Ok(if n > 1 { buf[1..n].to_vec() } else { Vec::new() })
    }

    /// Like [`Hidraw::get_feature`] but returns the ioctl result and the whole
    /// buffer, for diagnostics.
    pub fn get_feature_debug(&self, report_id: u8, len: usize) -> io::Result<(i32, Vec<u8>)> {
        self.declared(report_id)?;
        let mut buf = vec![0u8; len + 1];
        buf[0] = report_id;
        let n = unsafe {
            libc::ioctl(
                self.fd.as_raw_fd(),
                hidioc_get_feature(buf.len()),
                buf.as_mut_ptr(),
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((n as i32, buf))
    }

    /// `SET_REPORT(Feature)`. `payload` must begin with the report ID.
    ///
    /// This device has no interrupt OUT endpoint, so `write()` is never the
    /// right call — the vendor software uses control-transfer SET_REPORT and so
    /// do we.
    pub fn set_feature(&self, payload: &[u8]) -> io::Result<()> {
        if let Some(&report_id) = payload.first() {
            self.declared(report_id)?;
        }
        let mut buf = payload.to_vec();
        let n = unsafe {
            libc::ioctl(
                self.fd.as_raw_fd(),
                hidioc_set_feature(buf.len()),
                buf.as_mut_ptr(),
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Read one input report, waiting at most `timeout`. `Ok(None)` on timeout.
    pub fn read_report_timeout(&self, timeout: std::time::Duration) -> io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "dongle was removed"));
        }
        self.read_report().map(Some)
    }

    /// Read one pending input report, blocking until one arrives.
    pub fn read_report(&self) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 64];
        let n = unsafe {
            libc::read(
                self.fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(n as usize);
        Ok(buf)
    }
}

/// The device's HID report descriptor, as the kernel parsed it.
fn read_descriptor(fd: &OwnedFd) -> io::Result<Vec<u8>> {
    let mut size: libc::c_int = 0;
    if unsafe { libc::ioctl(fd.as_raw_fd(), hidioc_get_rdesc_size(), &mut size) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let size = (size.max(0) as usize).min(MAX_DESCRIPTOR);
    let mut desc = Box::new(ReportDescriptor { size: size as u32, value: [0; MAX_DESCRIPTOR] });
    if unsafe { libc::ioctl(fd.as_raw_fd(), hidioc_get_rdesc(), &mut *desc) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(desc.value[..size].to_vec())
}

/// Bytes in and out of the device: the real hidraw ioctls implemented above,
/// or — behind `QUANTUMENGINE_SIMULATE` — a simulated stand-in used for
/// development and tests that do not have the headset plugged in (see
/// `crate::sim`). [`crate::Headset`] only ever calls through this trait, so
/// nothing above this layer can tell which one it is talking to.
pub(crate) trait Transport: Send + Sync {
    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>>;
    fn get_feature_debug(&self, report_id: u8, len: usize) -> io::Result<(i32, Vec<u8>)>;
    fn set_feature(&self, payload: &[u8]) -> io::Result<()>;
    fn read_report(&self) -> io::Result<Vec<u8>>;
    fn read_report_timeout(&self, timeout: std::time::Duration) -> io::Result<Option<Vec<u8>>>;
}

impl Transport for Hidraw {
    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        Hidraw::get_feature(self, report_id, len)
    }
    fn get_feature_debug(&self, report_id: u8, len: usize) -> io::Result<(i32, Vec<u8>)> {
        Hidraw::get_feature_debug(self, report_id, len)
    }
    fn set_feature(&self, payload: &[u8]) -> io::Result<()> {
        Hidraw::set_feature(self, payload)
    }
    fn read_report(&self) -> io::Result<Vec<u8>> {
        Hidraw::read_report(self)
    }
    fn read_report_timeout(&self, timeout: std::time::Duration) -> io::Result<Option<Vec<u8>>> {
        Hidraw::read_report_timeout(self, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hid_id() {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00000ECB:00002069\nHID_NAME=JBL Quantum810 Wireless\n";
        assert_eq!(parse_hid_id(uevent), Some((0x0ECB, 0x2069)));
        assert_eq!(
            parse_field(uevent, "HID_NAME").as_deref(),
            Some("JBL Quantum810 Wireless")
        );
    }

    #[test]
    fn rejects_malformed_hid_id() {
        assert_eq!(parse_hid_id("HID_ID=nonsense\n"), None);
        assert_eq!(parse_hid_id("no fields here\n"), None);
    }

    #[test]
    fn ioctl_encoding_matches_linux_hid_h() {
        // Cross-checked against the values the working Python probe used.
        assert_eq!(hidioc_set_feature(2), 0xC002_4806);
        assert_eq!(hidioc_get_feature(2), 0xC002_4807);
        // _IOR('H', 0x01, int) and _IOR('H', 0x02, struct hidraw_report_descriptor).
        assert_eq!(hidioc_get_rdesc_size(), 0x8004_4801);
        assert_eq!(hidioc_get_rdesc(), 0x9004_4802);
    }

    /// Every report the library reads or writes, by number.
    fn used_reports() -> Vec<u8> {
        use crate::protocol::feature::*;
        vec![
            ANC_MIRROR, ANC, BATTERY, LIGHTS_MIRROR, LIGHTS, LIGHT_CONFIG, LIGHT_COLOUR, FIRMWARE, SIDETONE_MIRROR,
            SIDETONE, DEVICE_NAME, SERIAL, DIAL, MIC_ACTIVE, CONNECTION,
        ]
    }

    #[test]
    fn the_810_declares_every_report_the_library_uses() {
        // Its descriptor, read from a real 810's dongle.
        let ids = feature_ids(include_bytes!("testdata/quantum810.rdesc"));
        for report in used_reports() {
            assert!(ids.contains(&report), "0x{report:02X}");
        }
    }

    #[test]
    fn the_400_lacks_the_wireless_reports() {
        // Its descriptor, as posted on issue #4.
        let ids = feature_ids(include_bytes!("testdata/quantum400.rdesc"));
        for missing in [0x49, 0x5B, 0x68] {
            assert!(!ids.contains(&missing), "0x{missing:02X}");
        }
        for has in [0x4A, 0x4B, 0x4C, 0x4D, 0x51, 0x5C, 0x5D, 0x61, 0x62, 0x67] {
            assert!(ids.contains(&has), "0x{has:02X}");
        }
    }

    /// Run with `cargo test -- --ignored` with an 810 dongle plugged in. Only
    /// opens the node: the kernel answers the descriptor from its own copy,
    /// so nothing is sent to the dongle or the headset.
    #[test]
    #[ignore = "needs an 810 dongle plugged in"]
    fn a_real_dongle_declares_every_report_the_library_uses() {
        let node = discover(crate::VENDOR_ID, crate::PRODUCT_ID).unwrap().into_iter().next().expect("an 810 dongle");
        let h = Hidraw::open(&node.path).unwrap();
        let ids = h.features.as_ref().expect("the descriptor read");
        for report in used_reports() {
            assert!(ids.contains(&report), "0x{report:02X}");
        }
    }

    #[test]
    fn an_undeclared_report_never_reaches_the_device() {
        let fd: OwnedFd = fs::File::open("/dev/null").unwrap().into();
        let h = Hidraw { fd, features: Some(feature_ids(include_bytes!("testdata/quantum400.rdesc"))) };
        // /dev/null would fail the ioctl itself; this error comes first.
        let err = h.get_feature(0x68, 1).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert_eq!(h.set_feature(&[0x5B, 0]).unwrap_err().kind(), io::ErrorKind::Unsupported);
        assert_ne!(h.get_feature(0x4A, 1).unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
}

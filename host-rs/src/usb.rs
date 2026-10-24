use crate::{
    transport::{FrameReader, Transport, remaining},
    wire::Frame,
};
use rusb::{Context, Device, DeviceHandle, UsbContext};
use serde::{Deserialize, Serialize};
use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceId {
    pub bus: u8,
    pub address: u8,
    pub vid: u16,
    pub pid: u16,
    pub ports: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub id: DeviceId,
    pub name: String,
    pub suggested: bool,
}
#[derive(Debug)]
pub struct PermissionDenied(pub DeviceId);
impl std::fmt::Display for PermissionDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "USB access denied for the selected device")
    }
}
impl std::error::Error for PermissionDenied {}
fn access_error(error: rusb::Error, id: &DeviceId) -> Box<dyn std::error::Error + Send + Sync> {
    #[cfg(target_os = "linux")]
    if error == rusb::Error::Access {
        return Box::new(PermissionDenied(id.clone()));
    }
    #[cfg(windows)]
    if error == rusb::Error::NotSupported {
        return format!("WinUSB access unavailable for the selected interface: {error}").into();
    }
    let _ = id;
    error.into()
}
fn open_selected(device: &Device<Context>) -> crate::Result<DeviceHandle<Context>> {
    let id = identity(device)?;
    device.open().map_err(|error| access_error(error, &id))
}
pub fn accessory(vid: u16, pid: u16) -> bool {
    vid == 0x18d1 && [0x2d00, 0x2d01, 0x2d04, 0x2d05].contains(&pid)
}
fn suggested(vid: u16, pid: u16, mut interfaces: impl Iterator<Item = (u8, u8, u8)>) -> bool {
    accessory(vid, pid)
        || interfaces.any(|interface| matches!(interface, (0xff, 0x42, 1) | (6, 1, 1)))
}
fn identity(device: &Device<Context>) -> crate::Result<DeviceId> {
    let d = device.device_descriptor()?;
    Ok(DeviceId {
        bus: device.bus_number(),
        address: device.address(),
        vid: d.vendor_id(),
        pid: d.product_id(),
        ports: device.port_numbers()?,
    })
}
pub fn list() -> crate::Result<Vec<Candidate>> {
    let ctx = Context::new()?;
    let mut result = Vec::new();
    for device in ctx.devices()?.iter() {
        let Ok(d) = device.device_descriptor() else {
            continue;
        };
        if d.class_code() == 9 {
            continue;
        }
        let Ok(id) = identity(&device) else { continue };
        let interfaces: Vec<_> = (0..d.num_configurations())
            .filter_map(|n| device.config_descriptor(n).ok())
            .flat_map(|c| {
                c.interfaces()
                    .flat_map(|i| i.descriptors())
                    .map(|a| (a.class_code(), a.sub_class_code(), a.protocol_code()))
                    .collect::<Vec<_>>()
            })
            .collect();
        let suggested = suggested(id.vid, id.pid, interfaces.into_iter());
        let mut name = format!("USB {:04x}:{:04x}", id.vid, id.pid);
        #[cfg(target_os = "linux")]
        if !id.ports.is_empty() {
            let port = id
                .ports
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(".");
            if let Ok(product) =
                std::fs::read_to_string(format!("/sys/bus/usb/devices/{}-{}/product", id.bus, port))
            {
                let product = product.trim();
                if !product.is_empty() {
                    name = product.chars().take(100).collect();
                }
            }
        }
        name.push_str(&format!(
            " · {}-{}",
            id.bus,
            id.ports
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(".")
        ));
        result.push(Candidate {
            id,
            name,
            suggested,
        });
    }
    result.sort_by(|a, b| a.id.bus.cmp(&b.id.bus).then(a.id.ports.cmp(&b.id.ports)));
    Ok(result)
}
fn selected(ctx: &Context, id: &DeviceId, post_switch: bool) -> crate::Result<Device<Context>> {
    if id.ports.is_empty() {
        return Err("USB physical port is unavailable; reconnect the device".into());
    }
    for device in ctx.devices()?.iter() {
        if let Ok(actual) = identity(&device) {
            let same_port = actual.bus == id.bus && actual.ports == id.ports;
            if same_port && (actual == *id || (post_switch && accessory(actual.vid, actual.pid))) {
                return Ok(device);
            }
        }
    }
    Err("selected USB device disconnected".into())
}
#[cfg(target_os = "linux")]
pub fn access_denied(id: &DeviceId) -> crate::Result<bool> {
    let ctx = Context::new()?;
    match open_selected(&selected(&ctx, id, false)?) {
        Ok(_) => Ok(false),
        Err(error) if error.downcast_ref::<PermissionDenied>().is_some() => Ok(true),
        Err(error) => Err(error),
    }
}
pub fn switch(id: &DeviceId) -> crate::Result<()> {
    switch_selected(id, None)
}
pub fn switch_selected(id: &DeviceId, parent: Option<u32>) -> crate::Result<()> {
    let ctx = Context::new()?;
    let device = selected(&ctx, id, false)?;
    if accessory(id.vid, id.pid) {
        return Ok(());
    }
    let handle = open_selected(&device)?;
    #[cfg(target_os = "linux")]
    if let Some(parent) = parent {
        crate::platform::drop_to_parent(parent)?;
    }
    #[cfg(windows)]
    let _ = parent;
    let timeout = Duration::from_millis(1200);
    let mut protocol = [0; 2];
    if handle.read_control(0xc0, 51, 0, 0, &mut protocol, timeout)? != 2
        || u16::from_le_bytes(protocol) == 0
    {
        return Err("selected device does not support Android Open Accessory".into());
    }
    for (index, value) in [
        "Android Media Bridge",
        "AMB Host",
        "Low-latency Android to Linux media bridge",
        "0",
        "https://github.com/nekomario28/mobile-webcam-bridge",
        "amb-g0",
    ]
    .iter()
    .enumerate()
    {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        if handle.write_control(0x40, 52, 0, index as u16, &bytes, timeout)? != bytes.len() {
            return Err("short AOA identification transfer".into());
        }
    }
    handle.write_control(0x40, 53, 0, 0, &[], timeout)?;
    wait_accessory(&ctx, id)?;
    Ok(())
}
fn wait_accessory(ctx: &Context, id: &DeviceId) -> crate::Result<Device<Context>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(device) = selected(ctx, id, true) {
            let current = identity(&device)?;
            if accessory(current.vid, current.pid) {
                return Ok(device);
            }
        }
        if Instant::now() >= deadline {
            return Err("selected device did not enter accessory mode".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
// Called only after this session's privileged AOA switch has completed.
pub fn switched_identity(id: &DeviceId) -> crate::Result<DeviceId> {
    let actual = identity(&selected(&Context::new()?, id, true)?)?;
    if !accessory(actual.vid, actual.pid) {
        return Err("accessory mode ended before restart".into());
    }
    Ok(actual)
}
/// Room for one transfer, whatever the phone's stack decided to coalesce into it.
const TRANSFER_SCRATCH: usize = 1024 * 1024;

pub struct Usb {
    handle: DeviceHandle<Context>,
    interface: u8,
    input: u8,
    output: u8,
    reader: FrameReader,
    /// Bytes already read from the endpoint but not yet handed to the reader — how
    /// alignment keeps a header it found instead of discarding a live frame.
    pending: Vec<u8>,
    /// Where transfers are read before the reader sees them.
    ///
    /// A transfer is whatever the phone's USB stack hands over, which can be much larger
    /// than one frame; the reader's own buffer is not ours to size. Reading through this
    /// decouples the two — measured 2026-10-24 as libusb `Overflow` ("USB transfer larger
    /// than the read buffer") killing the connection a couple of seconds after
    /// authorization, which is a buffer-size problem reported as a link failure.
    scratch: Vec<u8>,
}
impl Usb {
    pub fn connect(id: &DeviceId) -> crate::Result<Self> {
        let ctx = Context::new()?;
        let device = selected(&ctx, id, false)?;
        let current = identity(&device)?;
        if !accessory(current.vid, current.pid) {
            switch(id)?;
        }
        let device = if accessory(current.vid, current.pid) {
            device
        } else {
            selected(&ctx, id, true)?
        };
        let active_id = identity(&device)?;
        let config = device.active_config_descriptor()?;
        let mut candidates = Vec::new();
        for interface in config.interfaces() {
            for alt in interface.descriptors() {
                let mut input = None;
                let mut output = None;
                for ep in alt
                    .endpoint_descriptors()
                    .filter(|e| e.transfer_type() == rusb::TransferType::Bulk)
                {
                    if ep.direction() == rusb::Direction::In {
                        input = Some((ep.address(), ep.max_packet_size() as usize & 0x7ff));
                    } else {
                        output = Some(ep.address());
                    }
                }
                if let (Some((input, packet)), Some(output)) = (input, output) {
                    candidates.push((
                        alt.class_code() == 0xff && alt.sub_class_code() == 0xff,
                        alt.interface_number(),
                        alt.setting_number(),
                        input,
                        output,
                        packet,
                    ));
                }
            }
        }
        candidates.sort_by_key(|c| !c.0);
        let (_, interface, alternate, input, output, packet_size) =
            *candidates.first().ok_or("AOA bulk endpoints unavailable")?;
        if packet_size == 0 {
            return Err("invalid USB packet size".into());
        }
        let handle = open_selected(&device)?;
        #[cfg(unix)]
        {
            let _ = handle.set_auto_detach_kernel_driver(true);
        }
        handle
            .claim_interface(interface)
            .map_err(|error| access_error(error, &active_id))?;
        if alternate != 0 {
            handle.set_alternate_setting(interface, alternate)?;
        }
        let mut opened = Self {
            handle,
            interface,
            input,
            output,
            reader: FrameReader::default(),
            pending: Vec::new(),
            scratch: vec![0u8; TRANSFER_SCRATCH],
        };
        // Start at a frame boundary. The endpoint may hold frames from a previous host
        // connection; AOA reports no disconnect, so nothing else resets it.
        if std::env::var_os("AMB_USB_TRACE").is_some() {
            eprintln!(
                "usb trace: input ep=0x{input:02x} output ep=0x{output:02x} transfer_scratch={TRANSFER_SCRATCH}"
            );
        }
        let _ = opened.align_stream();
        Ok(opened)
    }
}
impl Drop for Usb {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(self.interface);
    }
}
impl Usb {
    /// Start the stream at a frame boundary.
    ///
    /// The accessory endpoint has no session boundary: it holds whatever the phone
    /// wrote before this host opened it, and AOA reports no disconnect, so a new host
    /// process begins reading in the middle of somebody else's frame. Measured
    /// 2026-10-24 with `--usb-dump`: the phone writes one transfer for the 24-byte header
    /// and one for the payload, and a host that starts on a *payload* transfer sees
    /// `66 bytes` then `24 bytes` forever — every header is read as garbage and the reader
    /// can never complete a frame ("incomplete frame deadline expired").
    ///
    /// This reads transfers until one begins with `AMB1` (a header), then discards that
    /// frame's payload transfer, leaving the reader at the next header. Dropping a whole
    /// frame keeps an already-aligned stream aligned, so this is safe to call once after
    /// opening — which is where it is called from.
    pub fn align_stream(&mut self) -> std::io::Result<bool> {
        let mut buffer = [0u8; 1024];
        for _ in 0..64 {
            let count = match self.read_raw(Duration::from_millis(200), &mut buffer) {
                Ok(count) => count,
                Err(_) => return Ok(false),
            };
            if count == 0 {
                // Nothing queued: the next thing this host reads is a fresh header.
                return Ok(true);
            }
            if count >= 4 && &buffer[..4] == b"AMB1" {
                // This is a frame's beginning. Keep it — the reader consumes `pending`
                // first, so nothing is lost and no payload has to be guessed at.
                self.pending.extend_from_slice(&buffer[..count]);
                return Ok(true);
            }
            // Anything else is the tail of somebody else's frame: drop it and look again.
        }
        Ok(false)
    }

    /// Forget what the reader is holding and start at a frame boundary again.
    ///
    /// Needed between attempts, not only at connect: an attempt that ends mid-frame leaves
    /// the reader holding a partial header and whatever bytes followed it, and a reader in
    /// that state cannot resynchronise by itself (`io-budget`'s reader reports a bad header
    /// rather than scanning). Measured 2026-10-24: retrying after a host hello produced
    /// `invalid AMB1 header` and, on the next attempt, `does not support required per-PC
    /// authorization v2` — the same misalignment wearing two more costumes.
    pub fn reset_stream(&mut self) -> std::io::Result<bool> {
        self.pending.clear();
        self.reader = FrameReader::default();
        self.align_stream()
    }

    /// Read raw bytes, bypassing the frame reader. Diagnostic only: when a peer's frames
    /// do not parse, the bytes are the only fact that settles what it sent.
    pub fn read_raw(&self, timeout: Duration, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.handle.read_bulk(self.input, buffer, timeout) {
            Ok(count) => Ok(count),
            Err(rusb::Error::Timeout) => Ok(0),
            Err(error) => Err(std::io::Error::other(format!("{error:?}"))),
        }
    }
}

impl Transport for Usb {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        self.poll_receive(timeout)?.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "receive poll expired").into()
        })
    }
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        let handle = &self.handle;
        let input = self.input;
        let pending = &mut self.pending;
        let scratch = &mut self.scratch;
        self.reader
            .poll(timeout, |bytes, left| {
                // Bytes already read (alignment, or the tail of a transfer the reader's
                // buffer was too small for) come first: dropping them would lose a live
                // frame or desynchronise the stream.
                if pending.is_empty() {
                    match handle.read_bulk(input, scratch, left) {
                        Ok(0) | Err(rusb::Error::Interrupted) => {
                            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                        }
                        Ok(count) => pending.extend_from_slice(&scratch[..count]),
                        Err(rusb::Error::Timeout) => {
                            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
                        }
                        Err(error) => return Err(std::io::Error::other(format!("{error:?}"))),
                    }
                }
                let count = pending.len().min(bytes.len());
                bytes[..count].copy_from_slice(&pending[..count]);
                pending.drain(..count);
                Ok(count)
            })
            .map_err(Into::into)
    }
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        frame.validate()?;
        // Android UsbAccessory consumes a whole transfer per read: never combine these.
        for bytes in [frame.header.encode()?.as_slice(), frame.payload.as_slice()] {
            let mut offset = 0;
            let deadline = Instant::now() + timeout;
            while offset < bytes.len() {
                let count =
                    self.handle
                        .write_bulk(self.output, &bytes[offset..], remaining(deadline)?)?;
                if count == 0 {
                    return Err("USB write made no progress".into());
                }
                offset += count;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_transfer_phone_without_debugging_is_suggested() {
        // Standard still-image/MTP interface, with no ADB interface.
        assert!(suggested(0x0fce, 0x020d, [(6, 1, 1)].into_iter()));
        assert!(suggested(0x1234, 0x5678, [(6, 1, 1)].into_iter()));
        assert!(!suggested(0x1234, 0x5678, [(3, 1, 1)].into_iter()));
    }
    #[test]
    fn aoa_audio_only_is_not_accessory() {
        for pid in [0x2d00, 0x2d01, 0x2d04, 0x2d05] {
            assert!(accessory(0x18d1, pid));
        }
        for pid in [0x2d02, 0x2d03] {
            assert!(!accessory(0x18d1, pid));
        }
        assert!(!accessory(0x0fce, 0x2d00));
    }
}

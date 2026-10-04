use crate::{
    transport::Transport,
    wire::{Frame, HEADER_SIZE, Header},
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
    if matches!(error, rusb::Error::Access | rusb::Error::NotSupported) {
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
        let suggested = accessory(id.vid, id.pid)
            || (0..d.num_configurations())
                .filter_map(|n| device.config_descriptor(n).ok())
                .any(|c| {
                    c.interfaces().flat_map(|i| i.descriptors()).any(|a| {
                        (a.class_code(), a.sub_class_code(), a.protocol_code()) == (0xff, 0x42, 1)
                    })
                });
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
pub struct Usb {
    handle: DeviceHandle<Context>,
    interface: u8,
    input: u8,
    output: u8,
    packet_size: usize,
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
        Ok(Self {
            handle,
            interface,
            input,
            output,
            packet_size,
        })
    }
}
impl Drop for Usb {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(self.interface);
    }
}
fn remaining(deadline: Instant) -> crate::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .map(|d| d.max(Duration::from_millis(1)))
        .ok_or_else(|| "USB transfer timed out".into())
}
impl Transport for Usb {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        let mut bytes = [0; 1024];
        let deadline = Instant::now() + timeout;
        let header = loop {
            let size = self.handle.read_bulk(
                self.input,
                &mut bytes[..self.packet_size.clamp(HEADER_SIZE, 1024)],
                remaining(deadline)?,
            )?;
            if size == HEADER_SIZE
                && let Ok(header) = Header::decode(&bytes[..size])
            {
                break header;
            }
        };
        let mut payload = vec![0; header.payload_len as usize];
        let mut offset = 0;
        let mut deadline = Instant::now() + timeout;
        while offset < payload.len() {
            let count =
                self.handle
                    .read_bulk(self.input, &mut payload[offset..], remaining(deadline)?)?;
            if count > 0 {
                offset += count;
                deadline = Instant::now() + timeout;
            }
        }
        Ok(Frame { header, payload })
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

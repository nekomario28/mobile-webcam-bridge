use crate::platform::windows::{Handle, wide};
use std::{
    mem, ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Memory::*, Registry::*, Threading::*},
};
const MAX_SIZE: usize = 3840 * 2160 * 4 * 2;
#[repr(C)]
struct Header {
    max_size: u32,
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    resize: i32,
    mirror: i32,
    timeout: i32,
}
struct Shared {
    mutex: Handle,
    want: Handle,
    sent: Handle,
    _file: Handle,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    capacity: usize,
    last_demand: Instant,
}
impl Drop for Shared {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view);
        }
    }
}
pub struct Camera {
    _producer: Handle,
    want: Option<Handle>,
    shared: Option<Shared>,
    pub stride: usize,
    pub size: usize,
}
fn lock(mutex: HANDLE) -> crate::Result<()> {
    let result = unsafe { WaitForSingleObject(mutex, 2000) };
    if result == WAIT_OBJECT_0 {
        return Ok(());
    }
    if result == WAIT_ABANDONED {
        unsafe {
            ReleaseMutex(mutex);
        }
    }
    Err("Unity Capture mutex unavailable".into())
}
struct Unlock(HANDLE);
impl Drop for Unlock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
        }
    }
}
impl Camera {
    pub fn open(name: &str, width: u32, height: u32, _fps: u32) -> crate::Result<Self> {
        if width > 3840 || height > 2160 {
            return Err("Unity Capture supports at most 3840x2160".into());
        }
        let mut bytes = [0u16; 256];
        let mut size = mem::size_of_val(&bytes) as u32;
        let code = unsafe {
            RegGetValueW(
                HKEY_CLASSES_ROOT,
                wide("CLSID\\{5C2CD55C-92AD-4999-8666-912BD3E70010}").as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                bytes.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return Err("Unity Capture is not installed; run Mobile Webcam Setup.exe".into());
        }
        let installed = String::from_utf16(&bytes[..size as usize / 2 - 1])?;
        if name != installed {
            return Err(format!("camera name must be {installed}").into());
        }
        let raw = unsafe {
            CreateMutexW(
                ptr::null(),
                0,
                wide("Local\\MobileWebcam_UnityCaptureProducer").as_ptr(),
            )
        };
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let producer = Handle::checked(raw)?;
        if existed {
            return Err("another Mobile Webcam producer is using the camera".into());
        }
        Ok(Self {
            _producer: producer,
            want: None,
            shared: None,
            stride: width as usize * 4,
            size: width as usize * height as usize * 4,
        })
    }
    pub fn ready(&mut self) -> crate::Result<bool> {
        if let Some(shared) = &mut self.shared {
            match unsafe { WaitForSingleObject(shared.want.0, 0) } {
                WAIT_OBJECT_0 => shared.last_demand = Instant::now(),
                WAIT_TIMEOUT => {}
                _ => return Err(std::io::Error::last_os_error().into()),
            }
            return Ok(shared.last_demand.elapsed() <= Duration::from_secs(1));
        }
        let raw = unsafe {
            OpenMutexW(
                SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE,
                0,
                wide("UnityCapture_Mutx").as_ptr(),
            )
        };
        if raw.is_null() {
            return Ok(false);
        }
        let mutex = Handle::checked(raw)?;
        lock(mutex.0)?;
        let unlock = Unlock(mutex.0);
        // The pinned consumer opens Want before it creates Sent. Keep this
        // producer event alive while its alternating initialization finishes.
        if self.want.is_none() {
            self.want = Some(Handle::checked(unsafe {
                CreateEventW(ptr::null(), 0, 0, wide("UnityCapture_Want").as_ptr())
            })?);
        }
        let raw = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                0,
                wide("UnityCapture_Sent").as_ptr(),
            )
        };
        if raw.is_null() {
            return Ok(false);
        }
        let sent = Handle::checked(raw)?;
        let raw =
            unsafe { OpenFileMappingW(FILE_MAP_WRITE, 0, wide("UnityCapture_Data").as_ptr()) };
        if raw.is_null() {
            return Ok(false);
        }
        let file = Handle::checked(raw)?;
        let view = unsafe { MapViewOfFile(file.0, FILE_MAP_WRITE, 0, 0, 0) };
        if view.Value.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQuery(view.Value, &mut info, mem::size_of_val(&info)) } == 0
            || info.RegionSize < mem::size_of::<Header>()
        {
            unsafe {
                UnmapViewOfFile(view);
            }
            return Err("invalid Unity Capture mapping".into());
        }
        let capacity = unsafe { (*(view.Value as *const Header)).max_size as usize };
        if capacity > MAX_SIZE
            || capacity > info.RegionSize - mem::size_of::<Header>()
            || capacity < self.size
        {
            unsafe {
                UnmapViewOfFile(view);
            }
            return Err("Unity Capture shared frame capacity is invalid".into());
        }
        drop(unlock);
        self.shared = Some(Shared {
            mutex,
            want: self.want.take().unwrap(),
            sent,
            _file: file,
            view,
            capacity,
            // Bootstrap one frame: the pinned receiver signals Want only
            // after the shared header has a nonzero width.
            last_demand: Instant::now(),
        });
        Ok(true)
    }
    pub fn submit(&mut self, bytes: &[u8], width: u32, height: u32) -> crate::Result<()> {
        let shared = self
            .shared
            .as_ref()
            .ok_or("Unity Capture consumer is unavailable")?;
        if bytes.len() > shared.capacity {
            return Err("Unity Capture frame exceeds capacity".into());
        }
        lock(shared.mutex.0)?;
        let unlock = Unlock(shared.mutex.0);
        unsafe {
            let header = &mut *(shared.view.Value as *mut Header);
            header.width = width as i32;
            header.height = height as i32;
            header.stride = width as i32;
            header.format = 0;
            header.resize = 1;
            header.mirror = 0;
            header.timeout = 1000;
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (shared.view.Value as *mut u8).add(mem::size_of::<Header>()),
                bytes.len(),
            );
        }
        drop(unlock);
        if unsafe { SetEvent(shared.sent.0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

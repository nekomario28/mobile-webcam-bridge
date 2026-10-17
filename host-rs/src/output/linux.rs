use std::{
    fs::{File, OpenOptions},
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
};
#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code
)]
mod abi {
    include!(concat!(env!("OUT_DIR"), "/v4l2.rs"));
}
use abi::*;
pub struct Camera {
    file: File,
    pub stride: usize,
    pub size: usize,
    buffers: Vec<MappedBuffer>,
    started: std::time::Instant,
}
struct MappedBuffer {
    ptr: *mut u8,
    length: usize,
}
impl Drop for MappedBuffer {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.length) };
    }
}
// SAFETY: the mapping is process-wide and every access goes through `submit`.
unsafe impl Send for Camera {}
impl Drop for Camera {
    fn drop(&mut self) {
        let mut kind: libc::c_uint = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        let _ = ioctl(&self.file, AMB_STREAMOFF, &mut kind);
    }
}
fn ioctl<T>(file: &File, request: u64, value: &mut T) -> std::io::Result<()> {
    loop {
        // The generated UAPI type and request code bind this pointer to the kernel ABI.
        if unsafe { libc::ioctl(file.as_raw_fd(), request as libc::c_ulong, &mut *value) } >= 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}
impl Camera {
    pub fn open(path: &str, width: u32, height: u32, fps: u32) -> crate::Result<Self> {
        // V4L2 MMAP streaming requires a read+write descriptor. Non-blocking:
        // with no consumer attached the driver never returns a buffer, and a
        // blocking DQBUF would stall the pipeline instead of dropping the frame.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        let mut cap = v4l2_capability::default();
        ioctl(&file, AMB_QUERYCAP, &mut cap)?;
        let caps = if cap.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };
        if caps & V4L2_CAP_VIDEO_OUTPUT == 0 || caps & V4L2_CAP_STREAMING == 0 {
            return Err("camera does not support V4L2 mmap output streaming".into());
        }
        let mut format = v4l2_format {
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
            ..Default::default()
        };
        let pix = unsafe { &mut format.fmt.pix };
        pix.width = width;
        pix.height = height;
        pix.pixelformat = AMB_YUYV;
        pix.field = V4L2_FIELD_NONE;
        pix.colorspace = V4L2_COLORSPACE_REC709;
        pix.__bindgen_anon_1.ycbcr_enc = V4L2_YCBCR_ENC_709;
        pix.quantization = V4L2_QUANTIZATION_LIM_RANGE;
        pix.xfer_func = V4L2_XFER_FUNC_709;
        ioctl(&file, AMB_S_FMT, &mut format)?;
        let pix = unsafe { format.fmt.pix };
        if pix.width != width || pix.height != height || pix.pixelformat != AMB_YUYV {
            return Err("V4L2 rejected YUYV dimensions".into());
        }
        let stride = (pix.bytesperline as usize).max(width as usize * 2);
        let size = (pix.sizeimage as usize).max(stride * height as usize);
        if size > 256 * 1024 * 1024 {
            return Err("V4L2 output allocation exceeds limit".into());
        }
        let mut parameters = v4l2_streamparm {
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
            ..Default::default()
        };
        parameters.parm.output.timeperframe.numerator = 1;
        parameters.parm.output.timeperframe.denominator = fps;
        ioctl(&file, AMB_S_PARM, &mut parameters)?;
        let mut request: v4l2_requestbuffers = unsafe { std::mem::zeroed() };
        request.count = 2;
        request.type_ = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        request.memory = AMB_MEMORY_MMAP;
        ioctl(&file, AMB_REQBUFS, &mut request)?;
        if request.count < 1 {
            return Err("V4L2 refused output buffers".into());
        }
        let mut buffers = Vec::new();
        for index in 0..request.count {
            let mut buffer: v4l2_buffer = unsafe { std::mem::zeroed() };
            buffer.type_ = V4L2_BUF_TYPE_VIDEO_OUTPUT;
            buffer.memory = AMB_MEMORY_MMAP;
            buffer.index = index;
            ioctl(&file, AMB_QUERYBUF, &mut buffer)?;
            let length = buffer.length as usize;
            if length < size {
                return Err("V4L2 buffer is smaller than one frame".into());
            }
            // SAFETY: QUERYBUF populated the buffer; `offset` is the MMAP arm.
            let offset = unsafe { buffer.m.offset } as libc::off_t;
            // SAFETY: offset/length come from VIDIOC_QUERYBUF on this fd.
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    length,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    file.as_raw_fd(),
                    offset,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err("V4L2 output buffer mapping failed".into());
            }
            buffers.push(MappedBuffer {
                ptr: ptr as *mut u8,
                length,
            });
        }
        // Prime every buffer with real content: a consumer that dequeues a
        // zero-length buffer reports "contains 0 bytes, but N were expected"
        // (seen from ffplay), so an empty buffer is never queued.
        let started = std::time::Instant::now();
        for index in 0..request.count {
            let mapped = buffers
                .get(index as usize)
                .ok_or("V4L2 reported a buffer index it did not map")?;
            // SAFETY: the mapping is at least `size` bytes and owned here.
            unsafe { std::ptr::write_bytes(mapped.ptr, 0x80, size) };
            let mut buffer: v4l2_buffer = unsafe { std::mem::zeroed() };
            buffer.type_ = V4L2_BUF_TYPE_VIDEO_OUTPUT;
            buffer.memory = AMB_MEMORY_MMAP;
            buffer.index = index;
            buffer.bytesused = size as u32;
            buffer.timestamp.tv_sec = 0;
            buffer.timestamp.tv_usec = 0;
            buffer.flags = AMB_TIMESTAMP_COPY;
            ioctl(&file, AMB_QBUF, &mut buffer)?;
        }
        let mut kind: libc::c_uint = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        ioctl(&file, AMB_STREAMON, &mut kind)?;
        Ok(Self {
            file,
            stride,
            size,
            buffers,
            started,
        })
    }
    pub fn ready(&mut self) -> crate::Result<bool> {
        Ok(true)
    }
    pub fn submit(&mut self, bytes: &[u8], _width: u32, _height: u32) -> crate::Result<()> {
        if bytes.len() > self.size {
            return Err("frame is larger than the V4L2 output buffer".into());
        }
        let mut buffer: v4l2_buffer = unsafe { std::mem::zeroed() };
        buffer.type_ = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        buffer.memory = AMB_MEMORY_MMAP;
        match ioctl(&self.file, AMB_DQBUF, &mut buffer) {
            Ok(()) => {}
            // No consumer has released a buffer: drop this frame rather than
            // stalling the producer. A reader that attaches later starts
            // receiving from the next queued buffer.
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        let index = buffer.index as usize;
        let mapped = self
            .buffers
            .get(index)
            .ok_or("V4L2 returned an unknown output buffer index")?;
        // SAFETY: the mapping is at least one frame long and owned here.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.ptr, bytes.len()) };
        buffer.bytesused = bytes.len() as u32;
        // v4l2loopback only exposes a queued buffer to consumers when it carries
        // a timestamp: with a zero `timeval` the reader's stream is empty and its
        // `STREAMON` can even fail with EIO. Monotonic since open is enough, and
        // TIMESTAMP_COPY states that this producer supplied it.
        let elapsed = self.started.elapsed();
        buffer.timestamp.tv_sec = elapsed.as_secs() as _;
        buffer.timestamp.tv_usec = elapsed.subsec_micros() as _;
        buffer.flags = AMB_TIMESTAMP_COPY;
        ioctl(&self.file, AMB_QBUF, &mut buffer)?;
        Ok(())
    }
}

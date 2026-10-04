use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::fd::AsRawFd,
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
        let file = OpenOptions::new().write(true).open(path)?;
        let mut cap = v4l2_capability::default();
        ioctl(&file, AMB_QUERYCAP, &mut cap)?;
        let caps = if cap.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };
        if caps & V4L2_CAP_VIDEO_OUTPUT == 0 || caps & V4L2_CAP_READWRITE == 0 {
            return Err("camera does not support V4L2 write output".into());
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
        Ok(Self { file, stride, size })
    }
    pub fn ready(&mut self) -> crate::Result<bool> {
        Ok(true)
    }
    pub fn submit(&mut self, bytes: &[u8], _width: u32, _height: u32) -> crate::Result<()> {
        write_frame(&mut self.file, bytes).map_err(Into::into)
    }
}
fn write_frame(output: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    loop {
        match output.write(bytes) {
            Ok(n) if n == bytes.len() => return Ok(()),
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "partial V4L2 frame write",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Partial(usize);
    impl Write for Partial {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Ok(2)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn partial_frames_are_not_submitted_as_tails() {
        let mut camera = Partial(0);
        assert!(write_frame(&mut camera, &[0; 4]).is_err());
        assert_eq!(camera.0, 1);
    }
}

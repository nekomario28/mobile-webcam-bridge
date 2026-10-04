use super::ffmpeg::{Frame, abi as ffi, check};
use std::{
    ffi::CString,
    ptr::{self, NonNull},
};
struct Hardware {
    pixel: ffi::AVPixelFormat,
    allow_software: bool,
}
unsafe extern "C" fn choose(
    context: *mut ffi::AVCodecContext,
    formats: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    unsafe {
        let hardware = &*((*context).opaque as *const Hardware);
        let mut p = formats;
        while *p != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == hardware.pixel {
                return *p;
            }
            p = p.add(1);
        }
        if hardware.allow_software {
            *formats
        } else {
            ffi::AVPixelFormat::AV_PIX_FMT_NONE
        }
    }
}
pub struct Decoder {
    context: NonNull<ffi::AVCodecContext>,
    hardware: Box<Hardware>,
    packet: NonNull<ffi::AVPacket>,
    transferred: Frame,
    name: String,
    active_hardware: bool,
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            let mut p = self.context.as_ptr();
            ffi::avcodec_free_context(&mut p);
            let mut p = self.packet.as_ptr();
            ffi::av_packet_free(&mut p);
        }
    }
}
impl Decoder {
    pub fn open(requested: &str) -> crate::Result<Self> {
        let modes: Vec<&str> = match requested {
            "auto" => vec![
                if cfg!(windows) { "d3d11va" } else { "vaapi" },
                "cuda",
                "software",
            ],
            "off" | "software" => vec!["software"],
            mode => vec![mode],
        };
        let mut last = None;
        for mode in modes {
            match Self::attempt(mode, requested == "auto") {
                Ok(d) => return Ok(d),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| "decoder unavailable".into()))
    }
    fn attempt(mode: &str, allow_software: bool) -> crate::Result<Self> {
        let codec = unsafe { ffi::avcodec_find_decoder(ffi::AVCodecID::AV_CODEC_ID_H264) };
        if codec.is_null() {
            return Err("H.264 decoder unavailable".into());
        }
        let transferred = Frame::empty()?;
        let context = NonNull::new(unsafe { ffi::avcodec_alloc_context3(codec) })
            .ok_or("FFmpeg context allocation failed")?;
        let packet = match NonNull::new(unsafe { ffi::av_packet_alloc() }) {
            Some(p) => p,
            None => {
                unsafe {
                    let mut p = context.as_ptr();
                    ffi::avcodec_free_context(&mut p);
                }
                return Err("FFmpeg packet allocation failed".into());
            }
        };
        let mut result = Self {
            context,
            hardware: Box::new(Hardware {
                pixel: ffi::AVPixelFormat::AV_PIX_FMT_NONE,
                allow_software,
            }),
            packet,
            transferred,
            name: mode.into(),
            active_hardware: false,
        };
        unsafe {
            let raw = result.context.as_ptr();
            (*raw).max_pixels = 33_554_432;
            (*raw).pkt_timebase = ffi::AVRational {
                num: 1,
                den: 1_000_000,
            };
            if mode != "software" {
                let name = CString::new(mode)?;
                let kind = ffi::av_hwdevice_find_type_by_name(name.as_ptr());
                if kind == ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
                    return Err(format!("unsupported hardware decoder: {mode}").into());
                }
                let mut found = false;
                for index in 0.. {
                    let config = ffi::avcodec_get_hw_config(codec, index);
                    if config.is_null() {
                        break;
                    }
                    if (*config).device_type == kind
                        && (*config).methods & ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32
                            != 0
                    {
                        result.hardware.pixel = (*config).pix_fmt;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(format!("H.264 hardware configuration unavailable: {mode}").into());
                }
                let mut device = ptr::null_mut();
                check(ffi::av_hwdevice_ctx_create(
                    &mut device,
                    kind,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                ))?;
                (*raw).hw_device_ctx = device;
                (*raw).opaque = (&mut *result.hardware as *mut Hardware).cast();
                (*raw).get_format = Some(choose);
            }
            check(ffi::avcodec_open2(raw, codec, ptr::null_mut()))?;
        }
        Ok(result)
    }
    pub fn send(&mut self, bytes: &[u8], pts: u64, keyframe: bool) -> crate::Result<()> {
        unsafe {
            let p = self.packet.as_ptr();
            ffi::av_packet_unref(p);
            check(ffi::av_new_packet(p, i32::try_from(bytes.len())?))?;
            ptr::copy_nonoverlapping(bytes.as_ptr(), (*p).data, bytes.len());
            (*p).pts = pts as i64;
            (*p).dts = pts as i64;
            if keyframe {
                (*p).flags |= ffi::AV_PKT_FLAG_KEY;
            }
            check(ffi::avcodec_send_packet(self.context.as_ptr(), p))?;
        }
        Ok(())
    }
    pub fn receive(&mut self, frame: &mut Frame) -> crate::Result<bool> {
        let rc = unsafe { ffi::avcodec_receive_frame(self.context.as_ptr(), frame.ptr()) };
        if rc == ffi::AVERROR(11) || rc == ffi::AVERROR_EOF {
            return Ok(false);
        }
        check(rc)?;
        self.active_hardware = frame.format() == self.hardware.pixel as i32;
        if self.active_hardware {
            unsafe {
                ffi::av_frame_unref(self.transferred.ptr());
                check(ffi::av_hwframe_transfer_data(
                    self.transferred.ptr(),
                    frame.const_ptr(),
                    0,
                ))?;
                check(ffi::av_frame_copy_props(
                    self.transferred.ptr(),
                    frame.const_ptr(),
                ))?;
            }
            std::mem::swap(frame, &mut self.transferred);
        }
        Ok(true)
    }
    pub fn active(&self) -> &str {
        if self.active_hardware {
            &self.name
        } else {
            "software"
        }
    }
    pub fn flush(&mut self) {
        unsafe {
            ffi::avcodec_flush_buffers(self.context.as_ptr());
        }
    }
}

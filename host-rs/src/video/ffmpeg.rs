pub use ffmpeg_sys_next as abi;
#[cfg(ffmpeg_9)]
const BILINEAR: i32 = abi::SwsFlags::SWS_BILINEAR as i32;
#[cfg(not(ffmpeg_9))]
const BILINEAR: i32 = abi::SWS_BILINEAR;
use std::{
    ffi::CStr,
    ptr::{self, NonNull},
};
pub fn check(code: i32) -> crate::Result<()> {
    if code >= 0 {
        return Ok(());
    }
    let mut message = [0i8; 256];
    unsafe {
        abi::av_strerror(code, message.as_mut_ptr(), message.len());
    }
    Err(unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_string_lossy()
        .into_owned()
        .into())
}
pub struct Frame(NonNull<abi::AVFrame>);
impl Frame {
    pub fn empty() -> crate::Result<Self> {
        Ok(Self(
            NonNull::new(unsafe { abi::av_frame_alloc() })
                .ok_or("FFmpeg frame allocation failed")?,
        ))
    }
    pub fn new(format: abi::AVPixelFormat, width: u32, height: u32) -> crate::Result<Self> {
        let mut frame = Self::empty()?;
        unsafe {
            (*frame.ptr()).format = format as i32;
            (*frame.ptr()).width = width as i32;
            (*frame.ptr()).height = height as i32;
        }
        check(unsafe { abi::av_frame_get_buffer(frame.ptr(), 32) })?;
        Ok(frame)
    }
    pub fn ptr(&mut self) -> *mut abi::AVFrame {
        self.0.as_ptr()
    }
    pub fn const_ptr(&self) -> *const abi::AVFrame {
        self.0.as_ptr()
    }
    pub fn width(&self) -> u32 {
        unsafe { (*self.0.as_ptr()).width as u32 }
    }
    pub fn height(&self) -> u32 {
        unsafe { (*self.0.as_ptr()).height as u32 }
    }
    pub fn format(&self) -> i32 {
        unsafe { (*self.0.as_ptr()).format }
    }
    pub fn stride(&self) -> crate::Result<usize> {
        let stride = unsafe { (*self.0.as_ptr()).linesize[0] };
        if stride <= 0 {
            Err("invalid FFmpeg RGB/output stride".into())
        } else {
            Ok(stride as usize)
        }
    }
    fn plane(&self) -> crate::Result<(*mut u8, usize)> {
        unsafe {
            let frame = self.0.as_ptr();
            let size = self
                .stride()?
                .checked_mul(self.height() as usize)
                .ok_or("FFmpeg plane overflow")?;
            let buffer = abi::av_frame_get_plane_buffer(frame, 0);
            if buffer.is_null() || (*frame).data[0].is_null() {
                return Err("FFmpeg output plane unavailable".into());
            }
            let offset = ((*frame).data[0] as usize)
                .checked_sub((*buffer).data as usize)
                .ok_or("FFmpeg plane precedes buffer")?;
            if offset
                .checked_add(size)
                .is_none_or(|end| end > (*buffer).size)
            {
                return Err("FFmpeg plane exceeds buffer".into());
            }
            Ok(((*frame).data[0], size))
        }
    }
    pub fn data(&self) -> crate::Result<&[u8]> {
        let (p, n) = self.plane()?;
        Ok(unsafe { std::slice::from_raw_parts(p, n) })
    }
    pub fn data_mut(&mut self) -> crate::Result<&mut [u8]> {
        let (p, n) = self.plane()?;
        Ok(unsafe { std::slice::from_raw_parts_mut(p, n) })
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            let mut p = self.0.as_ptr();
            abi::av_frame_free(&mut p);
        }
    }
}
pub struct Scaler {
    ptr: NonNull<abi::SwsContext>,
    key: (i32, u32, u32, abi::AVPixelFormat, u32, u32),
}
impl Scaler {
    pub fn matches(
        &self,
        source: &Frame,
        format: abi::AVPixelFormat,
        width: u32,
        height: u32,
    ) -> bool {
        self.key
            == (
                source.format(),
                source.width(),
                source.height(),
                format,
                width,
                height,
            )
    }
    pub fn new(
        source: &Frame,
        format: abi::AVPixelFormat,
        width: u32,
        height: u32,
    ) -> crate::Result<Self> {
        // AVFrame stores an integer. Resolve it through FFmpeg's descriptor
        // inventory rather than constructing a possibly invalid Rust enum.
        let mut descriptor = unsafe { abi::av_pix_fmt_desc_next(ptr::null()) };
        let source_format = loop {
            if descriptor.is_null() {
                return Err("invalid decoded pixel format".into());
            }
            let format = unsafe { abi::av_pix_fmt_desc_get_id(descriptor) };
            if format as i32 == source.format() {
                break format;
            }
            descriptor = unsafe { abi::av_pix_fmt_desc_next(descriptor) };
        };
        let p = unsafe {
            abi::sws_getContext(
                source.width() as i32,
                source.height() as i32,
                source_format,
                width as i32,
                height as i32,
                format,
                BILINEAR,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        Ok(Self {
            ptr: NonNull::new(p).ok_or("FFmpeg scaling context unavailable")?,
            key: (
                source.format(),
                source.width(),
                source.height(),
                format,
                width,
                height,
            ),
        })
    }
    pub fn run(&mut self, source: &Frame, destination: &mut Frame) -> crate::Result<()> {
        let (_, width, height, format, out_width, out_height) = self.key;
        if source.width() != width
            || source.height() != height
            || source.format() != self.key.0
            || destination.format() != format as i32
            || destination.width() != out_width
            || destination.height() != out_height
        {
            return Err("FFmpeg scaler dimensions changed".into());
        }
        let rows = unsafe {
            abi::sws_scale(
                self.ptr.as_ptr(),
                (*source.const_ptr()).data.as_ptr().cast(),
                (*source.const_ptr()).linesize.as_ptr(),
                0,
                height as i32,
                (*destination.ptr()).data.as_ptr(),
                (*destination.ptr()).linesize.as_ptr(),
            )
        };
        if rows != out_height as i32 {
            return Err(format!("pixel conversion produced {rows} rows").into());
        }
        Ok(())
    }
}
impl Drop for Scaler {
    fn drop(&mut self) {
        unsafe {
            abi::sws_freeContext(self.ptr.as_ptr());
        }
    }
}

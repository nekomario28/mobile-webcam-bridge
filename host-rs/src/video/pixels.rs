use super::{
    Config, Transform,
    ffmpeg::{Frame, Scaler, abi::AVPixelFormat as Pixel},
    transform,
};
#[derive(Default)]
pub struct Pixels {
    direct: Option<Scaler>,
    to_rgb: Option<Scaler>,
    fitted: Option<Scaler>,
    converted: Option<Frame>,
    rgb: Option<Frame>,
    rotated: Option<Frame>,
    fit_frame: Option<Frame>,
    bytes: Vec<u8>,
}
fn scaler(
    slot: &mut Option<Scaler>,
    source: &Frame,
    format: Pixel,
    width: u32,
    height: u32,
) -> crate::Result<()> {
    if slot
        .as_ref()
        .is_none_or(|s| !s.matches(source, format, width, height))
    {
        *slot = Some(Scaler::new(source, format, width, height)?);
    }
    Ok(())
}
fn frame(
    slot: &mut Option<Frame>,
    format: Pixel,
    width: u32,
    height: u32,
) -> crate::Result<&mut Frame> {
    if slot
        .as_ref()
        .is_none_or(|f| f.format() != format as i32 || f.width() != width || f.height() != height)
    {
        *slot = Some(Frame::new(format, width, height)?);
    }
    Ok(slot.as_mut().unwrap())
}
impl Pixels {
    pub fn convert(
        &mut self,
        source: &Frame,
        c: Config,
        stride: usize,
        size: usize,
        t: Transform,
    ) -> crate::Result<&[u8]> {
        let rgba = cfg!(windows);
        let format = if rgba {
            Pixel::AV_PIX_FMT_RGBA
        } else {
            Pixel::AV_PIX_FMT_YUYV422
        };
        let bpp = if rgba { 4 } else { 2 };
        self.bytes.resize(size, 0);
        if t.rotation == 0 {
            scaler(&mut self.direct, source, format, c.width, c.height)?;
            let converted = frame(&mut self.converted, format, c.width, c.height)?;
            self.direct.as_mut().unwrap().run(source, converted)?;
            for y in 0..c.height as usize {
                let row = &converted.data()?[y * converted.stride()?..][..c.width as usize * bpp];
                self.bytes[y * stride..y * stride + row.len()].copy_from_slice(row);
            }
        } else {
            scaler(
                &mut self.to_rgb,
                source,
                Pixel::AV_PIX_FMT_RGB24,
                c.width,
                c.height,
            )?;
            let rgb = frame(&mut self.rgb, Pixel::AV_PIX_FMT_RGB24, c.width, c.height)?;
            self.to_rgb.as_mut().unwrap().run(source, rgb)?;
            let (w, h) = if t.rotation == 90 || t.rotation == 270 {
                (c.height, c.width)
            } else {
                (c.width, c.height)
            };
            let rotated = frame(&mut self.rotated, Pixel::AV_PIX_FMT_RGB24, w, h)?;
            let rotated_stride = rotated.stride()?;
            transform::rotate(
                rgb.data()?,
                c.width as usize,
                c.height as usize,
                rgb.stride()?,
                rotated.data_mut()?,
                rotated_stride,
                t.rotation,
            )?;
            let rect = transform::fit(w as usize, h as usize, c.width as usize, c.height as usize)?;
            scaler(
                &mut self.fitted,
                rotated,
                format,
                rect.width as u32,
                rect.height as u32,
            )?;
            let fitted = frame(
                &mut self.fit_frame,
                format,
                rect.width as u32,
                rect.height as u32,
            )?;
            self.fitted.as_mut().unwrap().run(rotated, fitted)?;
            if rgba {
                self.bytes.fill(0);
                for alpha in self.bytes.iter_mut().skip(3).step_by(4) {
                    *alpha = 255;
                }
            } else {
                self.bytes.fill(128);
                for y in 0..c.height as usize {
                    for x in (0..c.width as usize * 2).step_by(2) {
                        self.bytes[y * stride + x] = 16;
                    }
                }
            }
            for y in 0..rect.height {
                let row = &fitted.data()?[y * fitted.stride()?..][..rect.width * bpp];
                let offset = (y + rect.y) * stride + rect.x * bpp;
                self.bytes[offset..offset + row.len()].copy_from_slice(row);
            }
        }
        transform::flips(
            &mut self.bytes,
            c.width as usize,
            c.height as usize,
            stride,
            rgba,
            t.mirror,
            t.vertical ^ rgba,
        );
        Ok(&self.bytes)
    }
}

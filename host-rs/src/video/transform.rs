#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}
pub fn fit(
    width: usize,
    height: usize,
    out_width: usize,
    out_height: usize,
) -> crate::Result<Rect> {
    if width == 0 || height == 0 || out_width < 2 || !out_width.is_multiple_of(2) || out_height == 0
    {
        return Err("invalid fit dimensions".into());
    }
    let (w, h) = if width * out_height <= height * out_width {
        ((width * out_height / height) & !1, out_height)
    } else {
        (out_width, height * out_width / width)
    };
    if w < 2 || h == 0 {
        return Err("rotated image is too narrow".into());
    }
    Ok(Rect {
        x: ((out_width - w) / 2) & !1,
        y: (out_height - h) / 2,
        width: w,
        height: h,
    })
}
pub fn rotate(
    source: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    destination: &mut [u8],
    out_stride: usize,
    angle: u16,
) -> crate::Result<()> {
    let (ow, oh) = if angle == 90 || angle == 270 {
        (height, width)
    } else {
        (width, height)
    };
    if width == 0
        || height == 0
        || stride < width * 3
        || out_stride < ow * 3
        || source.len() / stride < height
        || destination.len() / out_stride < oh
    {
        return Err("invalid RGB buffer".into());
    }
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = match angle {
                0 => (x, y),
                90 => (height - y - 1, x),
                180 => (width - x - 1, height - y - 1),
                270 => (y, width - x - 1),
                _ => return Err("invalid rotation".into()),
            };
            destination[dy * out_stride + dx * 3..dy * out_stride + dx * 3 + 3]
                .copy_from_slice(&source[y * stride + x * 3..y * stride + x * 3 + 3]);
        }
    }
    Ok(())
}
pub fn flips(
    bytes: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    rgba: bool,
    horizontal: bool,
    vertical: bool,
) {
    let row_bytes = width * if rgba { 4 } else { 2 };
    for y in 0..height {
        let row = &mut bytes[y * stride..y * stride + row_bytes];
        if horizontal {
            if rgba {
                for x in 0..width / 2 {
                    for c in 0..4 {
                        row.swap(x * 4 + c, (width - x - 1) * 4 + c);
                    }
                }
            } else {
                for pair in row.as_chunks_mut::<4>().0 {
                    pair.swap(0, 2);
                }
                for p in 0..width / 4 {
                    for c in 0..4 {
                        row.swap(p * 4 + c, (width / 2 - p - 1) * 4 + c);
                    }
                }
            }
        }
    }
    if vertical {
        for y in 0..height / 2 {
            for x in 0..row_bytes {
                bytes.swap(y * stride + x, (height - y - 1) * stride + x);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_cpp_rotations_and_letterbox() {
        let source: Vec<_> = (1..=6).flat_map(|x| [x, x, x]).collect();
        for (angle, expected) in [
            (90, [4, 1, 5, 2, 6, 3]),
            (180, [6, 5, 4, 3, 2, 1]),
            (270, [3, 6, 2, 5, 1, 4]),
        ] {
            let mut output = [0; 18];
            rotate(
                &source,
                3,
                2,
                9,
                &mut output,
                if angle == 180 { 9 } else { 6 },
                angle,
            )
            .unwrap();
            assert_eq!(
                output
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| p[0])
                    .collect::<Vec<_>>(),
                expected
            );
        }
        assert_eq!(
            fit(720, 1280, 1280, 720).unwrap(),
            Rect {
                x: 438,
                y: 0,
                width: 404,
                height: 720
            }
        );
    }
    #[test]
    fn yuyv_flip_preserves_chroma_and_padding() {
        let mut row = [1, 10, 2, 20, 3, 30, 4, 40, 99, 99];
        flips(&mut row, 4, 1, 10, false, true, false);
        assert_eq!(row, [4, 30, 3, 40, 2, 10, 1, 20, 99, 99]);
    }
}

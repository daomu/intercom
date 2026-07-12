//! RGB565 framebuffer + `embedded-graphics` `DrawTarget` adapter.
//!
//! `Rgb565Buf` owns a heap-allocated `[u8]` of `w*h*2` bytes and implements
//! `embedded_graphics::draw_target::DrawTarget<Color = Rgb565>`. Pixels are
//! stored big-endian (high byte first) to match the ST7789 16-bit SPI data
//! order expected by `LcdDriver::present`.
//!
//! This is the reusable drawing surface for all UI rendering until (if ever)
//! the slint runtime backend is wired up. `embedded-graphics` primitives
//! (`Rectangle`, `Text`, `Circle`, …) draw directly into this buffer, then
//! `as_bytes()` hands the raw slice to `LcdDriver::present`.

use core::convert::Infallible;

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::Pixel;

/// Heap-allocated RGB565 framebuffer (big-endian, ST7789-compatible).
pub struct Rgb565Buf {
    data: Box<[u8]>,
    w: u32,
    h: u32,
}

impl Rgb565Buf {
    /// Allocate a zeroed `w × h` RGB565 buffer (size = `w*h*2` bytes).
    pub fn new(w: u32, h: u32) -> Self {
        let len = (w as usize) * (h as usize) * 2;
        let data = vec![0u8; len].into_boxed_slice();
        Self { data, w, h }
    }

    /// Raw byte slice for LCD push (`LcdDriver::present`).
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.w
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.h
    }

    /// Fill the entire buffer with one color.
    pub fn fill(&mut self, color: Rgb565) {
        let v = RawU16::from(color).into_inner();
        let hi = (v >> 8) as u8;
        let lo = v as u8;
        for pair in self.data.chunks_mut(2) {
            pair[0] = hi;
            pair[1] = lo;
        }
    }

    /// Bounds-checked write of a single pixel at `(x, y)`.
    #[inline]
    fn set_pixel(&mut self, x: i32, y: i32, color: Rgb565) {
        if x < 0 || y < 0 || (x as u32) >= self.w || (y as u32) >= self.h {
            return;
        }
        let idx = ((y as usize) * (self.w as usize) + (x as usize)) * 2;
        let v = RawU16::from(color).into_inner();
        self.data[idx] = (v >> 8) as u8;
        self.data[idx + 1] = v as u8;
    }

    /// Offset (in bytes) of pixel `(x, y)`, or `None` if out of bounds.
    #[inline]
    fn offset_of(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || (x as u32) >= self.w || (y as u32) >= self.h {
            None
        } else {
            Some(((y as usize) * (self.w as usize) + (x as usize)) * 2)
        }
    }
}

impl Dimensions for Rgb565Buf {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), Size::new(self.w, self.h))
    }
}

impl DrawTarget for Rgb565Buf {
    type Color = Rgb565;
    type Error = Infallible;

    /// Sparse unordered pixel draw (text edges, lines, single pixels).
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels.into_iter() {
            self.set_pixel(point.x, point.y, color);
        }
        Ok(())
    }

    /// Fast solid rectangle fill (clear, background rects).
    fn fill_solid(
        &mut self,
        area: &Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let clip = area.intersection(&self.bounding_box());
        if clip.size.width == 0 || clip.size.height == 0 {
            return Ok(());
        }
        let v = RawU16::from(color).into_inner();
        let hi = (v >> 8) as u8;
        let lo = v as u8;
        let row_w = clip.size.width as usize * 2;
        for ry in 0..clip.size.height as i32 {
            let y = clip.top_left.y + ry;
            if let Some(off) = self.offset_of(clip.top_left.x, y) {
                for b in 0..row_w {
                    self.data[off + b] = if b & 1 == 0 { hi } else { lo };
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_matches_dimensions() {
        let buf = Rgb565Buf::new(240, 240);
        assert_eq!(buf.as_bytes().len(), 240 * 240 * 2);
        assert_eq!(buf.width(), 240);
        assert_eq!(buf.height(), 240);
    }

    #[test]
    fn fill_writes_big_endian() {
        let mut buf = Rgb565Buf::new(4, 1);
        buf.fill(Rgb565::WHITE);
        let bytes = buf.as_bytes();
        // Rgb565 WHITE = 0xFFFF → big-endian [0xFF, 0xFF] per pixel.
        for &b in bytes {
            assert_eq!(b, 0xFF);
        }
    }

    #[test]
    fn fill_solid_rect_writes_only_inside() {
        let mut buf = Rgb565Buf::new(8, 8);
        buf.fill(Rgb565::BLACK);
        let rect = Rectangle::new(Point::new(2, 2), Size::new(3, 2));
        buf.fill_solid(&rect, Rgb565::RED).ok();
        // Inside rect: red.
        assert_pixel(&buf, 2, 2, Rgb565::RED);
        assert_pixel(&buf, 4, 3, Rgb565::RED);
        // Outside: black.
        assert_pixel(&buf, 0, 0, Rgb565::BLACK);
        assert_pixel(&buf, 7, 7, Rgb565::BLACK);
    }

    fn assert_pixel(buf: &Rgb565Buf, x: i32, y: i32, expected: Rgb565) {
        let off = buf.offset_of(x, y).unwrap();
        let got_hi = buf.as_bytes()[off];
        let got_lo = buf.as_bytes()[off + 1];
        let got = RawU16::new(((got_hi as u16) << 8) | got_lo as u16);
        let want = RawU16::from(expected);
        assert_eq!(got.into_inner(), want.into_inner(), "at ({x},{y})");
    }
}

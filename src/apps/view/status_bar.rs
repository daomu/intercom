//! Global top status bar (240×20px).
//!
//! Draws: mode placeholder | mute | grouped | ...... | signal bars | battery | time
//! All icons are embedded-graphics primitives (no bitmap assets).

use embedded_graphics::geometry::Point;
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;

use crate::apps::RenderCtx;
use crate::services::display_buf::Rgb565Buf;

/// Status bar height in pixels.
pub const STATUS_BAR_H: u32 = 20;

/// Draw the status bar at the top of `fb`.
pub fn draw_status_bar(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let bar = Rectangle::new(Point::new(0, 0), Size::new(240, STATUS_BAR_H));
    let _ = bar.into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x10, 0x10, 0x18))).draw(fb);

    let mut x = 4i32;
    let y = 4i32;

    // Mute icon (if muted): draw a small crossed-out speaker ≈ 12×12.
    if ctx.muted {
        draw_mute_icon(fb, x, y);
        x += 16;
    }

    // Grouped status: filled circle = grouped, hollow = ungrouped.
    draw_grouped_dot(fb, x, y + 4, ctx.is_grouped);
    x += 14;

    // Safe-mode indicator: if safe_mode, draw "S" letter.
    if ctx.safe_mode {
        let style = MonoTextStyle::new(&FONT_6X9, Rgb565::new(0xFF, 0x80, 0x00));
        let _ = Text::new("S", Point::new(x, y + 9), style).draw(fb);
    }

    // Right-aligned cluster: time on far right, battery + signal to its left.
    // Time text (HH:MM).
    let style = MonoTextStyle::new(&FONT_6X9, Rgb565::WHITE);
    let time_str = format!("{:02}:{:02}", ctx.time_hms.0, ctx.time_hms.1);
    let time_w = (time_str.len() as i32) * 6;
    let time_x = 240 - time_w - 4;
    let _ = Text::new(&time_str, Point::new(time_x, y + 9), style).draw(fb);

    // Battery 4-step icon, left of time.
    let bat_x = time_x - 22;
    draw_battery_icon(fb, bat_x, y, ctx.battery_step);

    // Signal bars, left of battery.
    let sig_x = bat_x - 22;
    draw_signal_bars(fb, sig_x, y, ctx.signal_bars);
}

fn draw_mute_icon(fb: &mut Rgb565Buf, x: i32, y: i32) {
    let color = Rgb565::new(0xFF, 0x60, 0x60);
    // Speaker body (small rect).
    let body = Rectangle::new(Point::new(x, y + 3), Size::new(4, 6));
    let _ = body.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    // Cone (triangle approximated by lines).
    let cone = Rectangle::new(Point::new(x + 4, y + 1), Size::new(4, 10));
    let _ = cone.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    // Slash (mute slash).
    let slash = embedded_graphics::primitives::Line::new(
        Point::new(x, y),
        Point::new(x + 11, y + 11),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 1));
    let _ = slash.draw(fb);
}

fn draw_grouped_dot(fb: &mut Rgb565Buf, x: i32, y: i32, grouped: bool) {
    let color = if grouped {
        Rgb565::new(0x00, 0xC0, 0x40) // green
    } else {
        Rgb565::new(0x40, 0x40, 0x40) // dim gray
    };
    let dot = Rectangle::new(Point::new(x, y), Size::new(6, 6));
    let style = if grouped {
        PrimitiveStyle::with_fill(color)
    } else {
        PrimitiveStyleBuilder::new().stroke_color(color).stroke_width(1).build()
    };
    let _ = dot.into_styled(style).draw(fb);
}

fn draw_battery_icon(fb: &mut Rgb565Buf, x: i32, y: i32, step: u8) {
    // Battery outline: 16×10 body + 2×4 nub on right.
    let outline = Rectangle::new(Point::new(x, y + 2), Size::new(16, 10));
    let _ = outline
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(Rgb565::WHITE)
                .stroke_width(1)
                .fill_color(Rgb565::BLACK)
                .build(),
        )
        .draw(fb);
    let nub = Rectangle::new(Point::new(x + 16, y + 5), Size::new(2, 4));
    let _ = nub.into_styled(PrimitiveStyle::with_fill(Rgb565::WHITE)).draw(fb);

    // Fill level: step 0=empty red, 1=orange, 2=yellow, 3=green. Fill width = step * 4.
    let (fill_w, color) = match step {
        0 => (0u32, Rgb565::new(0xC0, 0x00, 0x00)),
        1 => (4, Rgb565::new(0xFF, 0x80, 0x00)),
        2 => (8, Rgb565::new(0xFF, 0xE0, 0x00)),
        _ => (12, Rgb565::new(0x20, 0xC0, 0x20)),
    };
    if fill_w > 0 {
        let fill = Rectangle::new(Point::new(x + 2, y + 4), Size::new(fill_w, 6));
        let _ = fill.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    }
}

fn draw_signal_bars(fb: &mut Rgb565Buf, x: i32, y: i32, bars: u8) {
    // 4 ascending bars, each 3px wide, heights 3/5/7/9, gap 1px.
    let heights = [3u32, 5, 7, 9];
    let base_y = y + 12;
    for i in 0..4u8 {
        let h = heights[i as usize];
        let bx = x + (i as i32) * 4;
        let on = i < bars;
        let color = if on {
            Rgb565::new(0x40, 0xC0, 0x40)
        } else {
            Rgb565::new(0x30, 0x30, 0x30)
        };
        let bar = Rectangle::new(Point::new(bx, base_y as i32 - h as i32), Size::new(3, h));
        let _ = bar.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    }
}

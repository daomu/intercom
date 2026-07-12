//! Volume panel overlay: semi-transparent mask + centered slider + mute button.
//!
//! Opened by PLUS short-press, closed by PLUS short-press again or tap on mask.
//! Slider drag adjusts volume 0-100 and persists immediately via save callback.

#![allow(dead_code)]

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;

use crate::apps::RenderCtx;
use crate::services::display_buf::Rgb565Buf;

// Layout.
const PANEL_W: i32 = 200;
const PANEL_H: i32 = 120;
const PANEL_X: i32 = (240 - PANEL_W) / 2;
const PANEL_Y: i32 = (240 - PANEL_H) / 2;

// Colors.
const MASK: Rgb565 = Rgb565::new(0x00, 0x00, 0x00);
const PANEL_BG: Rgb565 = Rgb565::new(0x1C, 0x26, 0x38);
const ACCENT: Rgb565 = Rgb565::new(0x40, 0x80, 0xC0);
const TEXT: Rgb565 = Rgb565::WHITE;
const DIM: Rgb565 = Rgb565::new(0x80, 0x88, 0x90);
const GREEN: Rgb565 = Rgb565::new(0x20, 0xC0, 0x20);
const RED: Rgb565 = Rgb565::new(0xC0, 0x20, 0x20);

/// Draw the volume panel overlay on top of the current framebuffer.
/// `fb` already contains the rendered app view; this draws a dim mask +
/// centered panel with slider + mute button.
pub fn draw_volume_panel(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    // Semi-transparent mask: fill the entire framebuffer with dark overlay.
    let mask = Rectangle::new(Point::new(0, 0), Size::new(240, 240));
    let _ = mask
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x00, 0x00, 0x08)))
        .draw(fb);

    // Panel background + border.
    let panel = Rectangle::new(
        Point::new(PANEL_X, PANEL_Y),
        Size::new(PANEL_W as u32, PANEL_H as u32),
    );
    let _ = panel
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(ACCENT)
                .stroke_width(2)
                .fill_color(PANEL_BG)
                .build(),
        )
        .draw(fb);

    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    // Title.
    let _ = Text::new("Volume", Point::new(PANEL_X + 76, PANEL_Y + 16), style).draw(fb);

    // Value text.
    let val_str = format!("{}%", ctx.settings.volume);
    let vw = (val_str.len() as i32) * 6;
    let vx = PANEL_X + (PANEL_W - vw) / 2;
    let _ = Text::new(&val_str, Point::new(vx, PANEL_Y + 34), style).draw(fb);

    // Slider track.
    let track_y = PANEL_Y + 60;
    let track_x = PANEL_X + 16;
    let track_w = PANEL_W - 32;
    let track = Rectangle::new(
        Point::new(track_x, track_y),
        Size::new(track_w as u32, 8),
    );
    let _ = track
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x30, 0x30, 0x38)))
        .draw(fb);

    // Filled portion.
    let fill_w = ((ctx.settings.volume as u32) * track_w as u32 / 100) as u32;
    if fill_w > 0 {
        let fill = Rectangle::new(
            Point::new(track_x, track_y),
            Size::new(fill_w, 8),
        );
        let _ = fill
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb);
    }

    // Knob.
    let knob_x = track_x + (ctx.settings.volume as i32 * track_w / 100);
    let knob = Rectangle::new(
        Point::new(knob_x - 3, track_y - 4),
        Size::new(8, 16),
    );
    let _ = knob.into_styled(PrimitiveStyle::with_fill(TEXT)).draw(fb);

    // Min/Max labels.
    let _ = Text::new("0", Point::new(track_x - 4, track_y + 16), dim_style).draw(fb);
    let _ = Text::new("100", Point::new(track_x + track_w - 12, track_y + 16), dim_style).draw(fb);

    // Mute button at bottom of panel.
    let (mute_color, mute_label) = if ctx.settings.muted {
        (RED, "UNMUTE")
    } else {
        (GREEN, "MUTE")
    };
    let mute_btn = Rectangle::new(
        Point::new(PANEL_X + (PANEL_W - 80) / 2, PANEL_Y + 88),
        Size::new(80, 24),
    );
    let _ = mute_btn
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(mute_color)
                .stroke_width(2)
                .fill_color(Rgb565::new(0x20, 0x28, 0x38))
                .build(),
        )
        .draw(fb);
    let mw = (mute_label.len() as i32) * 6;
    let mx = PANEL_X + (PANEL_W - mw) / 2;
    let _ = Text::new(mute_label, Point::new(mx, PANEL_Y + 103), style).draw(fb);
}

/// Hit-test for volume panel overlay.
/// Returns Some(VolumeMuteBtn) if mute button hit, Some(VolumePanelClose) if
/// mask area hit (tap outside panel closes), None otherwise.
pub fn hit_test(x: i32, y: i32) -> Option<crate::apps::HitTarget> {
    // Mute button.
    let mute_x = PANEL_X + (PANEL_W - 80) / 2;
    let mute_y = PANEL_Y + 88;
    if x >= mute_x && x < mute_x + 80 && y >= mute_y && y < mute_y + 24 {
        return Some(crate::apps::HitTarget::VolumeMuteBtn);
    }
    // Tap outside panel → close.
    if x < PANEL_X || x >= PANEL_X + PANEL_W || y < PANEL_Y || y >= PANEL_Y + PANEL_H {
        return Some(crate::apps::HitTarget::VolumePanelClose);
    }
    None
}

/// Slider track bounds (for drag-to-adjust).
pub fn slider_bounds() -> (i32, i32, i32) {
    let track_x = PANEL_X + 16;
    let track_w = PANEL_W - 32;
    (track_x, PANEL_Y + 60, track_w)
}

/// Map an x-coordinate to a volume value (0-100) based on slider position.
pub fn x_to_volume(x: i32) -> u8 {
    let (track_x, _, track_w) = slider_bounds();
    let rel = (x - track_x).max(0).min(track_w);
    ((rel as u32 * 100 / track_w as u32) as u8).min(100)
}

//! Launcher home view: 2×1 app-entry grid (Intercom / Settings).
//!
//! Draws two tiles below the status bar. Each tile = filled rounded-ish
//! rectangle + icon (primitives) + English label. No touch wiring here —
//! `hit_test` maps coordinates to `HitTarget` for the controller.

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use crate::apps::{HitTarget, RenderCtx};
use crate::apps::view::status_bar::STATUS_BAR_H;
use crate::services::display_buf::Rgb565Buf;

/// Tile layout: 2 columns × 1 row, each 116×200, with 4px gaps + 4px margin.
const TILE_W: i32 = 116;
const TILE_H: i32 = 200;
const MARGIN: i32 = 4;
const GAP: i32 = 4;

/// Draw the Launcher home grid. Caller has already drawn the status bar.
pub fn draw_launcher(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    // Clear content area below status bar.
    let content = Rectangle::new(
        Point::new(0, STATUS_BAR_H as i32),
        Size::new(240, 240 - STATUS_BAR_H),
    );
    let _ = content
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x04, 0x06, 0x0C)))
        .draw(fb);

    // Left tile: Intercom. Right tile: Settings.
    let left_x = MARGIN;
    let right_x = MARGIN + TILE_W + GAP;
    let top_y = STATUS_BAR_H as i32 + MARGIN;

    draw_tile(fb, left_x, top_y, TileKind::Intercom, ctx);
    draw_tile(fb, right_x, top_y, TileKind::Settings, ctx);
}

#[derive(Clone, Copy)]
enum TileKind {
    Intercom,
    Settings,
}

fn draw_tile(fb: &mut Rgb565Buf, x: i32, y: i32, kind: TileKind, _ctx: &RenderCtx) {
    let (bg, border, label) = match kind {
        TileKind::Intercom => (
            Rgb565::new(0x18, 0x2A, 0x3A),
            Rgb565::new(0x40, 0x80, 0xC0),
            "Intercom",
        ),
        TileKind::Settings => (
            Rgb565::new(0x2A, 0x26, 0x18),
            Rgb565::new(0xC0, 0xA0, 0x40),
            "Settings",
        ),
    };
    // Tile background.
    let tile = Rectangle::new(Point::new(x, y), Size::new(TILE_W as u32, TILE_H as u32));
    let _ = tile.into_styled(PrimitiveStyle::with_fill(bg)).draw(fb);
    // Tile border.
    let _ = tile
        .into_styled(PrimitiveStyleBuilder::new().stroke_color(border).stroke_width(2).build())
        .draw(fb);

    // Icon centered horizontally, ~60px above center.
    let icon_cx = x + TILE_W / 2;
    let icon_cy = y + TILE_H / 2 - 30;
    match kind {
        TileKind::Intercom => draw_intercom_icon(fb, icon_cx, icon_cy, border),
        TileKind::Settings => draw_settings_icon(fb, icon_cx, icon_cy, border),
    }

    // Label centered below icon.
    let style = MonoTextStyle::new(&FONT_6X9, Rgb565::WHITE);
    let label_w = (label.len() as i32) * 6;
    let lx = x + (TILE_W - label_w) / 2;
    let ly = icon_cy + 30;
    let _ = Text::new(label, Point::new(lx, ly), style).draw(fb);
}

/// Intercom icon: speaker + sound waves.
fn draw_intercom_icon(fb: &mut Rgb565Buf, cx: i32, cy: i32, color: Rgb565) {
    // Speaker body (small rect on left).
    let body = Rectangle::new(Point::new(cx - 18, cy - 5), Size::new(8, 12));
    let _ = body.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    // Cone (triangle-ish: wider rect to the right).
    let cone = Rectangle::new(Point::new(cx - 10, cy - 10), Size::new(6, 22));
    let _ = cone.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    // Sound wave arcs (approximated by arcs/circles — use small rects as brackets).
    for (i, &(dx, h)) in [(2i32, 8i32), (8, 14), (14, 20)].iter().enumerate() {
        let _ = i; // suppress unused
        let wave = embedded_graphics::primitives::Line::new(
            Point::new(cx + dx, cy - h / 2),
            Point::new(cx + dx, cy + h / 2),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 2));
        let _ = wave.draw(fb);
    }
}

/// Settings icon: gear (approximated by a circle with 8 small nubs).
fn draw_settings_icon(fb: &mut Rgb565Buf, cx: i32, cy: i32, color: Rgb565) {
    let r = 12i32;
    // Outer ring.
    let ring = Rectangle::new(Point::new(cx - r, cy - r), Size::new((r * 2) as u32, (r * 2) as u32));
    let _ = ring
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(color)
                .stroke_width(2)
                .fill_color(Rgb565::BLACK)
                .build(),
        )
        .draw(fb);
    // Center hole.
    let hole = Rectangle::new(Point::new(cx - 3, cy - 3), Size::new(6, 6));
    let _ = hole.into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK)).draw(fb);
    // 4 nubs (top/bottom/left/right) — small rects extending beyond ring.
    for &(dx, dy) in &[(0i32, -16), (0, 16), (-16, 0), (16, 0)] {
        let nub = Rectangle::new(Point::new(cx + dx - 2, cy + dy - 2), Size::new(4, 4));
        let _ = nub.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    }
}

/// Procedural hit-test for the Launcher grid.
pub fn hit_test(_x: i32, y: i32) -> Option<HitTarget> {
    if y < (STATUS_BAR_H as i32) {
        return None; // status bar area
    }
    let left_start = MARGIN;
    let right_start = MARGIN + TILE_W + GAP;
    if _x >= left_start && _x < right_start {
        Some(HitTarget::LauncherIntercomTile)
    } else if _x >= right_start && _x < right_start + TILE_W {
        Some(HitTarget::LauncherSettingsTile)
    } else {
        None
    }
}

// Bring PrimitiveStyleBuilder into scope (used via full path above, but alias for clarity).
use embedded_graphics::primitives::PrimitiveStyleBuilder;

//! Intercom view: peer cards + PTT area + not-grouped / voice-changer / group-info pages.
//!
//! Layout (below 20px status bar, 220px content):
//!   - Not grouped: centered "No Group" + instructions
//!   - Main page:  peer cards (adaptive 1/2/4 layout) + bottom PTT area (40px)
//!   - Voice changer: 3 options (Normal/PitchUp/PitchDown) + highlight
//!   - Group info: group details + exit button → confirmation modal
//!
//! CJK font subset deferred — all labels are ASCII (FONT_6X9).

#![allow(dead_code)]

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;

use crate::apps::view::status_bar::STATUS_BAR_H;
use crate::apps::intercom_app::{IntercomApp, IntercomUiState, PeerCard};
use crate::apps::RenderCtx;
use crate::services::display_buf::Rgb565Buf;

// Colors.
const BG: Rgb565 = Rgb565::new(0x04, 0x06, 0x0C);
const PANEL: Rgb565 = Rgb565::new(0x18, 0x22, 0x30);
const ACCENT: Rgb565 = Rgb565::new(0x40, 0x80, 0xC0);
const TEXT: Rgb565 = Rgb565::WHITE;
const DIM: Rgb565 = Rgb565::new(0x80, 0x88, 0x90);
const GREEN: Rgb565 = Rgb565::new(0x20, 0xC0, 0x20);
const RED: Rgb565 = Rgb565::new(0xC0, 0x20, 0x20);
const YELLOW: Rgb565 = Rgb565::new(0xFF, 0xE0, 0x00);
const ORANGE: Rgb565 = Rgb565::new(0xFF, 0x80, 0x00);

// Layout.
const SCREEN_W: i32 = 240;
const SCREEN_H: i32 = 240;
const PTT_AREA_H: i32 = 40;
const CONTENT_TOP: i32 = STATUS_BAR_H as i32;
const PTT_TOP: i32 = SCREEN_H - PTT_AREA_H;

/// Intercom view pages (swipe-switched).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntercomPage {
    Main,
    VoiceChanger,
    GroupInfo,
}

/// Draw the Intercom app. Caller has already drawn the status bar.
pub fn draw_intercom(fb: &mut Rgb565Buf, ctx: &RenderCtx, app: &IntercomApp, page: IntercomPage) {
    // Clear content area.
    let content = Rectangle::new(
        Point::new(0, CONTENT_TOP),
        Size::new(SCREEN_W as u32, (SCREEN_H - CONTENT_TOP) as u32),
    );
    let _ = content.into_styled(PrimitiveStyle::with_fill(BG)).draw(fb);

    if !ctx.is_grouped {
        draw_not_grouped(fb);
        return;
    }

    match page {
        IntercomPage::Main => draw_main_page(fb, ctx, app),
        IntercomPage::VoiceChanger => draw_voice_changer_page(fb),
        IntercomPage::GroupInfo => draw_group_info_page(fb, ctx),
    }
}

// ---- Not-grouped page ----------------------------------------------------

fn draw_not_grouped(fb: &mut Rgb565Buf) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("No Group", Point::new(90, 60), style).draw(fb);
    let _ = Text::new("Not connected", Point::new(72, 90), dim_style).draw(fb);
    let _ = Text::new("to any group.", Point::new(72, 104), dim_style).draw(fb);

    // Instructions.
    let _ = Text::new("Use Settings to", Point::new(66, 140), dim_style).draw(fb);
    let _ = Text::new("create or join", Point::new(72, 154), dim_style).draw(fb);
    let _ = Text::new("a group.", Point::new(84, 168), dim_style).draw(fb);
}

// ---- Main intercom page --------------------------------------------------

fn draw_main_page(fb: &mut Rgb565Buf, _ctx: &RenderCtx, app: &IntercomApp) {
    let peers = app.peers();
    let ui_state = app.ui_state();

    if peers.is_empty() {
        let style = MonoTextStyle::new(&FONT_6X9, DIM);
        let _ = Text::new("No peers online", Point::new(60, 80), style).draw(fb);
        let _ = Text::new("Waiting for", Point::new(72, 100), style).draw(fb);
        let _ = Text::new("heartbeats...", Point::new(66, 114), style).draw(fb);
    } else {
        draw_peer_cards(fb, peers);
    }

    // UI state indicator (top-right of content area).
    draw_state_indicator(fb, ui_state, peers.len());

    // PTT area at bottom.
    draw_ptt_area(fb, ui_state);
}

fn draw_peer_cards(fb: &mut Rgb565Buf, peers: &[PeerCard]) {
    let area_top = CONTENT_TOP + 4;
    let area_h = PTT_TOP - area_top - 4;
    let area_w = SCREEN_W;

    match peers.len() {
        1 => draw_single_card(fb, &peers[0], 4, area_top, area_w - 8, area_h),
        2 => {
            let half_w = (area_w - 12) / 2;
            draw_single_card(fb, &peers[0], 4, area_top, half_w, area_h);
            draw_single_card(fb, &peers[1], 4 + half_w + 4, area_top, half_w, area_h);
        }
        3 => {
            let half_w = (area_w - 12) / 2;
            let top_h = (area_h - 4) / 2;
            draw_single_card(fb, &peers[0], 4, area_top, half_w, top_h);
            draw_single_card(fb, &peers[1], 4 + half_w + 4, area_top, half_w, top_h);
            draw_single_card(fb, &peers[2], 4, area_top + top_h + 4, area_w - 8, top_h);
        }
        _ => {
            // 4+ peers: 2×2 grid (show first 4).
            let half_w = (area_w - 12) / 2;
            let half_h = (area_h - 4) / 2;
            for (i, p) in peers.iter().take(4).enumerate() {
                let col = (i % 2) as i32;
                let row = (i / 2) as i32;
                let x = 4 + col * (half_w + 4);
                let y = area_top + row * (half_h + 4);
                draw_single_card(fb, p, x, y, half_w, half_h);
            }
        }
    }
}

fn draw_single_card(fb: &mut Rgb565Buf, peer: &PeerCard, x: i32, y: i32, w: i32, h: i32) {
    let bg = if peer.online { PANEL } else { Rgb565::new(0x0C, 0x10, 0x18) };
    let border = if peer.voice_active { GREEN } else { ACCENT };

    // Card background + border.
    let card = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
    let _ = card
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(border)
                .stroke_width(if peer.voice_active { 2 } else { 1 })
                .fill_color(bg)
                .build(),
        )
        .draw(fb);

    let style = MonoTextStyle::new(&FONT_6X9, if peer.online { TEXT } else { DIM });

    // Name (truncated to fit card width).
    let max_chars = (w as usize / 6) - 1;
    let name: String = peer.name.chars().take(max_chars).collect();
    let _ = Text::new(&name, Point::new(x + 6, y + 16), style).draw(fb);

    // Online/offline indicator.
    let dot_color = if peer.online { GREEN } else { DIM };
    let dot = Rectangle::new(Point::new(x + w - 12, y + 8), Size::new(6, 6));
    let _ = dot.into_styled(PrimitiveStyle::with_fill(dot_color)).draw(fb);

    // Signal bars (if online).
    if peer.online {
        draw_mini_signal_bars(fb, x + 6, y + h - 16, peer.rssi_bars);
    }

    // Voice-active indicator.
    if peer.voice_active {
        let _ = Text::new("TALK", Point::new(x + w - 30, y + h - 12), MonoTextStyle::new(&FONT_6X9, GREEN)).draw(fb);
    }
}

fn draw_mini_signal_bars(fb: &mut Rgb565Buf, x: i32, y: i32, bars: u8) {
    let heights = [3u32, 5, 7, 9];
    let base_y = y + 10;
    for i in 0..4u8 {
        let h = heights[i as usize];
        let bx = x + (i as i32) * 4;
        let color = if i < bars {
            Rgb565::new(0x40, 0xC0, 0x40)
        } else {
            Rgb565::new(0x30, 0x30, 0x30)
        };
        let bar = Rectangle::new(Point::new(bx, base_y - h as i32), Size::new(3, h));
        let _ = bar.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    }
}

fn draw_state_indicator(fb: &mut Rgb565Buf, state: IntercomUiState, peer_count: usize) {
    let (text, color) = match state {
        IntercomUiState::Idle => (format!("Idle |{}p", peer_count), DIM),
        IntercomUiState::Listening => ("Listen".to_string(), GREEN),
        IntercomUiState::PttArming => ("Arming".to_string(), YELLOW),
        IntercomUiState::PttActive => ("TALKING".to_string(), RED),
        IntercomUiState::ChannelBusy => ("Busy".to_string(), ORANGE),
    };
    let style = MonoTextStyle::new(&FONT_6X9, color);
    let tw = (text.len() as i32) * 6;
    let _ = Text::new(&text, Point::new(SCREEN_W - tw - 4, CONTENT_TOP + 4), style).draw(fb);
}

fn draw_ptt_area(fb: &mut Rgb565Buf, state: IntercomUiState) {
    let (bg, border, label) = match state {
        IntercomUiState::PttActive => (Rgb565::new(0x40, 0x10, 0x10), RED, "RELEASE"),
        IntercomUiState::PttArming => (Rgb565::new(0x30, 0x28, 0x08), YELLOW, "ARMING..."),
        IntercomUiState::ChannelBusy => (Rgb565::new(0x20, 0x18, 0x08), ORANGE, "BUSY"),
        IntercomUiState::Listening => (Rgb565::new(0x08, 0x20, 0x10), GREEN, "PTT"),
        IntercomUiState::Idle => (Rgb565::new(0x18, 0x22, 0x30), ACCENT, "PTT"),
    };

    let ptt = Rectangle::new(
        Point::new(0, PTT_TOP),
        Size::new(SCREEN_W as u32, PTT_AREA_H as u32),
    );
    let _ = ptt
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(border)
                .stroke_width(2)
                .fill_color(bg)
                .build(),
        )
        .draw(fb);

    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let lw = (label.len() as i32) * 6;
    let lx = (SCREEN_W - lw) / 2;
    let ly = PTT_TOP + PTT_AREA_H / 2 + 4;
    let _ = Text::new(label, Point::new(lx, ly), style).draw(fb);
}

// ---- Voice changer page -------------------------------------------------

fn draw_voice_changer_page(fb: &mut Rgb565Buf) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Voice Changer", Point::new(60, CONTENT_TOP + 10), style).draw(fb);

    let options = [
        ("Normal", false),
        ("Pitch Up", false),
        ("Pitch Down", false),
    ];
    let mut y = CONTENT_TOP + 40;
    for (label, _selected) in &options {
        let row = Rectangle::new(Point::new(20, y), Size::new(200, 28));
        let _ = row
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .stroke_color(ACCENT)
                    .stroke_width(1)
                    .fill_color(PANEL)
                    .build(),
            )
            .draw(fb);
        let _ = Text::new(label, Point::new(36, y + 18), style).draw(fb);
        y += 34;
    }

    let _ = Text::new("(Phase 5: selection)", Point::new(60, y + 10), dim_style).draw(fb);
}

// ---- Group info page ----------------------------------------------------

fn draw_group_info_page(fb: &mut Rgb565Buf, _ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Group Info", Point::new(72, CONTENT_TOP + 10), style).draw(fb);

    let mut y = CONTENT_TOP + 36;
    let row_h = 22;

    let _ = Text::new("Status:", Point::new(16, y + 12), dim_style).draw(fb);
    let _ = Text::new("Connected", Point::new(120, y + 12), MonoTextStyle::new(&FONT_6X9, GREEN)).draw(fb);
    y += row_h;

    let _ = Text::new("Peers:", Point::new(16, y + 12), dim_style).draw(fb);
    let _ = Text::new("0", Point::new(120, y + 12), style).draw(fb);
    y += row_h;

    // Exit button.
    let exit_btn = Rectangle::new(Point::new(60, y + 20), Size::new(120, 28));
    let _ = exit_btn
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(RED)
                .stroke_width(2)
                .fill_color(Rgb565::new(0x30, 0x10, 0x10))
                .build(),
        )
        .draw(fb);
    let _ = Text::new("Exit Group", Point::new(72, y + 37), MonoTextStyle::new(&FONT_6X9, TEXT)).draw(fb);
}

/// Procedural hit-test for Intercom pages.
pub fn hit_test(_x: i32, y: i32, _page: IntercomPage) -> Option<crate::apps::HitTarget> {
    // PTT area (bottom 40px).
    if y >= PTT_TOP {
        return Some(crate::apps::HitTarget::IntercomPttArea);
    }
    None
}

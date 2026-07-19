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
use crate::apps::intercom_app::{IntercomApp, IntercomUiState, PeerCard, VoiceChangerSubState};
use crate::apps::RenderCtx;
use crate::intercom::pairing::HostInfo;
use crate::intercom::state::VoiceEffect;
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

/// Intercom view pages (swipe-switched). The `Ungrouped*` pages replace the
/// old `draw_not_grouped` placeholder (change intercom-ungrouped-ui).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntercomPage {
    Main,
    VoiceChanger,
    GroupInfo,
    /// Ungrouped entry: Create Group / Search Groups buttons.
    UngroupedHome,
    /// Hosting in progress: broadcasting + peer count + Back.
    CreatingHost,
    /// Searching: discovered-host list + Refresh / Join / Back.
    SearchingHosts,
    /// Joining a selected host: waiting for approval + Cancel.
    JoiningHost,
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
        // Ungrouped: render the pairing-entry flow pages. Any grouped page
        // falls back to the home entry.
        match page {
            IntercomPage::CreatingHost => draw_creating_host(fb, ctx, app),
            IntercomPage::SearchingHosts => draw_searching_hosts(fb, app),
            IntercomPage::JoiningHost => draw_joining_host(fb, app),
            _ => draw_ungrouped_home(fb, app),
        }
        return;
    }

    match page {
        IntercomPage::VoiceChanger => draw_voice_changer_page(fb, app),
        IntercomPage::GroupInfo => draw_group_info_page(fb, ctx, app),
        // Main + any ungrouped page (stale after grouping) → Main.
        _ => draw_main_page(fb, ctx, app),
    }
}

// ---- Ungrouped pairing-entry pages (change intercom-ungrouped-ui) ---------

// Ungrouped button geometry (shared by draw + hit_test).
const UG_BTN_X: i32 = 20;
const UG_BTN_W: i32 = 200;
const CREATE_BTN_Y: i32 = 80;
const SEARCH_BTN_Y: i32 = 160;
const UG_BTN_H: i32 = 60;
// Search page action buttons (bottom row).
const SP_BTN_Y: i32 = 200;
const SP_BTN_H: i32 = 30;
const SP_REFRESH_X: i32 = 8;
const SP_JOIN_X: i32 = 88;
const SP_BACK_X: i32 = 168;
const SP_BTN_W: i32 = 64;
// Host list rows.
const HOST_ROW_TOP: i32 = CONTENT_TOP + 30;
const HOST_ROW_H: i32 = 22;
const HOST_ROWS_MAX: usize = 8;
// Back button on Creating/Joining pages.
const PB_BACK_X: i32 = 70;
const PB_BACK_Y: i32 = 190;
const PB_BACK_W: i32 = 100;
const PB_BACK_H: i32 = 32;

fn draw_button(fb: &mut Rgb565Buf, x: i32, y: i32, w: i32, h: i32, label: &str, color: Rgb565) {
    let btn = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
    let _ = btn
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(color)
                .stroke_width(2)
                .fill_color(PANEL)
                .build(),
        )
        .draw(fb);
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let lw = (label.len() as i32) * 6;
    let lx = x + (w - lw) / 2;
    let ly = y + h / 2 + 4;
    let _ = Text::new(label, Point::new(lx, ly), style).draw(fb);
}

fn draw_ungrouped_home(fb: &mut Rgb565Buf, app: &IntercomApp) {
    let title = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    // join_error toast (task 7.1).
    if let Some(err) = app.join_error() {
        let red = MonoTextStyle::new(&FONT_6X9, RED);
        let _ = Text::new(err, Point::new(20, CONTENT_TOP + 12), red).draw(fb);
    }

    let _ = Text::new("Intercom Setup", Point::new(60, CONTENT_TOP + 28), title).draw(fb);

    draw_button(fb, UG_BTN_X, CREATE_BTN_Y, UG_BTN_W, UG_BTN_H, "Create Group", GREEN);
    draw_button(fb, UG_BTN_X, SEARCH_BTN_Y, UG_BTN_W, UG_BTN_H, "Search Groups", ACCENT);

    let _ = Text::new("Host: others join you.", Point::new(24, CREATE_BTN_Y + UG_BTN_H + 14), dim_style).draw(fb);
    let _ = Text::new("Join: needs host online.", Point::new(24, SEARCH_BTN_Y + UG_BTN_H + 14), dim_style).draw(fb);
}

fn draw_creating_host(fb: &mut Rgb565Buf, ctx: &RenderCtx, app: &IntercomApp) {
    let title = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Creating Group", Point::new(60, CONTENT_TOP + 24), title).draw(fb);

    // Blinking spinner dot (based on time seconds parity as a coarse tick).
    let on = (ctx.time_hms.2 % 2) == 0;
    let dot_color = if on { GREEN } else { DIM };
    let dot = Rectangle::new(Point::new(116, CONTENT_TOP + 50), Size::new(8, 8));
    let _ = dot.into_styled(PrimitiveStyle::with_fill(dot_color)).draw(fb);

    let _ = Text::new("Broadcasting...", Point::new(66, CONTENT_TOP + 80), dim_style).draw(fb);
    let peers = format!("Peers joined: {}", app.creating_peer_count());
    let _ = Text::new(&peers, Point::new(60, CONTENT_TOP + 100), title).draw(fb);

    draw_button(fb, PB_BACK_X, PB_BACK_Y, PB_BACK_W, PB_BACK_H, "Back", RED);
}

fn draw_searching_hosts(fb: &mut Rgb565Buf, app: &IntercomApp) {
    let title = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Search Groups", Point::new(60, CONTENT_TOP + 12), title).draw(fb);

    let hosts = app.discovered_hosts();
    if hosts.is_empty() {
        let _ = Text::new("Searching...", Point::new(72, HOST_ROW_TOP + 20), dim_style).draw(fb);
    } else {
        let selected = app.selected_host();
        for (i, h) in hosts.iter().take(HOST_ROWS_MAX).enumerate() {
            let info = HostInfo::from_discovered(h);
            let y = HOST_ROW_TOP + (i as i32) * HOST_ROW_H;
            let is_sel = selected == Some(i);
            let row_bg = if is_sel { ACCENT } else { PANEL };
            let row = Rectangle::new(Point::new(4, y), Size::new((SCREEN_W - 8) as u32, (HOST_ROW_H - 2) as u32));
            let _ = row.into_styled(PrimitiveStyle::with_fill(row_bg)).draw(fb);
            let label = format!("{}  {}bars {}/{}", info.name, info.rssi_4bar, info.cur_members, info.max_members);
            let _ = Text::new(&label, Point::new(10, y + 14), title).draw(fb);
        }
    }

    draw_button(fb, SP_REFRESH_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H, "Refresh", ACCENT);
    let join_color = if app.selected_host().is_some() { GREEN } else { DIM };
    draw_button(fb, SP_JOIN_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H, "Join", join_color);
    draw_button(fb, SP_BACK_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H, "Back", RED);
}

fn draw_joining_host(fb: &mut Rgb565Buf, app: &IntercomApp) {
    let title = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let name = app
        .selected_host()
        .and_then(|i| app.discovered_hosts().get(i))
        .map(|h| HostInfo::from_discovered(h).name)
        .unwrap_or_else(|| "host".to_string());
    let head = format!("Joining {}", name);
    let _ = Text::new(&head, Point::new(40, CONTENT_TOP + 40), title).draw(fb);
    let _ = Text::new("Waiting for approval", Point::new(48, CONTENT_TOP + 70), dim_style).draw(fb);

    draw_button(fb, PB_BACK_X, PB_BACK_Y, PB_BACK_W, PB_BACK_H, "Cancel", RED);
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

// ---- Voice changer page (change intercom-voice-changer-preview) ---------

// Effect-button + cancel geometry (shared by draw + hit_test).
const VC_BTN_X: i32 = 20;
const VC_BTN_W: i32 = 200;
const VC_BTN_H: i32 = 28;
const VC_BTN_GAP: i32 = 8;
const VC_BTN_TOP: i32 = CONTENT_TOP + 20;
const VC_STATUS_Y: i32 = 158;
const VC_CANCEL_X: i32 = 70;
const VC_CANCEL_Y: i32 = 166;
const VC_CANCEL_W: i32 = 100;
const VC_CANCEL_H: i32 = 28;

const VC_EFFECTS: [VoiceEffect; 3] =
    [VoiceEffect::Normal, VoiceEffect::PitchUp, VoiceEffect::PitchDown];

fn effect_label(e: VoiceEffect) -> &'static str {
    match e {
        VoiceEffect::Normal => "Normal",
        VoiceEffect::PitchUp => "Pitch Up",
        VoiceEffect::PitchDown => "Pitch Down",
    }
}

fn draw_voice_changer_page(fb: &mut Rgb565Buf, app: &IntercomApp) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);
    let red_style = MonoTextStyle::new(&FONT_6X9, RED);

    let _ = Text::new("Voice Changer", Point::new(60, CONTENT_TOP + 12), style).draw(fb);

    let vc = app.vc_state();
    let in_call = app.in_call();
    let active = match vc {
        VoiceChangerSubState::Recording { target, .. } => Some(target),
        VoiceChangerSubState::Previewing { effect, .. } => Some(effect),
        VoiceChangerSubState::Idle => None,
    };
    let recording = matches!(vc, VoiceChangerSubState::Recording { .. });

    // 3 effect buttons. Highlight the active effect (yellow); dim while
    // recording or during a call (task 3.1 / 2.2).
    for (i, eff) in VC_EFFECTS.iter().enumerate() {
        let by = VC_BTN_TOP + (i as i32) * (VC_BTN_H + VC_BTN_GAP);
        let color = if active == Some(*eff) {
            YELLOW
        } else if recording || in_call {
            DIM
        } else {
            ACCENT
        };
        draw_button(fb, VC_BTN_X, by, VC_BTN_W, VC_BTN_H, effect_label(*eff), color);
    }

    // Status line + cancel button by sub-state.
    match vc {
        VoiceChangerSubState::Idle => {
            let hint = if in_call {
                "Cannot preview during call"
            } else {
                "Tap effect to preview 3s"
            };
            let _ = Text::new(hint, Point::new(30, VC_STATUS_Y), dim_style).draw(fb);
        }
        VoiceChangerSubState::Recording { remain_ms, .. } => {
            let secs = (remain_ms + 999) / 1000;
            let msg = format!("Recording... {}s", secs);
            let _ = Text::new(&msg, Point::new(30, VC_STATUS_Y), red_style).draw(fb);
            draw_button(fb, VC_CANCEL_X, VC_CANCEL_Y, VC_CANCEL_W, VC_CANCEL_H, "Cancel", ACCENT);
        }
        VoiceChangerSubState::Previewing { remain_ms, effect } => {
            let secs = (remain_ms + 999) / 1000;
            let msg = format!("Preview {} {}s", effect_label(effect), secs);
            let _ = Text::new(&msg, Point::new(20, VC_STATUS_Y), style).draw(fb);
            draw_button(fb, VC_CANCEL_X, VC_CANCEL_Y, VC_CANCEL_W, VC_CANCEL_H, "Cancel", ACCENT);
        }
    }
}

// ---- Group info page ----------------------------------------------------

// Leave-group button + confirmation modal geometry (shared by draw + hit_test).
const LEAVE_BTN_X: i32 = 20;
const LEAVE_BTN_Y: i32 = 200;
const LEAVE_BTN_W: i32 = 200;
const LEAVE_BTN_H: i32 = 34;
// Modal card + its two buttons.
const MODAL_X: i32 = 30;
const MODAL_Y: i32 = 70;
const MODAL_W: i32 = 180;
const MODAL_H: i32 = 100;
const MODAL_CANCEL_X: i32 = 40;
const MODAL_CONFIRM_X: i32 = 128;
const MODAL_BTN_Y: i32 = 132;
const MODAL_BTN_W: i32 = 72;
const MODAL_BTN_H: i32 = 28;

fn draw_group_info_page(fb: &mut Rgb565Buf, _ctx: &RenderCtx, app: &IntercomApp) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Group Info", Point::new(72, CONTENT_TOP + 10), style).draw(fb);

    let mut y = CONTENT_TOP + 36;
    let row_h = 22;

    let _ = Text::new("Status:", Point::new(16, y + 12), dim_style).draw(fb);
    let _ = Text::new("Connected", Point::new(120, y + 12), MonoTextStyle::new(&FONT_6X9, GREEN)).draw(fb);
    y += row_h;

    let peers = format!("{}", app.peers().len());
    let _ = Text::new("Peers:", Point::new(16, y + 12), dim_style).draw(fb);
    let _ = Text::new(&peers, Point::new(120, y + 12), style).draw(fb);

    // Leave Group button (bottom).
    draw_button(fb, LEAVE_BTN_X, LEAVE_BTN_Y, LEAVE_BTN_W, LEAVE_BTN_H, "Leave Group", RED);

    // Confirmation modal overlay.
    if app.confirming_leave() {
        draw_leave_modal(fb);
    }
}

/// Draw the leave-group confirmation modal. Rgb565 has no alpha channel, so
/// the "semi-transparent mask" is approximated by dimming the content area
/// with a solid dark fill before the centered card (change
/// intercom-group-info-leave, task 2.3 adapted).
fn draw_leave_modal(fb: &mut Rgb565Buf) {
    let mask = Rectangle::new(
        Point::new(0, CONTENT_TOP),
        Size::new(SCREEN_W as u32, (SCREEN_H - CONTENT_TOP) as u32),
    );
    let _ = mask
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x02, 0x03, 0x06)))
        .draw(fb);

    let card = Rectangle::new(Point::new(MODAL_X, MODAL_Y), Size::new(MODAL_W as u32, MODAL_H as u32));
    let _ = card
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(RED)
                .stroke_width(2)
                .fill_color(PANEL)
                .build(),
        )
        .draw(fb);

    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);
    let _ = Text::new("Leave Group?", Point::new(MODAL_X + 44, MODAL_Y + 22), style).draw(fb);
    let _ = Text::new("Lose all peers.", Point::new(MODAL_X + 40, MODAL_Y + 44), dim_style).draw(fb);

    draw_button(fb, MODAL_CANCEL_X, MODAL_BTN_Y, MODAL_BTN_W, MODAL_BTN_H, "Cancel", ACCENT);
    draw_button(fb, MODAL_CONFIRM_X, MODAL_BTN_Y, MODAL_BTN_W, MODAL_BTN_H, "Confirm", RED);
}

/// Procedural hit-test for Intercom pages. `confirming_leave` gates the
/// GroupInfo leave-confirmation modal (change intercom-group-info-leave).
pub fn hit_test(x: i32, y: i32, page: IntercomPage, confirming_leave: bool, vc_state: VoiceChangerSubState) -> Option<crate::apps::HitTarget> {
    use crate::apps::HitTarget;

    let in_rect = |bx: i32, by: i32, bw: i32, bh: i32| {
        x >= bx && x < bx + bw && y >= by && y < by + bh
    };

    match page {
        IntercomPage::UngroupedHome => {
            if in_rect(UG_BTN_X, CREATE_BTN_Y, UG_BTN_W, UG_BTN_H) {
                return Some(HitTarget::CreateHostButton);
            }
            if in_rect(UG_BTN_X, SEARCH_BTN_Y, UG_BTN_W, UG_BTN_H) {
                return Some(HitTarget::SearchHostsButton);
            }
            None
        }
        IntercomPage::CreatingHost | IntercomPage::JoiningHost => {
            if in_rect(PB_BACK_X, PB_BACK_Y, PB_BACK_W, PB_BACK_H) {
                return Some(HitTarget::PairingBackButton);
            }
            None
        }
        IntercomPage::SearchingHosts => {
            if in_rect(SP_REFRESH_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H) {
                return Some(HitTarget::RefreshHostsButton);
            }
            if in_rect(SP_JOIN_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H) {
                return Some(HitTarget::JoinSelectedButton);
            }
            if in_rect(SP_BACK_X, SP_BTN_Y, SP_BTN_W, SP_BTN_H) {
                return Some(HitTarget::PairingBackButton);
            }
            // Host list rows (idx bounds validated by the controller).
            if y >= HOST_ROW_TOP && y < HOST_ROW_TOP + (HOST_ROWS_MAX as i32) * HOST_ROW_H {
                let idx = ((y - HOST_ROW_TOP) / HOST_ROW_H) as u8;
                return Some(HitTarget::HostListItem { idx });
            }
            None
        }
        IntercomPage::GroupInfo => {
            // Modal masks all other hits when confirming (tasks 3.2 / 3.3).
            if confirming_leave {
                if in_rect(MODAL_CANCEL_X, MODAL_BTN_Y, MODAL_BTN_W, MODAL_BTN_H) {
                    return Some(HitTarget::CancelLeaveButton);
                }
                if in_rect(MODAL_CONFIRM_X, MODAL_BTN_Y, MODAL_BTN_W, MODAL_BTN_H) {
                    return Some(HitTarget::ConfirmLeaveButton);
                }
                return None;
            }
            if in_rect(LEAVE_BTN_X, LEAVE_BTN_Y, LEAVE_BTN_W, LEAVE_BTN_H) {
                return Some(HitTarget::LeaveGroupButton);
            }
            if y >= PTT_TOP {
                return Some(HitTarget::IntercomPttArea);
            }
            None
        }
        // Grouped Main page: only the bottom PTT area is interactive.
        IntercomPage::Main => {
            if y >= PTT_TOP {
                return Some(HitTarget::IntercomPttArea);
            }
            None
        }
        // VoiceChanger page: 3 effect buttons + Cancel (change
        // intercom-voice-changer-preview). Effect buttons are inert while
        // Recording (task 2.2); Cancel shows during Recording/Previewing.
        IntercomPage::VoiceChanger => {
            if !matches!(vc_state, VoiceChangerSubState::Idle)
                && in_rect(VC_CANCEL_X, VC_CANCEL_Y, VC_CANCEL_W, VC_CANCEL_H)
            {
                return Some(HitTarget::CancelVoicePreviewButton);
            }
            if !matches!(vc_state, VoiceChangerSubState::Recording { .. }) {
                for (i, eff) in VC_EFFECTS.iter().enumerate() {
                    let by = VC_BTN_TOP + (i as i32) * (VC_BTN_H + VC_BTN_GAP);
                    if in_rect(VC_BTN_X, by, VC_BTN_W, VC_BTN_H) {
                        return Some(HitTarget::VoiceEffectButton(*eff));
                    }
                }
            }
            if y >= PTT_TOP {
                return Some(HitTarget::IntercomPttArea);
            }
            None
        }
    }
}

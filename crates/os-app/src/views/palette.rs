// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 battlemountain2
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! View painter for the Omni Command Palette modal (`Ctrl+Shift+Space`).
//!
//! Renders a floating, centered, dark card over a dark veil with fuzzy-filtered results,
//! category badges, keyboard shortcut indicators, and an interactive search bar.

use os_ui::{Color, Font, Rect};
use crate::app::{Action, State, Ui};
use crate::palette::{self, PAGE_SIZE};

pub fn paint(ui: &mut Ui, s: &State) {
    if !s.palette.open {
        return;
    }
    // The theme's colours, so the palette sits in the skin it opens in (Mango's black and mango, or Citrus).
    let t = ui.t;
    let (border, bg_card, bg_search, bg_header_line) = (t.menu_drop_hi.shade(0.12), t.menu_drop, t.sunken, t.splitter);
    let (bg_row_sel, bg_row_hov, accent) = (t.accent.mix(t.menu_drop, 0.78), t.menu_drop_hi, t.accent);
    let (text_white, text_muted, text_dim) = (t.text, t.text_dim, t.text_faint);
    let (badge_key_bg, badge_key_text) = (t.button, t.glyph);

    // 1. Fullscreen dark backdrop veil
    let full = Rect::new(0.0, 0.0, s.screen.0, s.screen.1);
    ui.c.fill_rect(full, Color::rgb(0).with_alpha(140));
    // Clicking backdrop closes palette
    ui.hit(full, Action::ToggleCommandPalette, "");

    // 2. Centered floating modal card
    let card_w = 640.0;
    let card_h = 444.0;
    let cx = s.screen.0 / 2.0;
    let cy = (s.screen.1 / 3.0).max(230.0);
    let card_r = Rect::new((cx - card_w / 2.0).round(), (cy - card_h / 2.0).round(), card_w, card_h);

    // Drop shadow
    ui.c.fill_rrect(card_r.outset(6.0), 14.0, Color::rgb(0).with_alpha(90));
    ui.c.fill_rrect(card_r.outset(2.0), 12.0, Color::rgb(0).with_alpha(60));

    // Outer border & card background
    ui.c.fill_rrect(card_r, 10.0, border);
    ui.c.fill_rrect(card_r.inset(1.0), 9.0, bg_card);

    // Swallow clicks inside the card so they don't hit the backdrop
    ui.hit(card_r, Action::Swallow, "");

    // 3. Search Header
    let head_h = 52.0;
    let head_r = Rect::new(card_r.x + 1.0, card_r.y + 1.0, card_r.w - 2.0, head_h);
    ui.c.fill_rrect(head_r, 9.0, bg_search);
    // Square off bottom corners of header
    ui.c.fill_rect(Rect::new(head_r.x, head_r.bottom() - 10.0, head_r.w, 10.0), bg_search);
    // Header divider line
    ui.c.fill_rect(Rect::new(card_r.x, card_r.y + head_h, card_r.w, 1.0), bg_header_line);

    // Search Icon
    let icon_r = Rect::new(card_r.x + 18.0, card_r.y + 18.0, 16.0, 16.0);
    ui.c.icon(&os_ui::tabler::SEARCH, icon_r, accent);

    // Search Input Query / Placeholder
    let font_search = Font::new(15);
    let text_x = card_r.x + 46.0;
    let text_y = card_r.y + 16.0;
    if s.palette.query.is_empty() {
        ui.c.text(font_search, text_x, text_y, text_dim, "Type a command, plugin, window or track name...");
    } else {
        ui.c.text(font_search, text_x, text_y, text_white, &s.palette.query);
        // Blinking / solid cursor bar
        let tw = ui.c.text_width(font_search, &s.palette.query);
        let cur_r = Rect::new(text_x + tw + 2.0, card_r.y + 15.0, 2.0, 21.0);
        ui.c.fill_rect(cur_r, accent);
    }

    // "ESC" badge in top right
    let esc_w = 34.0;
    let esc_r = Rect::new(card_r.right() - esc_w - 16.0, card_r.y + 16.0, esc_w, 20.0);
    ui.c.fill_rrect(esc_r, 4.0, badge_key_bg);
    ui.c.text_center(Font::new(10).bold(), esc_r, text_muted, "ESC");
    ui.hit(esc_r, Action::ToggleCommandPalette, "Dismiss palette (Esc)");

    // 4. Filtered Results List
    let all_items = palette::items(s);
    let filtered = palette::filter_items(&all_items, &s.palette.query);
    let count = filtered.len();

    let list_top = card_r.y + head_h + 1.0;
    let list_h = 356.0;
    let list_r = Rect::new(card_r.x + 1.0, list_top, card_r.w - 2.0, list_h);

    if count == 0 {
        let msg = format!("No matching commands for \"{}\"", s.palette.query);
        let font_empty = Font::new(13);
        let tw = ui.c.text_width(font_empty, &msg);
        let empty_x = (card_r.cx() - tw / 2.0).round();
        let empty_y = list_r.cy() - 10.0;
        ui.c.text(font_empty, empty_x, empty_y, text_dim, &msg);
    } else {
        let row_h = 39.0;
        let start = s.palette.scroll;
        let end = (start + PAGE_SIZE).min(count);

        for (rel_i, item_ref) in filtered[start..end].iter().enumerate() {
            let abs_i = start + rel_i;
            let is_sel = abs_i == s.palette.selected;
            let row_r = Rect::new(card_r.x + 8.0, list_top + 4.0 + rel_i as f32 * row_h, card_w - 16.0, row_h - 2.0);

            // Register hit first to capture hover state
            let hov = ui.hit(row_r, Action::PaletteClick(item_ref.original_index), "Run command");

            // Row background
            if is_sel {
                ui.c.fill_rrect(row_r, 6.0, bg_row_sel);
                // Active indicator pill on left edge
                ui.c.fill_rrect(Rect::new(row_r.x + 1.0, row_r.y + 6.0, 3.0, row_r.h - 12.0), 1.5, accent);
            } else if hov {
                ui.c.fill_rrect(row_r, 6.0, bg_row_hov);
            }

            // Category badge
            let (bg_cat, fg_cat) = item_ref.item.category.colors();
            let badge_w = 60.0;
            let badge_r = Rect::new(row_r.x + 10.0, row_r.y + 8.0, badge_w, 20.0);
            ui.c.fill_rrect(badge_r, 4.0, bg_cat);
            ui.c.text_center(Font::new(10).bold(), badge_r, fg_cat, item_ref.item.category.label());

            // Title
            let font_title = Font::new(13);
            let title_color = if is_sel { text_white } else { text_white.shade(0.08) };
            let title_x = row_r.x + badge_w + 20.0;
            let title_y = row_r.y + 11.0;
            let title_w = ui.c.text_width(font_title, &item_ref.item.title);
            ui.c.text(font_title, title_x, title_y, title_color, &item_ref.item.title);

            // Subtitle
            let max_sub_w = if item_ref.item.shortcut.is_some() { 160.0 } else { 220.0 };
            let sub_x = title_x + title_w + 14.0;
            if sub_x + 30.0 < row_r.right() - max_sub_w {
                ui.c.text_clip(Font::new(11), sub_x, title_y + 1.0, max_sub_w, text_dim, &item_ref.item.subtitle);
            }

            // Shortcut Pill (if present)
            if let Some(sc) = item_ref.item.shortcut {
                let font_sc = Font::new(11);
                let sc_w = (ui.c.text_width(font_sc, sc) + 14.0).max(30.0);
                let sc_r = Rect::new(row_r.right() - sc_w - 10.0, row_r.y + 8.0, sc_w, 20.0);
                ui.c.fill_rrect(sc_r, 4.0, badge_key_bg);
                ui.c.text_center(font_sc, sc_r, badge_key_text, sc);
            }
        }
    }

    // 5. Footer Bar
    let foot_y = card_r.bottom() - 34.0;
    ui.c.fill_rect(Rect::new(card_r.x, foot_y, card_r.w, 1.0), bg_header_line);
    let foot_r = Rect::new(card_r.x + 1.0, foot_y + 1.0, card_r.w - 2.0, 33.0);
    ui.c.fill_rrect(foot_r, 9.0, bg_search);
    // Square off top of footer
    ui.c.fill_rect(Rect::new(foot_r.x, foot_r.y, foot_r.w, 10.0), bg_search);

    // Navigation hint
    ui.c.text(Font::new(11), card_r.x + 16.0, foot_y + 10.0, text_dim, "Up/Down Navigate   ·   Enter Run   ·   Esc Dismiss");

    // Result count
    let count_text = format!("{} results", count);
    let count_w = ui.c.text_width(Font::new(11), &count_text);
    ui.c.text(Font::new(11), card_r.right() - count_w - 16.0, foot_y + 10.0, text_dim, &count_text);
}

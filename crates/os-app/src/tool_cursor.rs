// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The playlist's and piano roll's tool cursors, as FL shows them over their grids: each tool's icon in white on a
//! dark rim (readable on the black grid and on light clips alike), as a cursor image with its hot spot (`App::
//! tool_cursor` picks the tool; the window makes the cursors once it is open).

use crate::icons::Icon;
use os_ui::{Color, Rect};

/// `icon`'s cursor `size` px square: straight (not premultiplied) RGBA rows, and the hot spot.
pub fn image(icon: Icon, size: u32) -> Option<(Vec<u8>, (u16, u16))> {
    let r = frame(size);
    let k = r.w / 24.0;
    // The canvas draws icons onto an opaque ground: the rim and the body are drawn apart, white on black, and their
    // lightness taken as their coverage.
    let mask = |rim: bool| -> Option<Vec<u8>> {
        let mut c = os_ui::Canvas::new(size, size, 1.0).ok()?;
        c.pixmap.fill(tiny_skia::Color::BLACK);
        let white = Color::rgb(0xffffff);
        if rim {
            crate::icons::draw_stroke(&mut c, icon, r, white, 2.0 * k + 3.0);
        }
        crate::icons::draw(&mut c, icon, r, white);
        Some(c.pixmap.pixels().iter().map(|p| p.demultiply().red()).collect())
    };
    let (rim, body) = (mask(true)?, mask(false)?);
    const DARK: u32 = 0x11;
    let rgba = rim.iter().zip(&body).flat_map(|(&a, &b)| {
        // White where the body covers, the dark rim around it.
        let a = a.max(b);
        let v = if a == 0 { 0 } else { ((u32::from(b) * 255 + u32::from(a - b) * DARK) / u32::from(a)) as u8 };
        [v, v, v, a]
    });
    Some((rgba.collect(), hot_spot(icon, size)))
}

/// Where the icon sits in the image: a margin for the rim.
fn frame(size: u32) -> Rect {
    let s = size as f32;
    Rect::new(2.0, 2.0, s - 4.0, s - 4.0)
}

/// The point that acts: the tip of the pencil, brush and knife (their lower left), the middle of the others.
fn hot_spot(icon: Icon, size: u32) -> (u16, u16) {
    let r = frame(size);
    let k = r.w / 24.0;
    let (x, y) = match icon {
        Icon::Pencil | Icon::Brush | Icon::Slice => (r.x + 4.0 * k, r.bottom() - 4.0 * k),
        _ => (r.cx(), r.cy()),
    };
    (x.round().clamp(0.0, f32::from(u16::MAX)) as u16, y.round().clamp(0.0, f32::from(u16::MAX)) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Over the playlist's grid the cursor is the playlist tool's icon (Draw shows the pencil, the Select tool keeps
    /// the arrow), and the arrow away from it (FL).
    #[test]
    fn the_playlist_shows_its_tool() {
        use crate::app::{App, Cursor, Win};
        use crate::playlist::PlTool;
        let Ok(mut c) = os_ui::Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::Playlist];
        app.paint(&mut c, None);
        let g = app.pl_grid().expect("the grid");
        let tools = [
            (PlTool::Draw, Cursor::Tool(Icon::Pencil)),
            (PlTool::Paint, Cursor::Tool(Icon::Brush)),
            (PlTool::Slice, Cursor::Tool(Icon::Slice)),
            (PlTool::Select, Cursor::Default),
        ];
        for (tool, want) in tools {
            app.state.playlist.tool = tool;
            app.mouse_move(g.x + g.w * 0.75, g.y + g.h * 0.75);
            assert_eq!(app.cursor(), want, "{tool:?}");
        }
        app.state.playlist.tool = PlTool::Paint;
        app.mouse_move(5.0, 5.0);
        assert_eq!(app.cursor(), Cursor::Default, "off the grid");
        // Over a clip, the tools that drag clips show the move cursor; the others keep their icon.
        crate::playlist::arr_mut(&mut app.state).clips.push(os_model::Clip::new(0, 384, 0, os_model::ClipSource::Pattern(1)));
        app.paint(&mut c, None);
        let r = crate::playlist::PGeom::new(&app.state, g).clip_rect(&crate::playlist::clips(&app.state)[0]);
        for (tool, want) in [(PlTool::Draw, Cursor::Move), (PlTool::Select, Cursor::Move), (PlTool::Delete, Cursor::Tool(Icon::Delete))] {
            app.state.playlist.tool = tool;
            app.mouse_move(r.cx(), r.cy());
            assert_eq!(app.cursor(), want, "{tool:?} over a clip");
        }
    }

    /// Every tool's cursor has a white body on a dark rim, a clear background and its hot spot inside the image.
    #[test]
    fn tool_cursors_are_drawn() {
        for icon in crate::app::TOOL_CURSORS {
            let (rgba, (hx, hy)) = image(icon, 32).expect("drawn");
            assert_eq!(rgba.len(), 32 * 32 * 4);
            assert!(u32::from(hx) < 32 && u32::from(hy) < 32, "{icon:?}");
            let px = |i: usize| &rgba[i * 4..i * 4 + 4];
            assert_eq!(px(0)[3], 0, "{icon:?}: clear corner");
            assert!((0..32 * 32).any(|i| px(i) == [255, 255, 255, 255]), "{icon:?}: white body");
            assert!((0..32 * 32).any(|i| px(i)[3] == 255 && px(i)[0] < 40), "{icon:?}: dark rim");
        }
    }
}

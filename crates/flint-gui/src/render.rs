//! Draws one frame: the launcher's state, laid out by [`Layout`], in the
//! palette's colours.
//!
//! The query line holds the scope chip (when the query is scoped), the
//! query with its caret, and the mode badge at the right; the caret is a
//! bar in Insert mode and a block in Normal mode, in that mode's colour,
//! as in Fenix. Each row shows its title with matched characters in the
//! highlight colour, its subtitle dimmed, and its provider at the right.
//!
//! Text goes through glyphon as in Fenix, in the same monospace family
//! Fenix picks by default, so the two look alike until M3 follows Fenix's
//! own `editor.font_family`. Loading the system's fonts is the slowest
//! thing here (a few hundred milliseconds on Windows), which is why the
//! renderer is built once when the daemon starts and never again.

use glyphon::{
    Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

use crate::gpu::{Gpu, Surface};
use crate::launcher::{Launcher, Mode};
use crate::layout::{self, Layout, Rect};
use crate::rect::Rects;
use crate::theme::{text_color, Palette};

/// Shown in the empty query line.
const PLACEHOLDER: &str = "Search";
/// Line height as a multiple of the font size, Fenix's ratio.
const LINE: f32 = 1.25;

/// Everything needed to draw, for surfaces of one format.
pub struct Renderer {
    format: wgpu::TextureFormat,
    rects: Rects,
    font_system: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text: TextRenderer,
    family: String,
    chip: Buffer,
    query: Buffer,
    mode: Buffer,
    rows: Vec<(Buffer, Buffer)>,
    empty: Buffer,
    footer: Buffer,
}

fn buffer(font_system: &mut FontSystem, size: f32) -> Buffer {
    let mut b = Buffer::new(font_system, Metrics::new(size, size * LINE));
    b.set_wrap(Wrap::None);
    b
}

/// The width of a shaped one-line buffer.
fn width_of(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
}

/// Splits `title` into runs of matched and unmatched characters, given
/// the matched character positions (ascending).
pub fn spans<'a>(title: &'a str, highlights: &[u32]) -> Vec<(&'a str, bool)> {
    let mut out: Vec<(&'a str, bool)> = Vec::new();
    let mut matched = highlights.iter().copied().peekable();
    let mut start = 0;
    let mut current = None;
    for (n, (byte, _)) in title.char_indices().enumerate() {
        while matched.peek().is_some_and(|&m| (m as usize) < n) {
            matched.next();
        }
        let is_match = matched.peek() == Some(&(n as u32));
        if current != Some(is_match) {
            if let Some(was) = current {
                out.push((&title[start..byte], was));
            }
            start = byte;
            current = Some(is_match);
        }
    }
    if let Some(was) = current {
        out.push((&title[start..], was));
    }
    out
}

impl Renderer {
    /// Builds the pipelines and loads the fonts.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Renderer {
        let mut font_system = FontSystem::new();
        let family = monospace_family(&font_system);
        let cache = Cache::new(&gpu.device);
        let viewport = Viewport::new(&gpu.device, &cache);
        let mut atlas = TextAtlas::new(&gpu.device, &gpu.queue, &cache, format);
        let text = TextRenderer::new(
            &mut atlas,
            &gpu.device,
            wgpu::MultisampleState::default(),
            None,
        );
        let fs = &mut font_system;
        Renderer {
            format,
            rects: Rects::new(&gpu.device, format),
            chip: buffer(fs, layout::FOOTER_FONT),
            query: buffer(fs, layout::QUERY_FONT),
            mode: buffer(fs, layout::FOOTER_FONT),
            empty: buffer(fs, layout::ROW_FONT),
            footer: buffer(fs, layout::FOOTER_FONT),
            rows: Vec::new(),
            font_system,
            swash: SwashCache::new(),
            viewport,
            atlas,
            text,
            family,
        }
    }

    /// Uses `wanted` for all text when it is installed, else the monospace
    /// family Fenix would pick. A name is no proof a font exists (a
    /// settings file from another machine), so it is checked first.
    pub fn set_font(&mut self, wanted: Option<&str>) {
        let db = self.font_system.db();
        let family = wanted
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .filter(|w| {
                db.faces().any(|face| {
                    face.families
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(w))
                })
            })
            .map(str::to_owned);
        self.family = family.unwrap_or_else(|| monospace_family(&self.font_system));
    }

    /// The family text is drawn in.
    pub fn font(&self) -> &str {
        &self.family
    }

    /// The surface format the pipelines were built for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Draws `launcher` and presents it. Returns whether a frame was
    /// presented; `false` means the surface wasn't available and the
    /// caller should try again on the next redraw.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        surface: &mut Surface,
        launcher: &Launcher,
        layout: &Layout,
        scale: f32,
        palette: &Palette,
    ) -> bool {
        let Some(frame) = surface.frame(gpu) else {
            return false;
        };
        let (width, height) = (surface.config.width as f32, surface.config.height as f32);
        let px = |r: Rect| r.scaled(scale);
        let pad = layout::PAD * scale;
        let gap = 8.0 * scale;
        let attrs = Attrs::new().family(Family::Name(&self.family));
        let colored = |c: [f32; 4]| attrs.clone().color(text_color(c));
        let fs = &mut self.font_system;
        let shape =
            |b: &mut Buffer, fs: &mut FontSystem, size: f32, text: &str, color: [f32; 4]| {
                b.set_metrics(Metrics::new(size, size * LINE));
                b.set_size(None, None);
                b.set_text(text, &colored(color), Shaping::Advanced, None);
                b.shape_until_scroll(fs, false);
                width_of(b)
            };

        self.rects.clear();
        let mut areas_spec: Vec<(Which, f32, f32, f32)> = Vec::new(); // (buffer, left, top, right)

        // The query line: chip, query, caret, mode badge.
        let q = px(layout.query);
        rect(&mut self.rects, q, palette.surface, width, height);
        let small = layout::FOOTER_FONT * scale;
        let small_top = q.y + (q.h - small * LINE) / 2.0;
        let (mode_text, mode_color) = match launcher.mode() {
            Mode::Insert => ("INSERT", palette.mode_insert),
            Mode::Normal => ("NORMAL", palette.mode_normal),
        };
        let mode_w = shape(&mut self.mode, fs, small, mode_text, palette.badge_text);
        let badge_pad = 6.0 * scale;
        let mode_left = width - pad - mode_w - 2.0 * badge_pad;
        let badge_rect = Rect {
            x: mode_left,
            y: small_top - 2.0 * scale,
            w: mode_w + 2.0 * badge_pad,
            h: small * LINE + 4.0 * scale,
        };
        rect(&mut self.rects, badge_rect, mode_color, width, height);
        areas_spec.push((Which::Mode, mode_left + badge_pad, small_top, width - pad));

        let mut query_left = pad;
        if let Some(chip) = launcher.scope() {
            let chip_w = shape(&mut self.chip, fs, small, &chip.label, palette.badge_text);
            let chip_rect = Rect {
                x: pad,
                y: small_top - 2.0 * scale,
                w: chip_w + 2.0 * badge_pad,
                h: small * LINE + 4.0 * scale,
            };
            rect(&mut self.rects, chip_rect, palette.chip, width, height);
            areas_spec.push((Which::Chip, pad + badge_pad, small_top, pad + chip_rect.w));
            query_left = pad + chip_rect.w + gap;
        }
        let query_size = layout::QUERY_FONT * scale;
        let (text, color) = if launcher.query().is_empty() {
            (PLACEHOLDER, palette.muted)
        } else {
            (launcher.query(), palette.foreground)
        };
        let shaped = shape(&mut self.query, fs, query_size, text, color);
        let typed = if launcher.query().is_empty() {
            0.0
        } else {
            shaped
        };
        let query_top = q.y + (q.h - query_size * LINE) / 2.0;
        areas_spec.push((Which::Query, query_left, query_top, mode_left - gap));
        let caret = match launcher.mode() {
            Mode::Insert => Rect {
                x: query_left + typed,
                y: query_top + query_size * 0.1,
                w: (2.0 * scale).round().max(1.0),
                h: query_size * 1.05,
            },
            Mode::Normal => Rect {
                x: query_left + typed,
                y: query_top + query_size * 0.1,
                w: query_size * 0.6,
                h: query_size * 1.05,
            },
        };
        let mut caret_color = match launcher.mode() {
            Mode::Insert => palette.mode_insert,
            Mode::Normal => palette.mode_normal,
        };
        if launcher.mode() == Mode::Normal {
            caret_color[3] = 0.5;
        }
        rect(&mut self.rects, caret, caret_color, width, height);

        // Rows: the results, or the action menu or which-key in their place.
        let view = launcher.view();
        let row_size = layout::ROW_FONT * scale;
        let visible = view.rows.len().saturating_sub(view.top).min(layout.rows);
        if view.overlay {
            // A tint over the list area says "this isn't the results".
            let first = px(layout.row(0));
            let list = Rect {
                h: px(layout.footer).y - first.y,
                ..first
            };
            let mut tint = palette.surface;
            tint[3] = 0.6;
            rect(&mut self.rects, list, tint, width, height);
        }
        while self.rows.len() < visible {
            self.rows.push((
                buffer(fs, layout::ROW_FONT),
                buffer(fs, layout::FOOTER_FONT),
            ));
        }
        if let Some(selected) = view.selected.filter(|_| visible > 0) {
            let selected = selected.saturating_sub(view.top);
            if selected < visible {
                rect(
                    &mut self.rects,
                    px(layout.row(selected)),
                    palette.selection,
                    width,
                    height,
                );
            }
        }
        for i in 0..visible {
            let row = &view.rows[view.top + i];
            let r = px(layout.row(i));
            let (title_buffer, badge_buffer) = &mut self.rows[i];
            // In the menu the badge is the action's key, which is pressed.
            let badge_color = if view.overlay {
                palette.accent
            } else {
                palette.muted
            };
            let badge_w = shape(badge_buffer, fs, small, &row.badge, badge_color);
            let badge_left = width - pad - badge_w;
            areas_spec.push((
                Which::Badge(i),
                badge_left,
                r.y + (r.h - small * LINE) / 2.0,
                width - pad,
            ));

            let mut rich: Vec<(&str, Attrs)> = spans(&row.title, &row.highlights)
                .into_iter()
                .map(|(text, hit)| {
                    (
                        text,
                        colored(if hit {
                            palette.accent
                        } else if row.dimmed {
                            palette.muted
                        } else {
                            palette.foreground
                        }),
                    )
                })
                .collect();
            if !row.subtitle.is_empty() {
                rich.push(("   ", colored(palette.muted)));
                rich.push((row.subtitle.as_str(), colored(palette.muted)));
            }
            title_buffer.set_metrics(Metrics::new(row_size, row_size * LINE));
            title_buffer.set_size(None, None);
            title_buffer.set_rich_text(rich, &attrs, Shaping::Advanced, None);
            title_buffer.shape_until_scroll(fs, false);
            areas_spec.push((
                Which::Row(i),
                pad,
                r.y + (r.h - row_size * LINE) / 2.0,
                badge_left - gap,
            ));
        }
        if !view.overlay
            && view.rows.is_empty()
            && (!launcher.query().is_empty() || launcher.scope().is_some())
        {
            let r = px(layout.row(0));
            shape(
                &mut self.empty,
                fs,
                row_size,
                "Nothing matches",
                palette.muted,
            );
            areas_spec.push((
                Which::Empty,
                pad,
                r.y + (r.h - row_size * LINE) / 2.0,
                width - pad,
            ));
        }

        // Footer.
        let f = px(layout.footer);
        rect(&mut self.rects, f, palette.surface, width, height);
        let line = launcher.footer();
        let footer_color = if line.error {
            palette.error
        } else {
            palette.muted
        };
        shape(&mut self.footer, fs, small, &line.text, footer_color);
        areas_spec.push((
            Which::Footer,
            pad,
            f.y + (f.h - small * LINE) / 2.0,
            width - pad,
        ));

        // The window's edge, over everything.
        let border = (layout::BORDER * scale).max(1.0);
        for edge in [
            Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: border,
            },
            Rect {
                x: 0.0,
                y: height - border,
                w: width,
                h: border,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                w: border,
                h: height,
            },
            Rect {
                x: width - border,
                y: 0.0,
                w: border,
                h: height,
            },
        ] {
            rect(&mut self.rects, edge, palette.border, width, height);
        }

        let areas = areas_spec.into_iter().map(|(which, left, top, right)| {
            let buffer = match which {
                Which::Chip => &self.chip,
                Which::Query => &self.query,
                Which::Mode => &self.mode,
                Which::Row(i) => &self.rows[i].0,
                Which::Badge(i) => &self.rows[i].1,
                Which::Empty => &self.empty,
                Which::Footer => &self.footer,
            };
            TextArea {
                buffer,
                left,
                top,
                scale: 1.0,
                bounds: TextBounds {
                    left: left as i32,
                    top: 0,
                    right: right.max(left) as i32,
                    bottom: height as i32,
                },
                default_color: text_color(palette.foreground),
                custom_glyphs: &[],
            }
        });

        self.viewport.update(
            &gpu.queue,
            Resolution {
                width: surface.config.width,
                height: surface.config.height,
            },
        );
        if self
            .text
            .prepare(
                &gpu.device,
                &gpu.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash,
            )
            .is_err()
        {
            // The atlas is full: drop what this frame doesn't use; a frame
            // without some text is better than none.
            self.atlas.trim();
        }
        self.rects.upload(&gpu.device, &gpu.queue);

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("flint-frame"),
            });
        {
            let [r, g, b, a] = palette.background.map(f64::from);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flint-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rects.draw(&mut pass);
            let _ = self.text.render(&self.atlas, &self.viewport, &mut pass);
        }
        gpu.queue.submit(Some(encoder.finish()));
        gpu.queue.present(frame);
        self.atlas.trim();
        true
    }
}

/// Which buffer a text area draws.
#[derive(Clone, Copy)]
enum Which {
    Chip,
    Query,
    Mode,
    Row(usize),
    Badge(usize),
    Empty,
    Footer,
}

fn rect(rects: &mut Rects, r: Rect, color: [f32; 4], width: f32, height: f32) {
    rects.push(r, color, width, height);
}

/// The monospace family Fenix would pick: the system's own, then the
/// common ones by name, whichever is installed. A name is no proof a font
/// exists, so each is checked in the font database.
fn monospace_family(font_system: &FontSystem) -> String {
    let db = font_system.db();
    let installed = |family: &str| {
        db.faces()
            .any(|face| face.monospaced && face.families.iter().any(|(name, _)| name == family))
    };
    for family in [
        db.family_name(&Family::Monospace),
        "Cascadia Mono",
        "Consolas",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Courier New",
    ] {
        if installed(family) {
            return family.to_owned();
        }
    }
    db.family_name(&Family::Monospace).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_split_matched_runs() {
        assert_eq!(
            spans("Firefox", &[0, 1, 4]),
            [("Fi", true), ("re", false), ("f", true), ("ox", false)]
        );
        assert_eq!(spans("abc", &[]), [("abc", false)]);
        assert_eq!(spans("", &[0]), Vec::<(&str, bool)>::new());
        assert_eq!(
            spans("éa", &[1]),
            [("é", false), ("a", true)],
            "positions are characters, not bytes"
        );
        assert_eq!(
            spans("ab", &[5]),
            [("ab", false)],
            "positions past the end are ignored"
        );
    }
}

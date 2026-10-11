//! Draws one frame: the launcher's state, laid out by [`Layout`], in the
//! palette's colours.
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
use crate::launcher::Launcher;
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
    query: Buffer,
    rows: Vec<Buffer>,
    footer: Buffer,
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
        let buffer = |font_system: &mut FontSystem, size: f32| {
            let mut b = Buffer::new(font_system, Metrics::new(size, size * LINE));
            b.set_wrap(Wrap::None);
            b
        };
        let query = buffer(&mut font_system, layout::QUERY_FONT);
        let footer = buffer(&mut font_system, layout::FOOTER_FONT);
        Renderer {
            format,
            rects: Rects::new(&gpu.device, format),
            font_system,
            swash: SwashCache::new(),
            viewport,
            atlas,
            text,
            family,
            query,
            rows: Vec::new(),
            footer,
        }
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

        // Rectangles.
        self.rects.clear();
        let mut rect = |r: Rect, color| self.rects.push(r, color, width, height);
        rect(px(layout.query), palette.surface);
        rect(px(layout.footer), palette.surface);
        if !launcher.rows().is_empty() {
            rect(
                px(layout.row(launcher.selected().min(layout.rows - 1))),
                palette.selection,
            );
        }
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
            rect(edge, palette.border);
        }

        // Text.
        let pad = layout::PAD * scale;
        let inner = (width - 2.0 * pad).max(1.0);
        let attrs = Attrs::new().family(Family::Name(&self.family));
        let fs = &mut self.font_system;

        let query_size = layout::QUERY_FONT * scale;
        self.query
            .set_metrics(Metrics::new(query_size, query_size * LINE));
        self.query.set_size(Some(inner), None);
        let (text, color) = if launcher.query().is_empty() {
            (PLACEHOLDER, palette.muted)
        } else {
            (launcher.query(), palette.foreground)
        };
        self.query.set_text(
            text,
            &attrs.clone().color(text_color(color)),
            Shaping::Advanced,
            None,
        );
        self.query.shape_until_scroll(fs, false);
        let query_top = px(layout.query).y + (px(layout.query).h - query_size * LINE) / 2.0;
        let typed_width = if launcher.query().is_empty() {
            0.0
        } else {
            self.query
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0, f32::max)
        };
        // The caret, after what was typed.
        self.rects.push(
            Rect {
                x: pad + typed_width,
                y: query_top + query_size * 0.1,
                w: (2.0 * scale).round().max(1.0),
                h: query_size * 1.05,
            },
            palette.accent,
            width,
            height,
        );

        let row_size = layout::ROW_FONT * scale;
        let shown = launcher.rows().len().min(layout.rows);
        while self.rows.len() < shown {
            let mut b = Buffer::new(fs, Metrics::new(row_size, row_size * LINE));
            b.set_wrap(Wrap::None);
            self.rows.push(b);
        }
        for (buffer, row) in self.rows.iter_mut().zip(launcher.rows()) {
            buffer.set_metrics(Metrics::new(row_size, row_size * LINE));
            buffer.set_size(Some(inner), None);
            let title = attrs.clone().color(text_color(palette.foreground));
            let subtitle = attrs.clone().color(text_color(palette.muted));
            buffer.set_rich_text(
                [
                    (row.title.as_str(), title),
                    ("   ", subtitle.clone()),
                    (row.subtitle.as_str(), subtitle),
                ],
                &attrs,
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(fs, false);
        }

        let footer_size = layout::FOOTER_FONT * scale;
        let line = launcher.footer();
        self.footer
            .set_metrics(Metrics::new(footer_size, footer_size * LINE));
        self.footer.set_size(Some(inner), None);
        let footer_color = if line.error {
            palette.error
        } else {
            palette.muted
        };
        self.footer.set_text(
            &line.text,
            &attrs.clone().color(text_color(footer_color)),
            Shaping::Advanced,
            None,
        );
        self.footer.shape_until_scroll(fs, false);

        let bounds = TextBounds {
            left: pad as i32,
            top: 0,
            right: (width - pad) as i32,
            bottom: height as i32,
        };
        let area = |buffer, top: f32, color| TextArea {
            buffer,
            left: pad,
            top,
            scale: 1.0,
            bounds,
            default_color: text_color(color),
            custom_glyphs: &[],
        };
        let mut areas = vec![area(&self.query, query_top, palette.foreground)];
        for (i, buffer) in self.rows.iter().take(shown).enumerate() {
            let r = px(layout.row(i));
            areas.push(area(
                buffer,
                r.y + (r.h - row_size * LINE) / 2.0,
                palette.foreground,
            ));
        }
        let f = px(layout.footer);
        areas.push(area(
            &self.footer,
            f.y + (f.h - footer_size * LINE) / 2.0,
            palette.muted,
        ));

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
                fs,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash,
            )
            .is_err()
        {
            // The atlas is full: drop what this frame doesn't use and try
            // once more; a frame without text is better than none.
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

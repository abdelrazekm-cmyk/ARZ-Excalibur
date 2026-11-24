//! Real Clip-Frame + 60-FPS Visual Effect Preview Engine (`PreviewFrameBank` + `draw_effect_preview`).
//! Captures the actual video frame at Premiere Pro's playhead (`exportFramePNG`) and precomputes
//! real pixel-filtered variants (Gaussian Blur, Lumetri Grade) so hovering/selecting any card
//! previews the effect directly on the editor's real timeline footage at 60 FPS!

use crate::catalog::ActionItem;
use crate::search::CommandCategory;
use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Stroke, TextureHandle, Vec2};
use image::{DynamicImage, Rgba, RgbaImage};
use std::path::Path;
use std::time::SystemTime;

const NOIR_BG: Color32 = Color32::from_rgb(19, 15, 11);
const BRASS: Color32 = Color32::from_rgb(219, 155, 62);
const BRASS_BRIGHT: Color32 = Color32::from_rgb(234, 173, 84);
const IVORY: Color32 = Color32::from_rgb(244, 238, 230);

pub struct PreviewFrameBank {
    pub raw_tex: TextureHandle,
    pub blurred_tex: TextureHandle,
    pub graded_tex: TextureHandle,
    pub last_mtime: Option<SystemTime>,
}

impl PreviewFrameBank {
    pub fn fallback(ctx: &egui::Context) -> Self {
        // Generate a tasteful cinematic dark-amber studio reference frame (240x135) when no Premiere sequence is open
        let (w, h) = (240u32, 135u32);
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 / w as f32) - 0.5;
                let ny = (y as f32 / h as f32) - 0.5;
                let dist = (nx * nx + ny * ny).sqrt();
                let vignette = (1.0 - dist * 1.25).clamp(0.18, 1.0);
                // Horizon glow + subject silhouette + grid lines
                let horizon = (1.0 - ((ny - 0.06).abs() * 7.0)).clamp(0.0, 1.0);
                let subject = if (nx * nx * 2.2 + (ny + 0.02) * (ny + 0.02) * 1.4) < 0.035 {
                    1.35
                } else {
                    1.0
                };
                let r = ((46.0 + 135.0 * horizon) * vignette * subject).clamp(0.0, 255.0) as u8;
                let g = ((35.0 + 95.0 * horizon) * vignette * subject).clamp(0.0, 255.0) as u8;
                let b = ((26.0 + 48.0 * horizon) * vignette * subject).clamp(0.0, 255.0) as u8;
                img.put_pixel(x, y, Rgba([r, g, b, 255]));
            }
        }
        Self::from_rgba_image(ctx, img, None)
    }

    pub fn try_load_from_disk(ctx: &egui::Context, path: &Path, prev_mtime: Option<SystemTime>) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta.modified().ok();
        if mtime.is_some() && mtime == prev_mtime {
            return None;
        }
        let dyn_img = image::open(path).ok()?;
        let thumb = dyn_img.thumbnail(256, 144).to_rgba8();
        if thumb.width() < 8 || thumb.height() < 8 {
            return None;
        }
        Some(Self::from_rgba_image(ctx, thumb, mtime))
    }

    fn from_rgba_image(ctx: &egui::Context, base: RgbaImage, mtime: Option<SystemTime>) -> Self {
        let dyn_base = DynamicImage::ImageRgba8(base.clone());
        let blurred = dyn_base.blur(5.2).to_rgba8();

        // Warm high-contrast Noir & Brass cinema grade
        let mut graded = dyn_base.adjust_contrast(26.0).to_rgba8();
        for px in graded.pixels_mut() {
            px[0] = (px[0] as f32 * 1.14).min(255.0) as u8;
            px[1] = (px[1] as f32 * 1.03).min(255.0) as u8;
            px[2] = (px[2] as f32 * 0.84) as u8;
        }

        let to_tex = |name: &str, im: &RgbaImage| {
            let sz = [im.width() as usize, im.height() as usize];
            let ci = egui::ColorImage::from_rgba_unmultiplied(sz, im.as_raw());
            ctx.load_texture(name.to_string(), ci, egui::TextureOptions::LINEAR)
        };

        Self {
            raw_tex: to_tex("preview_raw", &base),
            blurred_tex: to_tex("preview_blur", &blurred),
            graded_tex: to_tex("preview_grade", &graded),
            last_mtime: mtime,
        }
    }
}

fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0))
}

pub fn draw_effect_preview(
    painter: &egui::Painter,
    rect: Rect,
    item: &ActionItem,
    bank: &PreviewFrameBank,
    is_selected: bool,
    time: f64,
) {
    let p = painter.with_clip_rect(rect);
    p.rect_filled(rect, Rounding::same(5.0), NOIR_BG);

    let t = if is_selected {
        ((time * 1.1).fract()) as f32
    } else {
        0.45_f32
    };
    let wave = (t * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1 smooth oscillation
    let name_low = item.entry.name.to_lowercase();

    // Audio items show dynamic multi-band waveform over dimmed clip backdrop
    if matches!(
        item.entry.category,
        CommandCategory::AudioEffect | CommandCategory::AudioPreset | CommandCategory::AudioTransition
    ) || name_low.contains("audio")
        || name_low.contains("volume")
        || name_low.contains("denoise")
        || name_low.contains("limiter")
    {
        p.image(
            bank.raw_tex.id(),
            rect,
            full_uv(),
            Color32::from_rgba_unmultiplied(255, 255, 255, 55),
        );
        let bars = 15;
        let inner = rect.shrink2(Vec2::new(16.0, 12.0));
        let step = inner.width() / (bars as f32);
        for b in 0..bars {
            let ph = (time as f32 * (if is_selected { 8.0 } else { 0.0 })) + (b as f32 * 0.65);
            let amp = 0.22 + 0.72 * ph.sin().abs();
            let bh = (inner.height() * amp).max(4.0);
            let bx = inner.left() + (b as f32 + 0.5) * step;
            let bar_r = Rect::from_center_size(Pos2::new(bx, inner.center().y), Vec2::new(5.0, bh));
            p.rect_filled(
                bar_r,
                Rounding::same(2.0),
                if is_selected { BRASS_BRIGHT } else { BRASS },
            );
        }
        return;
    }

    // 1. Warp Stabilizer: shaky camera input ghost vs rock-steady stabilized real video frame + tracking crosshairs!
    if name_low.contains("warp") || name_low.contains("stabiliz") {
        let shake_x = if is_selected { (time as f32 * 19.0).sin() * 6.5 } else { 4.0 };
        let shake_y = if is_selected { (time as f32 * 25.0).cos() * 4.5 } else { -3.0 };
        // Unstabilized shaky ghost frame behind
        p.image(
            bank.raw_tex.id(),
            rect.shrink(3.0).translate(Vec2::new(shake_x, shake_y)),
            full_uv(),
            Color32::from_rgba_unmultiplied(255, 120, 80, 75),
        );
        // Locked rock-steady stabilized frame in front (slightly cropped in, just like Warp Stabilizer auto-scale!)
        let steady = rect.shrink2(Vec2::new(14.0, 9.0));
        p.image(
            bank.raw_tex.id(),
            steady,
            Rect::from_min_max(Pos2::new(0.08, 0.08), Pos2::new(0.92, 0.92)),
            Color32::WHITE,
        );
        p.rect_stroke(
            steady,
            Rounding::same(3.0),
            Stroke::new(1.5_f32, if is_selected { Color32::from_rgb(95, 225, 145) } else { BRASS }),
        );
        // Optical tracking crosshairs (`+`) locked onto stabilized frame
        for (ux, uy) in [(0.28, 0.35), (0.52, 0.50), (0.74, 0.40)] {
            let pt = Pos2::new(
                steady.left() + steady.width() * ux,
                steady.top() + steady.height() * uy,
            );
            let col = if is_selected { Color32::from_rgb(95, 225, 145) } else { BRASS_BRIGHT };
            p.line_segment([Pos2::new(pt.x - 4.0, pt.y), Pos2::new(pt.x + 4.0, pt.y)], Stroke::new(1.2_f32, col));
            p.line_segment([Pos2::new(pt.x, pt.y - 4.0), Pos2::new(pt.x, pt.y + 4.0)], Stroke::new(1.2_f32, col));
        }
        return;
    }

    // 2. Gaussian Blur: real Gaussian-blurred clip texture cross-fading / wiping over raw clip!
    if name_low.contains("blur") {
        p.image(bank.raw_tex.id(), rect, full_uv(), Color32::WHITE);
        let alpha = if is_selected {
            (60.0 + 195.0 * wave) as u8
        } else {
            185
        };
        p.image(
            bank.blurred_tex.id(),
            rect,
            full_uv(),
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
        );
        return;
    }

    // 3. Lumetri Color / Color Grade: real graded clip texture with animated split-screen wipe!
    if name_low.contains("lumetri") || name_low.contains("color") {
        p.image(bank.raw_tex.id(), rect, full_uv(), Color32::WHITE);
        let split_u = if is_selected { 0.15 + 0.70 * wave } else { 0.52 };
        let split_x = rect.left() + rect.width() * split_u;
        let graded_rect = Rect::from_min_max(Pos2::new(split_x, rect.top()), rect.max);
        let graded_uv = Rect::from_min_max(Pos2::new(split_u, 0.0), Pos2::new(1.0, 1.0));
        p.image(bank.graded_tex.id(), graded_rect, graded_uv, Color32::WHITE);
        p.line_segment(
            [Pos2::new(split_x, rect.top()), Pos2::new(split_x, rect.bottom())],
            Stroke::new(1.8_f32, IVORY),
        );
        return;
    }

    // 4. Scale / Transform / Zoom / Punch-In: real smooth UV zoom into the actual clip frame!
    if name_low.contains("scale")
        || name_low.contains("transform")
        || name_low.contains("zoom")
        || name_low.contains("punch")
    {
        let inset = if is_selected { 0.22 * wave } else { 0.12 };
        let uv = Rect::from_min_max(Pos2::new(inset, inset), Pos2::new(1.0 - inset, 1.0 - inset));
        p.image(bank.raw_tex.id(), rect, uv, Color32::WHITE);
        return;
    }

    // 5. Position / Move Clip: smooth horizontal slide of the real clip frame
    if name_low.contains("position") || name_low.contains("move clip") {
        let dx = if is_selected { (wave - 0.5) * 32.0 } else { 10.0 };
        let sub = rect.shrink2(Vec2::new(18.0, 8.0)).translate(Vec2::new(dx, 0.0));
        p.image(bank.raw_tex.id(), sub, full_uv(), Color32::WHITE);
        p.rect_stroke(sub, Rounding::same(2.0), Stroke::new(1.2_f32, BRASS));
        return;
    }

    // 6. Drop Shadow: offset soft shadow behind scaled clip frame
    if name_low.contains("shadow") {
        let sub = rect.shrink2(Vec2::new(20.0, 10.0));
        let off = if is_selected { 4.0 + 7.0 * wave } else { 7.0 };
        p.rect_filled(
            sub.translate(Vec2::new(off, off)),
            Rounding::same(3.0),
            Color32::from_rgba_unmultiplied(0, 0, 0, 195),
        );
        p.image(bank.raw_tex.id(), sub, full_uv(), Color32::WHITE);
        p.rect_stroke(sub, Rounding::same(2.0), Stroke::new(1.0_f32, BRASS));
        return;
    }

    // 7. Crop / Frame Size: cinematic letterbox & pillarbox bars closing onto the real clip!
    if name_low.contains("crop") || name_low.contains("frame size") {
        p.image(bank.raw_tex.id(), rect, full_uv(), Color32::WHITE);
        let bar = if is_selected { 4.0 + 16.0 * wave } else { 11.0 };
        p.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + bar)),
            Rounding::ZERO,
            Color32::BLACK,
        );
        p.rect_filled(
            Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - bar), rect.max),
            Rounding::ZERO,
            Color32::BLACK,
        );
        return;
    }

    // 8. Opacity / Video Transitions (Cross Dissolve, Dip to Black, Dip to White, Film Dissolve)
    if name_low.contains("opacity") || matches!(item.entry.category, CommandCategory::VideoTransition) {
        if name_low.contains("white") {
            p.image(bank.raw_tex.id(), rect, full_uv(), Color32::WHITE);
            let flash = ((1.0 - (t - 0.5).abs() * 2.0).max(0.0) * 235.0) as u8;
            p.rect_filled(rect, Rounding::same(4.0), Color32::from_rgba_unmultiplied(255, 255, 255, flash));
        } else if name_low.contains("black") || name_low.contains("opacity") {
            let vis = if is_selected { (45.0 + 210.0 * wave) as u8 } else { 165 };
            p.image(
                bank.raw_tex.id(),
                rect,
                full_uv(),
                Color32::from_rgba_unmultiplied(255, 255, 255, vis),
            );
        } else {
            // Cross Dissolve / Film Dissolve between raw shot A and graded/zoomed shot B!
            p.image(bank.raw_tex.id(), rect, full_uv(), Color32::WHITE);
            let b_alpha = (wave * 235.0) as u8;
            p.image(
                bank.graded_tex.id(),
                rect,
                Rect::from_min_max(Pos2::new(0.10, 0.10), Pos2::new(0.90, 0.90)),
                Color32::from_rgba_unmultiplied(255, 255, 255, b_alpha),
            );
        }
        return;
    }

    // 9. Default Timeline / Trim / Marker / Nest / Preset view over the real clip frame
    p.image(
        bank.raw_tex.id(),
        rect,
        full_uv(),
        Color32::from_rgba_unmultiplied(255, 255, 255, 215),
    );
    let px = rect.left() + rect.width() * (0.12 + 0.76 * t);
    p.line_segment(
        [Pos2::new(px, rect.top()), Pos2::new(px, rect.bottom())],
        Stroke::new(2.0_f32, if is_selected { BRASS_BRIGHT } else { IVORY }),
    );
}

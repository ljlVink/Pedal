use crate::appearance::theme;
use crate::task::{Poll, Pollable, Task};
use eframe::egui;
use image::ImageDecoder;
use rust_i18n::t;
use std::io::Cursor;
use std::time::Duration;

const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SIDE: u32 = 8192;

enum ImageFailure {
    Invalid(String),
    TooLarge,
    TextureLimit,
    Disconnected,
}

tabookit::class! {
    pub(super) struct ImagePreview {
        task: Option<Task<Result<egui::ColorImage, ImageFailure>>>,
        texture: Option<egui::TextureHandle>,
        error: Option<ImageFailure>,
        fit: bool,
        zoom: f32,
    }

    pub fn new(bytes: &[u8]) -> Self {
        let (task, error) = if bytes.len() > MAX_INPUT_BYTES {
            (None, Some(ImageFailure::TooLarge))
        } else {
            let bytes = bytes.to_vec();
            (Some(Task::spawn(move || decode(&bytes))), None)
        };
        Self { task, texture: None, error, fit: true, zoom: 1.0 }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.task else { return; };
        match task.poll() {
            Poll::Ready(result) => {
                self.task = None;
                match result {
                    Ok(pixels) => {
                        let limit = ctx.input(|input| input.max_texture_side);
                        if pixels.size.iter().any(|side| *side > limit) {
                            self.error = Some(ImageFailure::TextureLimit);
                        } else {
                            self.texture = Some(ctx.load_texture("image-preview", pixels, egui::TextureOptions::LINEAR));
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Poll::Pending => ctx.request_repaint_after(Duration::from_millis(30)),
            Poll::Lost => {
                self.task = None;
                self.error = Some(ImageFailure::Disconnected);
            }
        }
    }

    pub fn controls(&mut self, ui: &mut egui::Ui) {
        if ui.selectable_label(self.fit, t!("editor.image_fit")).clicked() {
            self.fit = true;
        }
        if ui.selectable_label(!self.fit && self.zoom == 1.0, t!("editor.image_actual_size")).clicked() {
            self.fit = false;
            self.zoom = 1.0;
        }
        if ui.add(egui::Slider::new(&mut self.zoom, 0.1..=8.0)
            .logarithmic(true).suffix("×").text(t!("editor.image_zoom"))).changed() {
            self.fit = false;
        }
        if let Some(texture) = &self.texture {
            ui.label(t!("editor.image_dimensions", width = texture.size()[0], height = texture.size()[1]));
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            let message = match error {
                ImageFailure::Invalid(error) => t!("editor.image_failed", error = error),
                ImageFailure::TooLarge => t!("editor.image_too_large"),
                ImageFailure::TextureLimit => t!("editor.image_texture_limit"),
                ImageFailure::Disconnected => t!("editor.image_task_failed"),
            };
            ui.label(message);
            return;
        }
        let Some(texture) = &self.texture else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t!("editor.image_loading"));
            });
            return;
        };
        let available = ui.available_size();
        let natural = texture.size_vec2();
        let scale = if self.fit {
            (available.x / natural.x).min(available.y / natural.y).clamp(0.01, 1.0)
        } else {
            self.zoom
        };
        let size = natural * scale;
        egui::ScrollArea::both().id_salt("image_content").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            paint_background(ui, rect);
            egui::Image::new((texture.id(), size)).paint_at(ui, rect);
        });
    }
}

fn decode(bytes: &[u8]) -> Result<egui::ColorImage, ImageFailure> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ImageFailure::TooLarge);
    }
    let format = image::guess_format(bytes).map_err(image_error)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) * 4 > MAX_DECODED_BYTES {
        return Err(ImageFailure::TooLarge);
    }
    let pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(image_error)?
        .into_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        pixels.as_raw(),
    ))
}

fn image_error(error: image::ImageError) -> ImageFailure {
    match error {
        image::ImageError::Limits(_) => ImageFailure::TooLarge,
        error => ImageFailure::Invalid(error.to_string()),
    }
}

fn paint_background(ui: &egui::Ui, rect: egui::Rect) {
    let visible = rect.intersect(ui.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let palette = theme::palette();
    ui.painter().rect_filled(visible, 0.0, palette.bg_medium);
    let tile = 12.0;
    let first_x = ((visible.left() - rect.left()) / tile).floor() as i32;
    let last_x = ((visible.right() - rect.left()) / tile).ceil() as i32;
    let first_y = ((visible.top() - rect.top()) / tile).floor() as i32;
    let last_y = ((visible.bottom() - rect.top()) / tile).ceil() as i32;
    for y in first_y..last_y {
        for x in first_x..last_x {
            if (x + y) % 2 == 0 {
                let cell = egui::Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * tile, y as f32 * tile),
                    egui::vec2(tile, tile),
                );
                ui.painter()
                    .rect_filled(cell.intersect(visible), 0.0, palette.bg_dark);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let mut out = Cursor::new(Vec::new());
        pixels.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_decoding_preserves_dimensions_color_and_transparency() {
        let pixels = decode(&png()).ok().unwrap();
        assert_eq!(pixels.size, [2, 1]);
        assert_eq!(pixels.pixels[0].to_array(), [255, 0, 0, 255]);
        assert_eq!(pixels.pixels[1].a(), 0);
        assert!(decode(b"invalid PNG").is_err());
    }

    #[test]
    fn webp_decoding_uses_signature_even_with_png_filename() {
        let pixels = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 50, 90, 255]));
        let mut encoded = Cursor::new(Vec::new());
        pixels
            .write_to(&mut encoded, image::ImageFormat::WebP)
            .unwrap();
        let decoded = decode(encoded.get_ref()).ok().unwrap();
        assert_eq!(decoded.size, [3, 2]);
        assert_eq!(decoded.pixels[0].to_array(), [10, 50, 90, 255]);
    }

    #[test]
    fn oversized_dimensions_are_rejected_before_pixel_allocation() {
        let pixels = image::RgbaImage::new(MAX_SIDE + 1, 1);
        let mut encoded = Cursor::new(Vec::new());
        pixels
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(matches!(
            decode(encoded.get_ref()),
            Err(ImageFailure::TooLarge)
        ));
    }

    #[test]
    fn preview_task_reaches_ready_or_error_and_reuses_texture() {
        let ctx = egui::Context::default();
        for bytes in [png(), b"bad PNG".to_vec()] {
            let valid = bytes.starts_with(b"\x89PNG");
            let mut preview = ImagePreview::new(&bytes);
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while preview.task.is_some() {
                preview.poll(&ctx);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(preview.texture.is_some(), valid);
            assert_eq!(preview.error.is_some(), !valid);
            let texture_id = preview.texture.as_ref().map(egui::TextureHandle::id);
            preview.poll(&ctx);
            assert_eq!(
                preview.texture.as_ref().map(egui::TextureHandle::id),
                texture_id
            );
        }
    }
}

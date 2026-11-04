use crate::core::image::AstroImage;
use crate::processing::curves::{CurvesChannel, CurvesSet};
use crate::processing::plate_solve::{AnnotationConfig, WcsSolution};
use crate::processing::stf::StfParameters;
use crate::telescope::coordinates::{format_dec, format_ra};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropAspectRatio {
    Free,
    Square1x1,
    Wide16x9,
    Standard4x3,
    Photo3x2,
}

impl CropAspectRatio {
    pub fn ratio(&self) -> Option<f32> {
        match self {
            Self::Free => None,
            Self::Square1x1 => Some(1.0),
            Self::Wide16x9 => Some(16.0 / 9.0),
            Self::Standard4x3 => Some(4.0 / 3.0),
            Self::Photo3x2 => Some(3.0 / 2.0),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Free => crate::i18n::tr("Vapaa"),
            Self::Square1x1 => crate::i18n::tr("1:1 Neliö"),
            Self::Wide16x9 => crate::i18n::tr("16:9 Laaja"),
            Self::Standard4x3 => crate::i18n::tr("4:3 Standardi"),
            Self::Photo3x2 => crate::i18n::tr("3:2 Kuva"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropHandle {
    Inside,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
    NewBox,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropAction {
    None,
    Apply,
    Cancel,
    Reset,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManualCropState {
    pub is_active: bool,
    /// Normalized coordinates in [0.0, 1.0] relative to image
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub aspect_ratio: CropAspectRatio,
    pub is_dragging: bool,
    pub drag_handle: Option<CropHandle>,
    pub drag_start_norm: Option<Pos2>,
    pub initial_crop: Option<[f32; 4]>,
}

impl Default for ManualCropState {
    fn default() -> Self {
        Self {
            is_active: false,
            x: 0.05,
            y: 0.05,
            w: 0.90,
            h: 0.90,
            aspect_ratio: CropAspectRatio::Free,
            is_dragging: false,
            drag_handle: None,
            drag_start_norm: None,
            initial_crop: None,
        }
    }
}

impl ManualCropState {
    pub fn pixel_rect(&self, img_w: usize, img_h: usize) -> (usize, usize, usize, usize) {
        if img_w == 0 || img_h == 0 {
            return (0, 0, 0, 0);
        }
        let x = (self.x * img_w as f32).round().clamp(0.0, (img_w.saturating_sub(1)) as f32) as usize;
        let y = (self.y * img_h as f32).round().clamp(0.0, (img_h.saturating_sub(1)) as f32) as usize;
        let max_w = img_w.saturating_sub(x);
        let max_h = img_h.saturating_sub(y);
        let w = ((self.w * img_w as f32).round() as usize).clamp(1, max_w);
        let h = ((self.h * img_h as f32).round() as usize).clamp(1, max_h);
        (x, y, w, h)
    }

    pub fn set_pixel_rect(&mut self, x: usize, y: usize, w: usize, h: usize, img_w: usize, img_h: usize) {
        if img_w == 0 || img_h == 0 {
            return;
        }
        self.x = (x as f32 / img_w as f32).clamp(0.0, 1.0);
        self.y = (y as f32 / img_h as f32).clamp(0.0, 1.0);
        self.w = (w as f32 / img_w as f32).clamp(0.01, 1.0 - self.x);
        self.h = (h as f32 / img_h as f32).clamp(0.01, 1.0 - self.y);
    }

    pub fn reset_to_full(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
        self.w = 1.0;
        self.h = 1.0;
    }
}

pub struct ImageViewer {
    pub pan: Vec2,
    pub zoom: f32,
    pub fit_to_window: bool,
    texture: Option<TextureHandle>,
    last_image_id: usize,
    last_stf_hash: u64,
    last_curves_hash: u64,
}

impl Default for ImageViewer {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            fit_to_window: true,
            texture: None,
            last_image_id: 0,
            last_stf_hash: 0,
            last_curves_hash: 0,
        }
    }
}

impl ImageViewer {
    pub fn reset_view(&mut self) {
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
        self.fit_to_window = true;
    }

    pub fn set_1_to_1(&mut self) {
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
        self.fit_to_window = false;
    }

    pub fn update_texture(
        &mut self,
        ctx: &egui::Context,
        image: &AstroImage,
        stf: &StfParameters,
        image_id: usize,
        curves: Option<&CurvesSet>,
    ) {
        // Fast hash of STF & Curves params
        let stf_hash = compute_stf_hash(stf);
        let curves_hash = compute_curves_hash(curves);

        if self.texture.is_none()
            || self.last_image_id != image_id
            || self.last_stf_hash != stf_hash
            || self.last_curves_hash != curves_hash
        {
            let params = if stf.enabled {
                [
                    (stf.channels[0].shadows, stf.channels[0].midtones, stf.channels[0].highlights),
                    (stf.channels[1].shadows, stf.channels[1].midtones, stf.channels[1].highlights),
                    (stf.channels[2].shadows, stf.channels[2].midtones, stf.channels[2].highlights),
                ]
            } else {
                [(0.0, 0.5, 1.0); 3]
            };

            let mut rgb_bytes = image.to_rgb8_stretched_channels(&params);

            if let Some(c) = curves {
                c.apply_to_rgb8(&mut rgb_bytes);
            }

            let color_image = ColorImage::from_rgb([image.width, image.height], &rgb_bytes);

            self.texture = Some(ctx.load_texture("main_display", color_image, egui::TextureOptions::LINEAR));
            self.last_image_id = image_id;
            self.last_stf_hash = stf_hash;
            self.last_curves_hash = curves_hash;
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        image: Option<&AstroImage>,
        stf: &StfParameters,
        image_id: usize,
        curves: Option<&CurvesSet>,
        crop_state: Option<&mut ManualCropState>,
        annotation: Option<(&AnnotationConfig, &WcsSolution)>,
    ) -> (Option<(usize, usize, Vec<f32>)>, CropAction) {
        let available_size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available_size, Sense::click_and_drag());

        // Dark checkerboard / space background
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(12, 14, 18));

        let Some(img) = image else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                crate::i18n::tr("🌌 Ei kuvaa ladattuna
Avaa FITS-, TIFF- tai PNG-kuva yläpalkista"),
                egui::FontId::proportional(18.0),
                Color32::from_rgb(120, 130, 150),
            );
            return (None, CropAction::None);
        };

        // Update texture if needed
        self.update_texture(ui.ctx(), img, stf, image_id, curves);

        let Some(texture) = &self.texture else {
            return (None, CropAction::None);
        };

        let img_w = img.width as f32;
        let img_h = img.height as f32;

        // Auto-fit if requested
        if self.fit_to_window {
            let scale_x = (rect.width() * 0.95) / img_w;
            let scale_y = (rect.height() * 0.95) / img_h;
            self.zoom = scale_x.min(scale_y);
            self.pan = Vec2::ZERO;
        }

        let is_crop_active = crop_state.as_ref().map(|c| c.is_active).unwrap_or(false);

        // Handle panning with mouse drag
        if is_crop_active {
            if response.dragged_by(egui::PointerButton::Middle) {
                self.pan += response.drag_delta();
                self.fit_to_window = false;
            }
        } else {
            if response.dragged_by(egui::PointerButton::Primary)
                || response.dragged_by(egui::PointerButton::Middle)
            {
                self.pan += response.drag_delta();
                self.fit_to_window = false;
            }
        }

        // Handle zooming with mouse wheel
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 && response.hovered() {
            let zoom_factor = if scroll_delta > 0.0 { 1.15 } else { 0.87 };
            let old_zoom = self.zoom;
            self.zoom = (self.zoom * zoom_factor).clamp(0.02, 50.0);

            if let Some(hover_pos) = response.hover_pos() {
                let center_offset = hover_pos - rect.center() - self.pan;
                self.pan -= center_offset * (self.zoom / old_zoom - 1.0);
            }
            self.fit_to_window = false;
        }

        // Calculate destination rect
        let displayed_size = Vec2::new(img_w * self.zoom, img_h * self.zoom);
        let center = rect.center() + self.pan;
        let dest_rect = Rect::from_center_size(center, displayed_size);

        // Draw image texture
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        ui.painter().image(texture.id(), dest_rect, uv, Color32::WHITE);

        // Calculate hovered pixel coordinates
        let hovered_pixel = if let Some(pos) = response.hover_pos() {
            if dest_rect.contains(pos) {
                let norm_x = (pos.x - dest_rect.min.x) / dest_rect.width();
                let norm_y = (pos.y - dest_rect.min.y) / dest_rect.height();

                let px = (norm_x * img_w).clamp(0.0, img_w - 1.0) as usize;
                let py = (norm_y * img_h).clamp(0.0, img_h - 1.0) as usize;

                let pixel_data = img.get_pixel(px, py).to_vec();
                Some((px, py, pixel_data))
            } else {
                None
            }
        } else {
            None
        };

        // Draw WCS Celestial Grid & DSO/Star Annotations if enabled
        if let Some((ann_cfg, wcs)) = annotation {
            if ann_cfg.enabled {
                let hovered_px = hovered_pixel.as_ref().map(|(x, y, _)| (*x, *y));
                self.draw_wcs_overlay(ui, rect, dest_rect, img_w, img_h, ann_cfg, wcs, hovered_px);
            }
        }

        let mut crop_action = CropAction::None;

        if let Some(crop) = crop_state {
            if crop.is_active {
                let crop_min = Pos2::new(
                    dest_rect.min.x + crop.x.clamp(0.0, 1.0) * dest_rect.width(),
                    dest_rect.min.y + crop.y.clamp(0.0, 1.0) * dest_rect.height(),
                );
                let crop_max = Pos2::new(
                    dest_rect.min.x + (crop.x + crop.w).clamp(0.0, 1.0) * dest_rect.width(),
                    dest_rect.min.y + (crop.y + crop.h).clamp(0.0, 1.0) * dest_rect.height(),
                );
                let crop_rect = Rect::from_min_max(crop_min, crop_max);

                // Handle hover cursor
                if let Some(pos) = response.hover_pos() {
                    if !crop.is_dragging {
                        let handle = if pos.distance(crop_rect.left_top()) <= 14.0 {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                            Some(CropHandle::TopLeft)
                        } else if pos.distance(crop_rect.right_top()) <= 14.0 {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw);
                            Some(CropHandle::TopRight)
                        } else if pos.distance(crop_rect.left_bottom()) <= 14.0 {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw);
                            Some(CropHandle::BottomLeft)
                        } else if pos.distance(crop_rect.right_bottom()) <= 14.0 {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                            Some(CropHandle::BottomRight)
                        } else if (pos.y - crop_rect.top()).abs() <= 8.0 && pos.x >= crop_rect.left() && pos.x <= crop_rect.right() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                            Some(CropHandle::Top)
                        } else if (pos.y - crop_rect.bottom()).abs() <= 8.0 && pos.x >= crop_rect.left() && pos.x <= crop_rect.right() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                            Some(CropHandle::Bottom)
                        } else if (pos.x - crop_rect.left()).abs() <= 8.0 && pos.y >= crop_rect.top() && pos.y <= crop_rect.bottom() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            Some(CropHandle::Left)
                        } else if (pos.x - crop_rect.right()).abs() <= 8.0 && pos.y >= crop_rect.top() && pos.y <= crop_rect.bottom() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            Some(CropHandle::Right)
                        } else if crop_rect.contains(pos) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                            Some(CropHandle::Inside)
                        } else if dest_rect.contains(pos) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                            Some(CropHandle::NewBox)
                        } else {
                            None
                        };
                        crop.drag_handle = handle;
                    }
                }

                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        crop.is_dragging = true;
                        let norm_x = ((pos.x - dest_rect.min.x) / dest_rect.width()).clamp(0.0, 1.0);
                        let norm_y = ((pos.y - dest_rect.min.y) / dest_rect.height()).clamp(0.0, 1.0);
                        crop.drag_start_norm = Some(Pos2::new(norm_x, norm_y));
                        crop.initial_crop = Some([crop.x, crop.y, crop.w, crop.h]);
                    }
                }

                if crop.is_dragging && response.dragged_by(egui::PointerButton::Primary) {
                    if let (Some(pos), Some(start_norm), Some(init)) = (
                        response.interact_pointer_pos(),
                        crop.drag_start_norm,
                        crop.initial_crop,
                    ) {
                        let curr_norm_x = ((pos.x - dest_rect.min.x) / dest_rect.width()).clamp(0.0, 1.0);
                        let curr_norm_y = ((pos.y - dest_rect.min.y) / dest_rect.height()).clamp(0.0, 1.0);
                        let dx = curr_norm_x - start_norm.x;
                        let dy = curr_norm_y - start_norm.y;

                        match crop.drag_handle {
                            Some(CropHandle::Inside) => {
                                crop.x = (init[0] + dx).clamp(0.0, 1.0 - crop.w);
                                crop.y = (init[1] + dy).clamp(0.0, 1.0 - crop.h);
                            }
                            Some(CropHandle::TopLeft) => {
                                let nx = (init[0] + dx).clamp(0.0, init[0] + init[2] - 0.02);
                                let ny = (init[1] + dy).clamp(0.0, init[1] + init[3] - 0.02);
                                crop.w = init[0] + init[2] - nx;
                                crop.h = init[1] + init[3] - ny;
                                crop.x = nx;
                                crop.y = ny;
                            }
                            Some(CropHandle::TopRight) => {
                                let ny = (init[1] + dy).clamp(0.0, init[1] + init[3] - 0.02);
                                crop.w = (init[2] + dx).clamp(0.02, 1.0 - init[0]);
                                crop.h = init[1] + init[3] - ny;
                                crop.y = ny;
                            }
                            Some(CropHandle::BottomLeft) => {
                                let nx = (init[0] + dx).clamp(0.0, init[0] + init[2] - 0.02);
                                crop.w = init[0] + init[2] - nx;
                                crop.h = (init[3] + dy).clamp(0.02, 1.0 - init[1]);
                                crop.x = nx;
                            }
                            Some(CropHandle::BottomRight) => {
                                crop.w = (init[2] + dx).clamp(0.02, 1.0 - init[0]);
                                crop.h = (init[3] + dy).clamp(0.02, 1.0 - init[1]);
                            }
                            Some(CropHandle::Top) => {
                                let ny = (init[1] + dy).clamp(0.0, init[1] + init[3] - 0.02);
                                crop.h = init[1] + init[3] - ny;
                                crop.y = ny;
                            }
                            Some(CropHandle::Bottom) => {
                                crop.h = (init[3] + dy).clamp(0.02, 1.0 - init[1]);
                            }
                            Some(CropHandle::Left) => {
                                let nx = (init[0] + dx).clamp(0.0, init[0] + init[2] - 0.02);
                                crop.w = init[0] + init[2] - nx;
                                crop.x = nx;
                            }
                            Some(CropHandle::Right) => {
                                crop.w = (init[2] + dx).clamp(0.02, 1.0 - init[0]);
                            }
                            Some(CropHandle::NewBox) => {
                                let min_x = start_norm.x.min(curr_norm_x);
                                let min_y = start_norm.y.min(curr_norm_y);
                                let max_x = start_norm.x.max(curr_norm_x);
                                let max_y = start_norm.y.max(curr_norm_y);
                                crop.x = min_x;
                                crop.y = min_y;
                                crop.w = (max_x - min_x).max(0.02);
                                crop.h = (max_y - min_y).max(0.02);
                            }
                            None => {}
                        }

                        // Maintain aspect ratio if set
                        if let Some(target_ar) = crop.aspect_ratio.ratio() {
                            let norm_ar = target_ar * (img_h / img_w);
                            crop.w = (crop.h * norm_ar).clamp(0.02, 1.0 - crop.x);
                        }
                    }
                }

                if response.drag_stopped() {
                    crop.is_dragging = false;
                    crop.drag_start_norm = None;
                    crop.initial_crop = None;
                }

                // Recalculate crop_rect for rendering
                let crop_min = Pos2::new(
                    dest_rect.min.x + crop.x.clamp(0.0, 1.0) * dest_rect.width(),
                    dest_rect.min.y + crop.y.clamp(0.0, 1.0) * dest_rect.height(),
                );
                let crop_max = Pos2::new(
                    dest_rect.min.x + (crop.x + crop.w).clamp(0.0, 1.0) * dest_rect.width(),
                    dest_rect.min.y + (crop.y + crop.h).clamp(0.0, 1.0) * dest_rect.height(),
                );
                let crop_rect = Rect::from_min_max(crop_min, crop_max);

                // 1. Dim outside 4 regions
                let dim_color = Color32::from_rgba_premultiplied(0, 0, 0, 175);
                if crop_rect.top() > dest_rect.top() {
                    ui.painter().rect_filled(
                        Rect::from_min_max(dest_rect.min, Pos2::new(dest_rect.max.x, crop_rect.top())),
                        0.0,
                        dim_color,
                    );
                }
                if crop_rect.bottom() < dest_rect.bottom() {
                    ui.painter().rect_filled(
                        Rect::from_min_max(Pos2::new(dest_rect.min.x, crop_rect.bottom()), dest_rect.max),
                        0.0,
                        dim_color,
                    );
                }
                if crop_rect.left() > dest_rect.min.x {
                    ui.painter().rect_filled(
                        Rect::from_min_max(Pos2::new(dest_rect.min.x, crop_rect.top()), Pos2::new(crop_rect.left(), crop_rect.bottom())),
                        0.0,
                        dim_color,
                    );
                }
                if crop_rect.right() < dest_rect.max.x {
                    ui.painter().rect_filled(
                        Rect::from_min_max(Pos2::new(crop_rect.right(), crop_rect.top()), Pos2::new(dest_rect.max.x, crop_rect.bottom())),
                        0.0,
                        dim_color,
                    );
                }

                // 2. Crop border stroke
                ui.painter().rect(
                    crop_rect,
                    0.0,
                    Color32::TRANSPARENT,
                    Stroke::new(2.0_f32, Color32::from_rgb(80, 200, 255)),
                    egui::StrokeKind::Middle,
                );

                // 3. Rule of thirds grid
                let w3 = crop_rect.width() / 3.0;
                let h3 = crop_rect.height() / 3.0;
                let grid_stroke = Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(255, 255, 255, 65));
                ui.painter().line_segment(
                    [Pos2::new(crop_rect.left() + w3, crop_rect.top()), Pos2::new(crop_rect.left() + w3, crop_rect.bottom())],
                    grid_stroke,
                );
                ui.painter().line_segment(
                    [Pos2::new(crop_rect.left() + 2.0 * w3, crop_rect.top()), Pos2::new(crop_rect.left() + 2.0 * w3, crop_rect.bottom())],
                    grid_stroke,
                );
                ui.painter().line_segment(
                    [Pos2::new(crop_rect.left(), crop_rect.top() + h3), Pos2::new(crop_rect.right(), crop_rect.top() + h3)],
                    grid_stroke,
                );
                ui.painter().line_segment(
                    [Pos2::new(crop_rect.left(), crop_rect.top() + 2.0 * h3), Pos2::new(crop_rect.right(), crop_rect.top() + 2.0 * h3)],
                    grid_stroke,
                );

                // 4. Handles (8 handles)
                let handles = [
                    crop_rect.left_top(),
                    crop_rect.right_top(),
                    crop_rect.left_bottom(),
                    crop_rect.right_bottom(),
                    Pos2::new(crop_rect.center().x, crop_rect.top()),
                    Pos2::new(crop_rect.center().x, crop_rect.bottom()),
                    Pos2::new(crop_rect.left(), crop_rect.center().y),
                    Pos2::new(crop_rect.right(), crop_rect.center().y),
                ];
                for h in handles {
                    ui.painter().circle_filled(h, 5.0, Color32::WHITE);
                    ui.painter().circle_stroke(h, 5.0, Stroke::new(1.5_f32, Color32::from_rgb(0, 120, 255)));
                }

                // 5. Dimension HUD pill
                let px_x = (crop.x * img_w).round() as usize;
                let px_y = (crop.y * img_h).round() as usize;
                let px_w = (crop.w * img_w).round().max(1.0) as usize;
                let px_h = (crop.h * img_h).round().max(1.0) as usize;
                let badge_text = format!("✂️ {} × {} px  (X: {}, Y: {})", px_w, px_h, px_x, px_y);
                let badge_pos = Pos2::new(crop_rect.center().x, (crop_rect.top() - 14.0).max(dest_rect.top() + 12.0));
                ui.painter().text(
                    badge_pos,
                    egui::Align2::CENTER_CENTER,
                    badge_text,
                    egui::FontId::proportional(13.0),
                    Color32::from_rgb(255, 230, 100),
                );

                // 6. Keyboard shortcuts: Enter to apply, Esc to cancel
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    crop_action = CropAction::Apply;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    crop_action = CropAction::Cancel;
                }

                // 7. Floating action bar at bottom of canvas
                let hud_rect = Rect::from_center_size(
                    Pos2::new(rect.center().x, rect.bottom() - 36.0),
                    Vec2::new(500.0, 42.0),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
                    egui::Frame::window(&ui.style())
                        .fill(Color32::from_rgba_premultiplied(18, 22, 32, 230))
                        .corner_radius(8.0)
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 80, 110)))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new(crate::i18n::tr("✂️ Käytä rajaus (Enter)")).color(Color32::from_rgb(100, 255, 150)).strong()).clicked() {
                                    crop_action = CropAction::Apply;
                                }
                                if ui.button(egui::RichText::new(crate::i18n::tr("❌ Peruuta (Esc)")).color(Color32::from_rgb(255, 120, 120))).clicked() {
                                    crop_action = CropAction::Cancel;
                                }
                                ui.separator();
                                if ui.button(crate::i18n::tr("↺ Koko kuva")).clicked() {
                                    crop_action = CropAction::Reset;
                                }
                                ui.separator();
                                ui.label(crate::i18n::tr("Suhde:"));
                                for ar in [CropAspectRatio::Free, CropAspectRatio::Square1x1, CropAspectRatio::Wide16x9, CropAspectRatio::Standard4x3, CropAspectRatio::Photo3x2] {
                                    if ui.selectable_label(crop.aspect_ratio == ar, ar.label()).clicked() {
                                        crop.aspect_ratio = ar;
                                        if let Some(target_ar) = ar.ratio() {
                                            let norm_ar = target_ar * (img_h / img_w);
                                            crop.w = (crop.h * norm_ar).clamp(0.02, 1.0 - crop.x);
                                        }
                                    }
                                }
                            });
                        });
                });
            }
        }

        (hovered_pixel, crop_action)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_wcs_overlay(
        &self,
        ui: &mut egui::Ui,
        viewport_rect: Rect,
        dest_rect: Rect,
        img_w: f32,
        img_h: f32,
        cfg: &AnnotationConfig,
        wcs: &WcsSolution,
        hovered_px: Option<(usize, usize)>,
    ) {
        let clip_rect = viewport_rect.intersect(dest_rect);
        if clip_rect.width() <= 4.0 || clip_rect.height() <= 4.0 {
            return;
        }
        let painter = ui.painter().with_clip_rect(clip_rect);

        let px_to_screen = |px: f32, py: f32| -> Pos2 {
            Pos2::new(
                dest_rect.min.x + (px / img_w) * dest_rect.width(),
                dest_rect.min.y + (py / img_h) * dest_rect.height(),
            )
        };

        let (fov_w_deg, fov_h_deg) = wcs.fov_deg();
        let max_fov = fov_w_deg.max(fov_h_deg).max(0.1);

        // 1. Equatorial RA / Dec Coordinate Grid
        if cfg.show_grid {
            let dec_step_deg = if max_fov < 0.7 {
                0.10 // 6'
            } else if max_fov < 1.8 {
                0.25 // 15'
            } else if max_fov < 4.5 {
                0.50 // 30'
            } else {
                1.00 // 1°
            };

            let cos_dec = wcs.center_dec_deg.to_radians().cos().abs().max(0.15);
            // Match RA step in hours roughly to Dec angular spacing
            let ra_step_hours = ((dec_step_deg / 15.0) / cos_dec).clamp(1.0 / 120.0, 0.25);
            // Snap RA step to clean minute fractions (0.5m, 1m, 2m, 5m, 10m)
            let ra_step_min = (ra_step_hours * 60.0).round().max(0.5);
            let clean_ra_step_h = ra_step_min / 60.0;

            let grid_stroke = Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(70, 185, 255, 78));
            let label_color = Color32::from_rgba_premultiplied(150, 220, 255, 210);

            let dec_span = max_fov * 1.15;
            let start_dec = ((wcs.center_dec_deg - dec_span) / dec_step_deg).floor() * dec_step_deg;
            let end_dec = ((wcs.center_dec_deg + dec_span) / dec_step_deg).ceil() * dec_step_deg;

            let ra_span_h = (max_fov / (15.0 * cos_dec)) * 1.15;
            let start_ra =
                ((wcs.center_ra_hours - ra_span_h) / clean_ra_step_h).floor() * clean_ra_step_h;
            let end_ra =
                ((wcs.center_ra_hours + ra_span_h) / clean_ra_step_h).ceil() * clean_ra_step_h;

            // Constant Declination parallels
            let mut dec = start_dec;
            while dec <= end_dec {
                if dec >= -89.8 && dec <= 89.8 {
                    let mut pts = Vec::with_capacity(25);
                    for i in 0..=24 {
                        let frac = i as f64 / 24.0;
                        let ra = (start_ra + (end_ra - start_ra) * frac).rem_euclid(24.0);
                        if let Some((px, py)) = wcs.radec_to_pixel(ra, dec) {
                            pts.push(px_to_screen(px, py));
                        }
                    }
                    for seg in pts.windows(2) {
                        painter.line_segment([seg[0], seg[1]], grid_stroke);
                    }
                    // Label Dec line inside visible clip area
                    if let Some(&label_pos) = pts.iter().find(|p| {
                        p.x >= clip_rect.left() + 14.0
                            && p.x <= clip_rect.right() - 60.0
                            && p.y >= clip_rect.top() + 18.0
                            && p.y <= clip_rect.bottom() - 18.0
                    }) {
                        painter.text(
                            Pos2::new(label_pos.x + 4.0, label_pos.y - 8.0),
                            egui::Align2::LEFT_BOTTOM,
                            format!("{:+.2}°", dec),
                            egui::FontId::monospace(10.5),
                            label_color,
                        );
                    }
                }
                dec += dec_step_deg;
            }

            // Constant Right Ascension meridians
            let mut ra = start_ra;
            while ra <= end_ra {
                let ra_norm = ra.rem_euclid(24.0);
                let mut pts = Vec::with_capacity(25);
                for i in 0..=24 {
                    let frac = i as f64 / 24.0;
                    let d = (start_dec + (end_dec - start_dec) * frac).clamp(-89.8, 89.8);
                    if let Some((px, py)) = wcs.radec_to_pixel(ra_norm, d) {
                        pts.push(px_to_screen(px, py));
                    }
                }
                for seg in pts.windows(2) {
                    painter.line_segment([seg[0], seg[1]], grid_stroke);
                }
                if let Some(&label_pos) = pts.iter().find(|p| {
                    p.x >= clip_rect.left() + 24.0
                        && p.x <= clip_rect.right() - 65.0
                        && p.y >= clip_rect.top() + 26.0
                        && p.y <= clip_rect.bottom() - 20.0
                }) {
                    let total_m = ra_norm * 60.0;
                    let h = (total_m / 60.0).floor() as u32;
                    let m = total_m % 60.0;
                    painter.text(
                        Pos2::new(label_pos.x + 4.0, label_pos.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{:02}h{:04.1}m", h, m),
                        egui::FontId::monospace(10.5),
                        label_color,
                    );
                }
                ra += clean_ra_step_h;
            }

            // Solved WCS plate center reticle
            let center_scr = px_to_screen(wcs.crpix_x as f32, wcs.crpix_y as f32);
            let ret_stroke = Stroke::new(1.2_f32, Color32::from_rgba_premultiplied(120, 220, 255, 160));
            painter.circle_stroke(center_scr, 7.0, ret_stroke);
            painter.line_segment(
                [Pos2::new(center_scr.x - 12.0, center_scr.y), Pos2::new(center_scr.x + 12.0, center_scr.y)],
                ret_stroke,
            );
            painter.line_segment(
                [Pos2::new(center_scr.x, center_scr.y - 12.0), Pos2::new(center_scr.x, center_scr.y + 12.0)],
                ret_stroke,
            );
        }

        // 2. Detected Stars Overlay
        if cfg.show_detected_stars {
            let star_stroke = Stroke::new(1.2_f32, Color32::from_rgba_premultiplied(90, 250, 170, 185));
            let star_r = (5.0 * self.zoom).clamp(4.5, 13.0);

            for (idx, star) in wcs.detected_stars.iter().take(cfg.max_annotated_stars).enumerate() {
                let pos = px_to_screen(star.x, star.y);
                if clip_rect.contains(pos) {
                    painter.circle_stroke(pos, star_r, star_stroke);
                    if idx < 12 {
                        painter.text(
                            Pos2::new(pos.x + star_r + 3.0, pos.y - star_r),
                            egui::Align2::LEFT_BOTTOM,
                            format!("#{}", idx + 1),
                            egui::FontId::monospace(10.0),
                            Color32::from_rgba_premultiplied(140, 255, 190, 215),
                        );
                    }
                }
            }
        }

        // 3. Catalog DSO Objects Overlay
        if cfg.show_dso_objects {
            for obj in &wcs.matched_objects {
                let pos = px_to_screen(obj.pixel_x, obj.pixel_y);
                let (cr, cg, cb) = obj.obj_type.color_rgb();
                let main_col = Color32::from_rgb(cr, cg, cb);
                let glow_col = Color32::from_rgba_premultiplied(cr / 3, cg / 3, cb / 3, 95);

                let r_scr = (obj.radius_px * self.zoom).clamp(16.0, dest_rect.width().max(dest_rect.height()) * 0.42);
                painter.circle_stroke(pos, r_scr + 2.0, Stroke::new(3.5_f32, glow_col));
                painter.circle_stroke(pos, r_scr, Stroke::new(1.8_f32, main_col));

                // Target ticks around circle
                let tick = 6.0;
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    painter.line_segment(
                        [
                            Pos2::new(pos.x + dx * (r_scr - tick), pos.y + dy * (r_scr - tick)),
                            Pos2::new(pos.x + dx * (r_scr + tick), pos.y + dy * (r_scr + tick)),
                        ],
                        Stroke::new(1.6_f32, main_col),
                    );
                }

                // Label pill
                let label_txt = format!(
                    "{} {} – {} ({:.0}')",
                    obj.obj_type.icon(),
                    obj.id,
                    obj.name,
                    obj.size_arcmin
                );
                let label_y = if pos.y - r_scr - 18.0 > clip_rect.top() + 28.0 {
                    pos.y - r_scr - 10.0
                } else {
                    (pos.y + r_scr + 14.0).min(clip_rect.bottom() - 18.0)
                };
                let label_pos = Pos2::new(
                    pos.x.clamp(clip_rect.left() + 90.0, clip_rect.right() - 90.0),
                    label_y,
                );

                let approx_w = (label_txt.chars().count() as f32 * 6.8).max(90.0);
                let bg_pill = Rect::from_center_size(label_pos, Vec2::new(approx_w + 14.0, 20.0));
                painter.rect_filled(bg_pill, 5.0, Color32::from_rgba_premultiplied(12, 16, 24, 215));
                painter.rect_stroke(
                    bg_pill,
                    5.0,
                    Stroke::new(1.0_f32, main_col),
                    egui::StrokeKind::Middle,
                );
                painter.text(
                    label_pos,
                    egui::Align2::CENTER_CENTER,
                    label_txt,
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(245, 248, 255),
                );
            }
        }

        // 4. Celestial Compass Rose (N / E) in top-right corner
        if cfg.show_compass {
            let compass_center = Pos2::new(clip_rect.right() - 46.0, clip_rect.top() + 46.0);
            let dial_r = 30.0;
            painter.circle_filled(
                compass_center,
                dial_r + 4.0,
                Color32::from_rgba_premultiplied(12, 16, 24, 210),
            );
            painter.circle_stroke(
                compass_center,
                dial_r + 4.0,
                Stroke::new(1.2_f32, Color32::from_rgb(70, 100, 140)),
            );

            let rot = wcs.rotation_deg.to_radians() as f32;
            let parity = wcs.parity as f32;
            // North (+Dec) points up (-Y) when rotation = 0
            let n_dir = Vec2::new(rot.sin(), -rot.cos());
            // East (+RA) points left (-X) when parity = +1 and rotation = 0
            let e_dir = Vec2::new(-rot.cos() * parity, -rot.sin() * parity);

            let n_tip = compass_center + n_dir * (dial_r - 5.0);
            let e_tip = compass_center + e_dir * (dial_r - 5.0);

            painter.line_segment(
                [compass_center, n_tip],
                Stroke::new(2.2_f32, Color32::from_rgb(255, 110, 90)),
            );
            painter.line_segment(
                [compass_center, e_tip],
                Stroke::new(2.0_f32, Color32::from_rgb(90, 210, 255)),
            );
            painter.circle_filled(compass_center, 2.5, Color32::WHITE);

            painter.text(
                compass_center + n_dir * (dial_r + 1.0),
                egui::Align2::CENTER_CENTER,
                "N",
                egui::FontId::proportional(11.5),
                Color32::from_rgb(255, 130, 110),
            );
            painter.text(
                compass_center + e_dir * (dial_r + 1.0),
                egui::Align2::CENTER_CENTER,
                "E",
                egui::FontId::proportional(11.5),
                Color32::from_rgb(110, 220, 255),
            );
        }

        // 5. Top-left WCS HUD Banner & Live Cursor RA/Dec Readout
        let hud_txt = if let Some((hx, hy)) = hovered_px {
            let (cur_ra, cur_dec) = wcs.pixel_to_radec(hx as f64, hy as f64);
            format!(
                "🧭 WCS: {}  {}  |  FOV {:.2}°×{:.2}° ({:.2}\"/px)  |  🎯 Kursori: {}  {}",
                format_ra(wcs.center_ra_hours),
                format_dec(wcs.center_dec_deg),
                fov_w_deg,
                fov_h_deg,
                wcs.arcsec_per_px,
                format_ra(cur_ra),
                format_dec(cur_dec),
            )
        } else {
            format!(
                "🧭 WCS: {}  {}  |  FOV {:.2}°×{:.2}° ({:.2}\"/px)  |  {}",
                format_ra(wcs.center_ra_hours),
                format_dec(wcs.center_dec_deg),
                fov_w_deg,
                fov_h_deg,
                wcs.arcsec_per_px,
                wcs.solver_source,
            )
        };

        let banner_w = (hud_txt.chars().count() as f32 * 6.5 + 20.0).min(clip_rect.width() - 95.0);
        let banner_rect = Rect::from_min_size(
            Pos2::new(clip_rect.left() + 10.0, clip_rect.top() + 10.0),
            Vec2::new(banner_w.max(220.0), 24.0),
        );
        painter.rect_filled(
            banner_rect,
            6.0,
            Color32::from_rgba_premultiplied(12, 16, 24, 220),
        );
        painter.rect_stroke(
            banner_rect,
            6.0,
            Stroke::new(1.0_f32, Color32::from_rgb(70, 140, 200)),
            egui::StrokeKind::Middle,
        );
        painter.text(
            Pos2::new(banner_rect.left() + 10.0, banner_rect.center().y),
            egui::Align2::LEFT_CENTER,
            hud_txt,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(210, 235, 255),
        );
    }
}

fn compute_stf_hash(stf: &StfParameters) -> u64 {
    let mut h: u64 = if stf.enabled { 1 } else { 0 };
    h = h.wrapping_mul(31).wrapping_add(if stf.linked_channels { 7 } else { 3 });
    h = h.wrapping_mul(31).wrapping_add(stf.target_bg.to_bits() as u64);
    for ch in &stf.channels {
        h = h.wrapping_mul(31).wrapping_add(ch.shadows.to_bits() as u64);
        h = h.wrapping_mul(31).wrapping_add(ch.midtones.to_bits() as u64);
        h = h.wrapping_mul(31).wrapping_add(ch.highlights.to_bits() as u64);
    }
    h
}

fn compute_curves_hash(curves: Option<&CurvesSet>) -> u64 {
    let Some(curves) = curves else { return 0 };
    if curves.is_identity() {
        return 0;
    }
    let mut h: u64 = 17;
    for ch in [
        CurvesChannel::Rgb,
        CurvesChannel::Red,
        CurvesChannel::Green,
        CurvesChannel::Blue,
        CurvesChannel::Saturation,
    ] {
        let curve = curves.get_curve(ch);
        h = h.wrapping_mul(31).wrapping_add(curve.points.len() as u64);
        for p in &curve.points {
            h = h.wrapping_mul(31).wrapping_add(p.x.to_bits() as u64);
            h = h.wrapping_mul(31).wrapping_add(p.y.to_bits() as u64);
        }
    }
    h
}

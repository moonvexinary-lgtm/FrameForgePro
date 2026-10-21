//! The program monitor's pixel path: the editing engine's compositor output
//! (premultiplied, linear-light f32) to an egui texture (premultiplied sRGB
//! 8-bit). Unpremultiply in linear, sRGB-encode, premultiply again — the same
//! round trip the upstream monitors use.

pub fn to_color_image(img: &filmcraft_render::Image) -> egui::ColorImage {
    let srgb = |v: f32| {
        let c = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        if c <= 0.003_1308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
    };
    let mut rgba = Vec::with_capacity(img.px.len());
    for px in img.px.chunks(4) {
        let a = if px[3].is_finite() { px[3].clamp(0.0, 1.0) } else { 0.0 };
        let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
        let channel = |v: f32| {
            let s = srgb(if v.is_finite() { v * inv } else { 0.0 });
            (s * a * 255.0 + 0.5) as u8
        };
        rgba.extend_from_slice(&[channel(px[0]), channel(px[1]), channel(px[2]), (a * 255.0 + 0.5) as u8]);
    }
    egui::ColorImage::from_rgba_premultiplied([img.w, img.h], &rgba)
}

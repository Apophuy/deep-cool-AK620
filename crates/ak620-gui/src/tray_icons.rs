//! Embedded tray artwork rendered as StatusNotifierItem ARGB pixmaps.

use std::sync::LazyLock;

use image::{ImageFormat, RgbaImage, imageops::FilterType};
use ksni::Icon;

const NORMAL_PNG: &[u8] = include_bytes!(
    "../../../packaging/icons/hicolor/256x256/apps/io.github.ak620linux.Control.png"
);
const ATTENTION_PNG: &[u8] = include_bytes!(
    "../../../packaging/icons/hicolor/256x256/apps/io.github.ak620linux.Control-attention.png"
);

#[derive(Clone, Copy)]
struct IconSize {
    raster: u32,
    dbus: i32,
    pixels: usize,
}

const ICON_SIZES: [IconSize; 3] = [
    IconSize {
        raster: 22,
        dbus: 22,
        pixels: 22,
    },
    IconSize {
        raster: 32,
        dbus: 32,
        pixels: 32,
    },
    IconSize {
        raster: 48,
        dbus: 48,
        pixels: 48,
    },
];

static NORMAL: LazyLock<Option<Vec<Icon>>> = LazyLock::new(|| render_family(NORMAL_PNG, "normal"));
static ATTENTION: LazyLock<Option<Vec<Icon>>> =
    LazyLock::new(|| render_family(ATTENTION_PNG, "attention"));

pub(crate) fn normal() -> Vec<Icon> {
    NORMAL.clone().unwrap_or_default()
}

pub(crate) fn attention() -> Vec<Icon> {
    ATTENTION.clone().unwrap_or_default()
}

fn render_family(bytes: &[u8], label: &str) -> Option<Vec<Icon>> {
    let master = match image::load_from_memory_with_format(bytes, ImageFormat::Png) {
        Ok(image) => image.into_rgba8(),
        Err(error) => {
            eprintln!("could not decode embedded {label} tray icon: {error}");
            return None;
        }
    };
    Some(
        ICON_SIZES
            .into_iter()
            .map(|size| render_icon(&master, size))
            .collect(),
    )
}

fn render_icon(master: &RgbaImage, size: IconSize) -> Icon {
    let resized = image::imageops::resize(master, size.raster, size.raster, FilterType::Lanczos3);
    let mut data = Vec::with_capacity(size.pixels * size.pixels * 4);
    for pixel in resized.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        data.extend_from_slice(&[alpha, red, green, blue]);
    }
    Icon {
        width: size.dbus,
        height: size.dbus,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::{ICON_SIZES, attention, normal};
    use ksni::Icon;

    #[test]
    fn both_states_publish_well_formed_full_bleed_pixmaps() {
        for family in [normal(), attention()] {
            assert_eq!(family.len(), ICON_SIZES.len());
            for (icon, expected) in family.iter().zip(ICON_SIZES) {
                assert_eq!((icon.width, icon.height), (expected.dbus, expected.dbus));
                assert_eq!(icon.data.len(), expected.pixels * expected.pixels * 4);
                assert!(edge_has_content(icon, expected.pixels, Edge::Top));
                assert!(edge_has_content(icon, expected.pixels, Edge::Right));
                assert!(edge_has_content(icon, expected.pixels, Edge::Bottom));
                assert!(edge_has_content(icon, expected.pixels, Edge::Left));
            }
        }
    }

    #[derive(Clone, Copy)]
    enum Edge {
        Top,
        Right,
        Bottom,
        Left,
    }

    fn edge_has_content(icon: &Icon, side: usize, edge: Edge) -> bool {
        (0..side).any(|offset| {
            let (x, y) = match edge {
                Edge::Top => (offset, 0),
                Edge::Right => (side - 1, offset),
                Edge::Bottom => (offset, side - 1),
                Edge::Left => (0, offset),
            };
            icon.data[(y * side + x) * 4] != 0
        })
    }
}

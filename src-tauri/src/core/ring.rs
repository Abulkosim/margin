use std::f64::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconStyle {
    Template,
    LightTaskbar,
    DarkTaskbar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    Normal,
    Warn,
    Critical,
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Arc,
    Track,
    Dash,
    Mark,
}

struct Palette {
    normal: [u8; 3],
    warn: [u8; 3],
    critical: [u8; 3],
    track: [u8; 3],
    neutral: [u8; 3],
}

const LIGHT: Palette = Palette {
    normal: [0x14, 0x6c, 0x3b],
    warn: [0x9a, 0x4f, 0x00],
    critical: [0xb4, 0x23, 0x18],
    track: [0x7a, 0x7a, 0x7a],
    neutral: [0x4a, 0x4a, 0x4a],
};

const DARK: Palette = Palette {
    normal: [0x5b, 0xd0, 0x8a],
    warn: [0xff, 0xb5, 0x47],
    critical: [0xff, 0x6b, 0x5e],
    track: [0x7a, 0x7a, 0x7a],
    neutral: [0xc8, 0xc8, 0xc8],
};

const SAMPLES: usize = 8;
const OUTER: f64 = 0.47;
const STROKE: f64 = 0.17;
const TRACK: f64 = 0.55;
const DOT: f64 = 0.15;

fn tier(value: f64) -> Tier {
    if value <= 0.05 {
        Tier::Critical
    } else if value <= 0.2 {
        Tier::Warn
    } else {
        Tier::Normal
    }
}

fn part_at(dx: f64, dy: f64, size: f64, value: Option<f64>, stale: bool) -> Option<Part> {
    let stroke = STROKE * size;
    let mid = OUTER * size - stroke / 2.0;
    let radius = dx.hypot(dy);
    let angle = (dy.atan2(dx) + FRAC_PI_2).rem_euclid(TAU) / TAU;
    if stale {
        if dx.abs() <= 0.18 * size && dy.abs() <= 0.06 * size {
            return Some(Part::Mark);
        }
    } else if value.is_some_and(|value| value <= 0.2) && radius <= DOT * size {
        return Some(Part::Mark);
    }
    let in_band = (radius - mid).abs() <= stroke / 2.0;
    let Some(value) = value else {
        return (in_band && (angle * 10.0).fract() < 0.55).then_some(Part::Dash);
    };
    if value > 0.0 {
        let cap = |turn: f64| {
            let theta = turn * TAU - FRAC_PI_2;
            (dx - mid * theta.cos()).hypot(dy - mid * theta.sin()) <= stroke / 2.0
        };
        if (in_band && angle <= value) || cap(0.0) || cap(value) {
            return Some(Part::Arc);
        }
    }
    ((radius - mid).abs() <= stroke * TRACK / 2.0).then_some(Part::Track)
}

fn color(part: Part, value: Option<f64>, stale: bool, style: IconStyle) -> ([u8; 3], f64) {
    let palette = match style {
        IconStyle::Template => {
            let alpha = match part {
                Part::Arc | Part::Mark => 1.0,
                Part::Dash => 0.75,
                Part::Track => 0.5,
            };
            return ([0, 0, 0], alpha);
        }
        IconStyle::LightTaskbar => &LIGHT,
        IconStyle::DarkTaskbar => &DARK,
    };
    let rgb = match (part, value) {
        (Part::Track, _) => palette.track,
        (Part::Dash, _) | (_, None) => palette.neutral,
        _ if stale => palette.neutral,
        (_, Some(value)) => match tier(value) {
            Tier::Normal => palette.normal,
            Tier::Warn => palette.warn,
            Tier::Critical => palette.critical,
        },
    };
    (rgb, 1.0)
}

pub struct Pixel {
    pub coverage: f64,
    pub rgba: [u8; 4],
}

pub fn raster(size: u32, remaining: Option<f64>, stale: bool, style: IconStyle) -> Vec<Pixel> {
    let value = remaining.map(|value| value.clamp(0.0, 100.0) / 100.0);
    let extent = f64::from(size);
    let centre = extent / 2.0;
    let total = (SAMPLES * SAMPLES) as f64;
    let mut pixels = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut covered, mut alpha, mut channels) = (0.0, 0.0, [0.0_f64; 3]);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let dx = f64::from(x) + (sx as f64 + 0.5) / SAMPLES as f64 - centre;
                    let dy = f64::from(y) + (sy as f64 + 0.5) / SAMPLES as f64 - centre;
                    if let Some(part) = part_at(dx, dy, extent, value, stale) {
                        let (rgb, weight) = color(part, value, stale, style);
                        covered += 1.0;
                        alpha += weight;
                        for (channel, component) in channels.iter_mut().zip(rgb) {
                            *channel += f64::from(component) * weight;
                        }
                    }
                }
            }
            let rgb = channels.map(|channel| {
                if alpha > 0.0 {
                    (channel / alpha).round() as u8
                } else {
                    0
                }
            });
            pixels.push(Pixel {
                coverage: covered / total,
                rgba: [
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    (alpha / total * 255.0).round() as u8,
                ],
            });
        }
    }
    pixels
}

pub fn ring_rgba(size: u32, remaining: Option<f64>, stale: bool, style: IconStyle) -> Vec<u8> {
    raster(size, remaining, stale, style)
        .into_iter()
        .flat_map(|pixel| pixel.rgba)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [u32; 3] = [16, 22, 32];
    const STATES: [(&str, Option<f64>, bool); 6] = [
        ("100", Some(100.0), false),
        ("60", Some(60.0), false),
        ("20", Some(20.0), false),
        ("5", Some(5.0), false),
        ("unknown", None, false),
        ("stale", Some(60.0), true),
    ];
    const TEMPLATE_LIGHT: [u8; 3] = [0xf2, 0xf2, 0xf2];
    const TEMPLATE_DARK: [u8; 3] = [0x26, 0x26, 0x26];
    const TASKBAR_LIGHT: [u8; 3] = [0xee, 0xee, 0xee];
    const TASKBAR_DARK: [u8; 3] = [0x20, 0x20, 0x20];

    struct Case {
        style: IconStyle,
        background: [u8; 3],
        tint: Option<[u8; 3]>,
        name: &'static str,
    }

    fn cases() -> [Case; 4] {
        [
            Case {
                style: IconStyle::Template,
                background: TEMPLATE_LIGHT,
                tint: Some([0, 0, 0]),
                name: "template-light",
            },
            Case {
                style: IconStyle::Template,
                background: TEMPLATE_DARK,
                tint: Some([0xff, 0xff, 0xff]),
                name: "template-dark",
            },
            Case {
                style: IconStyle::LightTaskbar,
                background: TASKBAR_LIGHT,
                tint: None,
                name: "colour-light",
            },
            Case {
                style: IconStyle::DarkTaskbar,
                background: TASKBAR_DARK,
                tint: None,
                name: "colour-dark",
            },
        ]
    }

    fn luminance(rgb: [f64; 3]) -> f64 {
        let channel = |value: f64| {
            let value = value / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
    }

    fn contrast(left: [f64; 3], right: [f64; 3]) -> f64 {
        let (a, b) = (luminance(left), luminance(right));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn composite(rgba: [u8; 4], tint: Option<[u8; 3]>, background: [u8; 3]) -> [f64; 3] {
        let rgb = tint.unwrap_or([rgba[0], rgba[1], rgba[2]]);
        let alpha = f64::from(rgba[3]) / 255.0;
        [0, 1, 2].map(|i| f64::from(rgb[i]) * alpha + f64::from(background[i]) * (1.0 - alpha))
    }

    fn min_contrast(case: &Case, size: u32, value: Option<f64>, stale: bool) -> f64 {
        let background = case.background.map(f64::from);
        raster(size, value, stale, case.style)
            .iter()
            .filter(|pixel| pixel.coverage >= 1.0)
            .map(|pixel| {
                contrast(
                    composite(pixel.rgba, case.tint, case.background),
                    background,
                )
            })
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn every_state_reads_at_tray_sizes_on_light_and_dark_bars() {
        for case in cases() {
            for size in SIZES {
                for (name, value, stale) in STATES {
                    let min = min_contrast(&case, size, value, stale);
                    assert!(
                        min.is_finite(),
                        "{} {size} {name} has no solid pixels",
                        case.name
                    );
                    assert!(min >= 3.0, "{} {size} {name} contrast {min:.2}", case.name);
                }
            }
        }
    }

    #[test]
    fn template_varies_only_alpha() {
        for size in SIZES {
            for (_, value, stale) in STATES {
                let bytes = ring_rgba(size, value, stale, IconStyle::Template);
                assert_eq!(bytes.len(), (size * size * 4) as usize);
                assert!(bytes.chunks(4).all(|pixel| pixel[..3] == [0, 0, 0]));
                assert!(bytes.chunks(4).any(|pixel| pixel[3] > 0 && pixel[3] < 255));
            }
        }
    }

    #[test]
    fn low_states_fill_the_centre_and_unknown_differs_from_empty() {
        let centre = |size: u32, value: Option<f64>, stale: bool| {
            let pixels = raster(size, value, stale, IconStyle::Template);
            pixels[(size / 2 * size + size / 2) as usize].rgba[3]
        };
        for size in SIZES {
            assert_eq!(centre(size, Some(100.0), false), 0);
            assert_eq!(centre(size, Some(60.0), false), 0);
            assert_eq!(centre(size, Some(20.0), false), 255);
            assert_eq!(centre(size, Some(5.0), false), 255);
            assert_eq!(centre(size, Some(60.0), true), 255);
            assert_eq!(centre(size, None, false), 0);
        }
        let corner = ring_rgba(36, Some(100.0), false, IconStyle::Template);
        assert_eq!(corner[3], 0);
        assert_ne!(
            ring_rgba(36, None, false, IconStyle::Template),
            ring_rgba(36, Some(0.0), false, IconStyle::Template)
        );
    }

    #[test]
    fn colour_tiers_follow_remaining() {
        let top = |value: f64, style: IconStyle| {
            let pixels = raster(32, Some(value), false, style);
            let row = (32.0 * (0.5 - OUTER + STROKE / 2.0)) as u32;
            pixels[(row * 32 + 17) as usize].rgba
        };
        for (style, palette) in [
            (IconStyle::LightTaskbar, &LIGHT),
            (IconStyle::DarkTaskbar, &DARK),
        ] {
            assert_eq!(top(60.0, style)[..3], palette.normal);
            assert_eq!(top(20.0, style)[..3], palette.warn);
            assert_eq!(top(5.0, style)[..3], palette.critical);
        }
    }

    #[test]
    #[ignore = "writes docs/images/tray previews"]
    fn write_previews() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/images/tray");
        std::fs::create_dir_all(&directory).unwrap();
        let cell = 40_u32;
        let width = cell * STATES.len() as u32;
        let height = cell * (SIZES.len() * cases().len()) as u32;
        let mut sheet = vec![0_u8; (width * height * 3) as usize];
        for (band, case) in cases().iter().enumerate() {
            for (row, size) in SIZES.into_iter().enumerate() {
                for (column, (name, value, stale)) in STATES.into_iter().enumerate() {
                    let min = min_contrast(case, size, value, stale);
                    println!("{} {size}px {name}: min contrast {min:.2}", case.name);
                    let pixels = raster(size, value, stale, case.style);
                    let top = (band * SIZES.len() + row) as u32 * cell;
                    let left = column as u32 * cell;
                    let inset = (cell - size) / 2;
                    for y in 0..cell {
                        for x in 0..cell {
                            let mut rgb = case.background.map(f64::from);
                            if (inset..inset + size).contains(&x)
                                && (inset..inset + size).contains(&y)
                            {
                                let pixel = &pixels[((y - inset) * size + x - inset) as usize];
                                rgb = composite(pixel.rgba, case.tint, case.background);
                            }
                            let offset = (((top + y) * width + left + x) * 3) as usize;
                            for (i, channel) in rgb.into_iter().enumerate() {
                                sheet[offset + i] = channel.round() as u8;
                            }
                        }
                    }
                }
            }
        }
        let zoom = 4;
        let mut zoomed = vec![0_u8; (width * height * 3 * zoom * zoom) as usize];
        for y in 0..height * zoom {
            for x in 0..width * zoom {
                let source = (((y / zoom) * width + x / zoom) * 3) as usize;
                let target = ((y * width * zoom + x) * 3) as usize;
                zoomed[target..target + 3].copy_from_slice(&sheet[source..source + 3]);
            }
        }
        for (file, data, scale) in [
            ("contact-sheet.png", &sheet, 1),
            ("contact-sheet@4x.png", &zoomed, zoom),
        ] {
            let output = std::fs::File::create(directory.join(file)).unwrap();
            let mut encoder = png::Encoder::new(output, width * scale, height * scale);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(data)
                .unwrap();
        }
    }
}

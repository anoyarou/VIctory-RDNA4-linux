//! A DualSense picture for the title's Settings controls screen, which
//! shows an Xbox controller from a 1024x512 BC7 texture (one zlib stream in
//! the user's copy; tiled 64 KiB `D`). The picture is Xelu's "Free
//! Controller & Key Prompts" PS5 line art (thoseawesomeguys.com/prompts,
//! CC0), which the user downloads: a white body with dark lines, the PS logo
//! left out (taken from the pack's plain variant), drawn wider than it shows
//! because the menu draws the texture at about 0.75 of its width (measured
//! in our runs 54-55). The stream keeps the title's length, so the file is
//! served in place (`VICTORY_FILE_PATCH`).

use victory_formats::texture::{Desc, Dimension, Layout, Swizzle};
use victory_formats::{bc7, zlib_exact};

pub const WIDTH: usize = 1024;
pub const HEIGHT: usize = 512;
/// Bytes of the texture: BC7, 16 bytes per 4x4 block, no mips.
pub const TEXTURE_BYTES: usize = WIDTH * HEIGHT;

/// The share of the texture's width the menu shows it at.
const SQUEEZE: f64 = 0.748;
/// The widest the picture may be in the texture.
const MAX_WIDTH: f64 = 1016.0;
const BODY: [f64; 3] = [244.0, 244.0, 247.0];
const LINE: [f64; 3] = [40.0, 40.0, 46.0];

/// The size of the pack's PS5 diagrams, and where the PS logo is in them.
const XELU_SIZE: (usize, usize) = (1452, 940);
const XELU_LOGO: [usize; 4] = [680, 552, 785, 623]; // x0, y0, x1, y1

/// An RGBA8 image, rows of `width` texels.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub px: Vec<u8>,
}

/// The picture: `full` is the pack's `PS5_Diagram.png`, `plain` its
/// `PS5_Diagram_Simple.png` (the same drawing without logo or symbols).
pub fn compose(full: &Rgba, plain: &Rgba) -> Result<Rgba, String> {
    for img in [full, plain] {
        if (img.width, img.height) != XELU_SIZE || img.px.len() != img.width * img.height * 4 {
            return Err(format!(
                "expected Xelu's {}x{} PS5 diagrams, got {}x{}",
                XELU_SIZE.0, XELU_SIZE.1, img.width, img.height
            ));
        }
    }
    let (w, h) = XELU_SIZE;
    let mut art = full.px.clone();
    let [x0, y0, x1, y1] = XELU_LOGO;
    for y in y0..y1 {
        let row = (y * w + x0) * 4..(y * w + x1) * 4;
        art[row.clone()].copy_from_slice(&plain.px[row]);
    }
    // The body: what the outline encloses (not reached from the corner).
    let line: Vec<f64> = (0..w * h)
        .map(|i| f64::from(art[i * 4 + 3]) / 255.0)
        .collect();
    let mut outside = vec![false; w * h];
    let mut todo = vec![0usize];
    outside[0] = true;
    while let Some(i) = todo.pop() {
        let (x, y) = (i % w, i / w);
        let mut go = |j: usize| {
            if !outside[j] && line[j] <= 0.35 {
                outside[j] = true;
                todo.push(j);
            }
        };
        if x > 0 {
            go(i - 1)
        }
        if x + 1 < w {
            go(i + 1)
        }
        if y > 0 {
            go(i - w)
        }
        if y + 1 < h {
            go(i + w)
        }
    }
    let mut pic = vec![0f64; w * h * 4];
    for i in 0..w * h {
        let a = line[i];
        for c in 0..3 {
            pic[i * 4 + c] = BODY[c] * (1.0 - a) + LINE[c] * a;
        }
        pic[i * 4 + 3] = if outside[i] { a * 255.0 } else { 255.0 };
    }
    // Crop to what shows, then scale: as tall as fits, drawn wide.
    let shows = |i: usize| pic[i * 4 + 3] > 0.0;
    let (mut bx0, mut bx1, mut by0, mut by1) = (w, 0, h, 0);
    for i in (0..w * h).filter(|&i| shows(i)) {
        let (x, y) = (i % w, i / w);
        (bx0, bx1, by0, by1) = (bx0.min(x), bx1.max(x + 1), by0.min(y), by1.max(y + 1));
    }
    if bx0 >= bx1 {
        return Err("the diagram is empty".into());
    }
    let (cw, ch) = (bx1 - bx0, by1 - by0);
    let crop: Vec<f64> = (by0..by1)
        .flat_map(|y| pic[(y * w + bx0) * 4..(y * w + bx1) * 4].iter().copied())
        .collect();
    let sy = (MAX_WIDTH * SQUEEZE / cw as f64).min((HEIGHT - 1) as f64 / ch as f64);
    let sx = sy / SQUEEZE;
    let (nw, nh) = (
        ((cw as f64 * sx).round() as usize).min(WIDTH),
        ((ch as f64 * sy).round() as usize).min(HEIGHT),
    );
    let scaled = resize(&crop, cw, ch, nw, nh);
    let clear = [LINE[0] as u8, LINE[1] as u8, LINE[2] as u8, 0];
    let mut out = Rgba {
        width: WIDTH,
        height: HEIGHT,
        px: clear.repeat(WIDTH * HEIGHT),
    };
    let (ox, oy) = ((WIDTH - nw) / 2, (HEIGHT - 1 - nh) / 2);
    for y in 0..nh {
        for x in 0..nw {
            let (s, d) = ((y * nw + x) * 4, ((oy + y) * WIDTH + ox + x) * 4);
            for c in 0..4 {
                out.px[d + c] = scaled[s + c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(out)
}

fn lanczos3(x: f64) -> f64 {
    let x = x.abs();
    if x < 1e-9 {
        1.0
    } else if x < 3.0 {
        let p = std::f64::consts::PI * x;
        3.0 * p.sin() * (p / 3.0).sin() / (p * p)
    } else {
        0.0
    }
}

/// Weights of the source texels for each target texel along one axis.
fn taps(from: usize, to: usize) -> Vec<(usize, Vec<f64>)> {
    let scale = from as f64 / to as f64;
    let support = 3.0 * scale.max(1.0);
    (0..to)
        .map(|i| {
            let centre = (i as f64 + 0.5) * scale;
            let lo = (centre - support).floor().max(0.0) as usize;
            let hi = ((centre + support).ceil() as usize).min(from);
            let mut w: Vec<f64> = (lo..hi)
                .map(|j| lanczos3((j as f64 + 0.5 - centre) / scale.max(1.0)))
                .collect();
            let sum: f64 = w.iter().sum();
            w.iter_mut().for_each(|v| *v /= sum);
            (lo, w)
        })
        .collect()
}

/// Lanczos-3 resize of an RGBA image (f64 channels 0-255), with
/// premultiplied alpha so that transparent texels do not bleed colour.
fn resize(src: &[f64], w: usize, h: usize, nw: usize, nh: usize) -> Vec<f64> {
    let pre: Vec<f64> = src
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] / 255.0;
            [p[0] * a, p[1] * a, p[2] * a, p[3]]
        })
        .collect();
    let mut rows = vec![0f64; nw * h * 4];
    let tx = taps(w, nw);
    for y in 0..h {
        for (x, (lo, wt)) in tx.iter().enumerate() {
            for (k, wk) in wt.iter().enumerate() {
                let s = (y * w + lo + k) * 4;
                let d = (y * nw + x) * 4;
                for c in 0..4 {
                    rows[d + c] += pre[s + c] * wk;
                }
            }
        }
    }
    let mut out = vec![0f64; nw * nh * 4];
    let ty = taps(h, nh);
    for (y, (lo, wt)) in ty.iter().enumerate() {
        for (k, wk) in wt.iter().enumerate() {
            for x in 0..nw {
                let s = ((lo + k) * nw + x) * 4;
                let d = (y * nw + x) * 4;
                for c in 0..4 {
                    out[d + c] += rows[s + c] * wk;
                }
            }
        }
    }
    for p in out.chunks_exact_mut(4) {
        let a = p[3].clamp(0.0, 255.0);
        for v in &mut p[..3] {
            *v = if a > 0.0 {
                (*v * 255.0 / a).clamp(0.0, 255.0)
            } else {
                LINE[0]
            };
        }
        p[3] = a;
    }
    out
}

/// The picture as the title stores it: BC7 blocks, tiled 64 KiB `D`.
pub fn texture(pic: &Rgba) -> Result<Vec<u8>, String> {
    if (pic.width, pic.height) != (WIDTH, HEIGHT) {
        return Err("the picture must be 1024x512".into());
    }
    let blocks = bc7::encode(&pic.px, WIDTH, HEIGHT);
    let desc = Desc {
        dimension: Dimension::Tex2d,
        width: (WIDTH / 4) as u32,
        height: (HEIGHT / 4) as u32,
        depth: 1,
        mips: 1,
        element_bytes: 16,
        block: 1,
        samples: 1,
        swizzle: Swizzle::SW_64KB_D,
    };
    let layout = Layout::new(&desc).map_err(|e| format!("texture layout: {e:?}"))?;
    let mut tiled = vec![0u8; TEXTURE_BYTES];
    let pitch = WIDTH / 4 * 16;
    layout
        .tile(&mut tiled, 0, 0, &blocks, pitch, blocks.len())
        .map_err(|e| format!("tiling: {e:?}"))?;
    Ok(tiled)
}

/// Where the texture's zlib stream starts in the title's file, and its
/// length: the first stream (at a 16-byte step) that inflates to exactly
/// the texture's size.
pub fn find_stream(file: &[u8]) -> Option<(usize, usize)> {
    (0..file.len().saturating_sub(2))
        .step_by(16)
        .find_map(|at| {
            let s = &file[at..];
            if s[0] != 0x78 {
                return None;
            }
            zlib_exact::inflate(s, TEXTURE_BYTES)
                .filter(|i| i.data.len() == TEXTURE_BYTES)
                .map(|i| (at, i.stream_len))
        })
}

/// The zlib stream for the new texture, `stream_len` bytes long.
pub fn pack(tiled: &[u8], stream_len: usize) -> Result<Vec<u8>, String> {
    zlib_exact::exact(tiled, stream_len, zlib_exact::fast)
        .ok_or_else(|| format!("the picture does not compress to {stream_len} bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Our own stand-in for the pack's drawing: a ring outline with a
    /// "logo" dot that only the full variant has.
    fn drawing(logo: bool) -> Rgba {
        let (w, h) = XELU_SIZE;
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = ((x as f64 - 726.0) / 650.0, (y as f64 - 470.0) / 420.0);
                let r = (dx * dx + dy * dy).sqrt();
                let on_ring = (r - 0.9).abs() < 0.008;
                let on_logo = logo && (700..760).contains(&x) && (570..600).contains(&y);
                if on_ring || on_logo {
                    px[(y * w + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        Rgba {
            width: w,
            height: h,
            px,
        }
    }

    #[test]
    fn the_picture_is_wide_filled_and_without_the_logo() {
        let pic = compose(&drawing(true), &drawing(false)).unwrap();
        let alpha = |x: usize, y: usize| pic.px[(y * WIDTH + x) * 4 + 3];
        assert_eq!(alpha(512, 255), 255, "the body is filled");
        assert_eq!(alpha(2, 255), 0, "outside stays clear");
        let rgb = &pic.px[(255 * WIDTH + 512) * 4..][..3];
        assert!(
            rgb.iter().all(|&c| c > 230),
            "no logo in the middle: {rgb:?}"
        );
        let cols: Vec<usize> = (0..WIDTH)
            .filter(|&x| (0..HEIGHT).any(|y| alpha(x, y) > 128))
            .collect();
        let rows: Vec<usize> = (0..HEIGHT)
            .filter(|&y| (0..WIDTH).any(|x| alpha(x, y) > 128))
            .collect();
        let (pw, ph) = (
            (cols[cols.len() - 1] - cols[0]) as f64,
            (rows[rows.len() - 1] - rows[0]) as f64,
        );
        // A 1300x840 ellipse shown at 0.748 of its texture width keeps its shape.
        assert!(
            ((pw * SQUEEZE / ph) - 1300.0 / 840.0).abs() < 0.05,
            "{pw} x {ph}"
        );
        let tiled = texture(&pic).unwrap();
        assert_eq!(tiled.len(), TEXTURE_BYTES);
        let mut file = vec![0u8; 0x1100];
        let target = 200_000;
        file.extend(pack(&tiled, target).unwrap());
        assert_eq!(find_stream(&file), Some((0x1100, target)));
    }
}

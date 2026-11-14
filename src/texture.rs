//! 8-bit textures (D6): downscale, median-cut palette, nearest-colour mapping,
//! and a minimal 8-bit BMP writer for the player thumbnail (D8).

use std::collections::HashMap;

use anyhow::{Context, Result};
use image::imageops::FilterType;

/// An indexed image: one palette index per pixel, 256 RGB palette entries.
pub struct Indexed {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub palette: Vec<[u8; 3]>,
}

impl Indexed {
    pub fn palette_bytes(&self) -> Vec<u8> {
        let mut p: Vec<u8> = self.palette.iter().flatten().copied().collect();
        p.resize(768, 0);
        p
    }
}

/// Decode `png`, fit it within `max` (sides a multiple of 8) and reduce to 256 colours.
pub fn goldsrc_texture(png: &[u8], max: u32) -> Result<Indexed> {
    goldsrc_texture_masked(png, max, false)
}

/// As `goldsrc_texture`; with `masked`, texels under half alpha become index
/// 255, which GoldSrc's masked flag draws as see-through (D6), and the
/// visible texels share the other 255 colours.
pub fn goldsrc_texture_masked(png: &[u8], max: u32, masked: bool) -> Result<Indexed> {
    let img = image::load_from_memory(png)
        .context("decoding texture")?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let scale = (max as f32 / w.max(h) as f32).min(1.0);
    let fit = |v: u32| (((v as f32 * scale) as u32) / 8 * 8).max(8);
    let (nw, nh) = (fit(w), fit(h));
    let img = if (nw, nh) != (w, h) {
        image::imageops::resize(&img, nw, nh, FilterType::Lanczos3)
    } else {
        img
    };
    if !masked {
        let px: Vec<[u8; 3]> = img.pixels().map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
        return Ok(quantise(&px, nw, nh));
    }
    let clear: Vec<bool> = img.pixels().map(|p| p.0[3] < 128).collect();
    let px: Vec<[u8; 3]> = img
        .pixels()
        .zip(&clear)
        .filter(|(_, c)| !**c)
        .map(|(p, _)| [p.0[0], p.0[1], p.0[2]])
        .collect();
    let mut q = quantise_n(&px, nw, nh, 255);
    q.palette.push([0, 0, 255]);
    let mut opaque = q.pixels.into_iter();
    q.pixels = clear
        .iter()
        .map(|&c| if c { 255 } else { opaque.next().unwrap() })
        .collect();
    Ok(q)
}

/// Median-cut quantisation to at most 256 colours (exact if the image has no more).
pub fn quantise(px: &[[u8; 3]], width: u32, height: u32) -> Indexed {
    quantise_n(px, width, height, 256)
}

/// `quantise` into `n` palette entries, all filled with distinct colours.
fn quantise_n(px: &[[u8; 3]], width: u32, height: u32, n: usize) -> Indexed {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in px {
        *counts.entry(*p).or_default() += 1;
    }
    let palette: Vec<[u8; 3]> = if counts.len() <= n {
        let mut c: Vec<[u8; 3]> = counts.keys().copied().collect();
        c.sort();
        c
    } else {
        // Sorted, so ties in the cut fall the same way every run.
        let mut c: Vec<([u8; 3], u32)> = counts.into_iter().collect();
        c.sort();
        median_cut(c, n)
    };
    // Sven treats a portrait palette under 224 colours as a remap image (A11),
    // so always fill every entry with a distinct colour.
    let mut palette = palette;
    let mut filler = (0..=255u8).flat_map(|r| [[r, r / 2, 255 - r], [255 - r, r, r / 3]]);
    while palette.len() < n {
        let c = filler.next().expect("enough filler colours");
        if !palette.contains(&c) && c != [0, 0, 255] {
            palette.push(c);
        }
    }
    let mut cache: HashMap<[u8; 3], u8> = HashMap::new();
    let pixels = px
        .iter()
        .map(|p| {
            *cache.entry(*p).or_insert_with(|| {
                let d = |q: &[u8; 3]| {
                    (0..3)
                        .map(|i| (p[i] as i32 - q[i] as i32).pow(2))
                        .sum::<i32>()
                };
                palette
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, q)| d(q))
                    .map(|(i, _)| i as u8)
                    .unwrap()
            })
        })
        .collect();
    Indexed {
        width,
        height,
        pixels,
        palette,
    }
}

fn median_cut(colours: Vec<([u8; 3], u32)>, n: usize) -> Vec<[u8; 3]> {
    let mut boxes = vec![colours];
    while boxes.len() < n {
        // Split the box with the widest channel range, weighted by pixel count.
        let pick = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .max_by_key(|(_, b)| {
                let (axis_range, _) = widest(b);
                axis_range as u64 * b.iter().map(|c| c.1 as u64).sum::<u64>()
            })
            .map(|(i, _)| i);
        let Some(i) = pick else { break };
        let mut b = boxes.swap_remove(i);
        let (_, axis) = widest(&b);
        b.sort_by_key(|c| c.0[axis]);
        let total: u64 = b.iter().map(|c| c.1 as u64).sum();
        let mut acc = 0;
        let mut cut = 1;
        for (k, c) in b.iter().enumerate() {
            acc += c.1 as u64;
            if acc * 2 >= total {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let w: u64 = b.iter().map(|c| c.1 as u64).sum();
            std::array::from_fn(|i| {
                ((b.iter().map(|c| c.0[i] as u64 * c.1 as u64).sum::<u64>() + w / 2) / w) as u8
            })
        })
        .collect()
}

fn widest(b: &[([u8; 3], u32)]) -> (u8, usize) {
    (0..3)
        .map(|i| {
            let lo = b.iter().map(|c| c.0[i]).min().unwrap();
            let hi = b.iter().map(|c| c.0[i]).max().unwrap();
            (hi - lo, i)
        })
        .max()
        .unwrap()
}

/// An uncompressed 8-bit BMP (bottom-up rows, 4-byte row padding).
pub fn bmp8(img: &Indexed) -> Vec<u8> {
    let row = (img.width as usize).div_ceil(4) * 4;
    let data = row * img.height as usize;
    let off = 14 + 40 + 1024;
    let mut b = Vec::with_capacity(off + data);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&((off + data) as u32).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(off as u32).to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(img.width as i32).to_le_bytes());
    b.extend_from_slice(&(img.height as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(data as u32).to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&256u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    for i in 0..256 {
        let c = img.palette.get(i).copied().unwrap_or([0; 3]);
        b.extend_from_slice(&[c[2], c[1], c[0], 0]);
    }
    for y in (0..img.height as usize).rev() {
        let s = y * img.width as usize;
        b.extend_from_slice(&img.pixels[s..s + img.width as usize]);
        b.resize(b.len() + row - img.width as usize, 0);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn few_colours_are_exact() {
        let px = vec![[10, 20, 30], [200, 0, 0], [10, 20, 30], [0, 0, 255]];
        let q = quantise(&px, 2, 2);
        for (p, &i) in px.iter().zip(&q.pixels) {
            assert_eq!(q.palette[i as usize], *p);
        }
        let distinct: std::collections::HashSet<_> = q.palette.iter().collect();
        assert_eq!(distinct.len(), 256);
    }

    #[test]
    fn many_colours_fit_256() {
        let px: Vec<[u8; 3]> = (0..4096u32)
            .map(|i| [(i % 256) as u8, (i / 16) as u8, (i * 7 % 256) as u8])
            .collect();
        let q = quantise(&px, 64, 64);
        assert!(q.palette.len() <= 256);
        assert_eq!(q.pixels.len(), 4096);
    }

    #[test]
    fn masked_keeps_index_255_for_clear_texels() {
        let mut img = image::RgbaImage::new(8, 8);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba(if x < 4 {
                [200, 10, 10, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let q = goldsrc_texture_masked(&png, 512, true).unwrap();
        assert_eq!(q.palette.len(), 256);
        assert_eq!(q.palette[255], [0, 0, 255]);
        for (i, &px) in q.pixels.iter().enumerate() {
            assert_eq!(px == 255, i % 8 >= 4, "texel {i}");
        }
        assert_eq!(q.palette[q.pixels[0] as usize], [200, 10, 10]);
    }

    #[test]
    fn bmp_size() {
        let img = Indexed {
            width: 164,
            height: 200,
            pixels: vec![0; 164 * 200],
            palette: vec![[0; 3]; 256],
        };
        assert_eq!(bmp8(&img).len(), 14 + 40 + 1024 + 164 * 200);
    }
}

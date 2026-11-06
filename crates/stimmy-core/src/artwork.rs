//! Artwork: a validated disk cache, plus the two derived images the desktop
//! design depends on, since GPUI has no element blur or mask.
//!
//! - `ambient`: the title's colour, blurred until no shape survives. The window
//!   is lit by this, so a dim ghost of a face never competes with the content.
//! - `matte`: the backdrop with a real alpha falloff, so it dissolves into
//!   whatever colour the ambient layer happens to be instead of a fixed one.

use crate::{
    client::StremioClient,
    error::{Error, Result},
    store::atomic_write,
};
use image::{
    DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage,
    imageops::{self, FilterType},
};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};
use url::Url;

const MAX_ARTWORK: u64 = 20 * 1024 * 1024;
pub const DEFAULT_CACHE_LIMIT: u64 = 800 * 1024 * 1024;

/// FNV-1a: stable across launches, so cache files survive a restart.
pub fn hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Clone)]
pub struct ArtworkCache {
    client: StremioClient,
    dir: PathBuf,
}

impl ArtworkCache {
    pub fn new(client: StremioClient, dir: PathBuf) -> Self {
        Self { client, dir }
    }
    fn file(&self, source: &str, suffix: &str) -> PathBuf {
        self.dir.join(format!("{:x}.{suffix}", hash(source)))
    }

    /// Downloads once. Anything that isn't an image is refused rather than
    /// cached, so a server's HTML error page can't become permanent "artwork".
    pub fn original(&self, source: &str) -> Result<PathBuf> {
        let url = Url::parse(source).map_err(|_| Error::BadArtwork)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::BadArtwork);
        }
        let target = self.file(source, "image");
        if target.is_file() {
            return Ok(target);
        }
        let response = self.client.http.get(url).send()?.error_for_status()?;
        let mut bytes = Vec::new();
        response
            .take(MAX_ARTWORK + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Offline)?;
        if bytes.len() as u64 > MAX_ARTWORK || image::guess_format(&bytes).is_err() {
            return Err(Error::BadArtwork);
        }
        atomic_write(&target, &bytes, false)?;
        Ok(target)
    }

    pub fn ambient(&self, source: &str) -> Result<PathBuf> {
        let target = self.file(source, "ambient.png");
        if target.is_file() {
            return Ok(target);
        }
        let image = decode(&self.original(source)?)?;
        write_png(&target, DynamicImage::ImageRgb8(ambient(&image)))?;
        Ok(target)
    }

    pub fn matte(&self, source: &str) -> Result<PathBuf> {
        let target = self.file(source, "matte.png");
        if target.is_file() {
            return Ok(target);
        }
        let image = decode(&self.original(source)?)?;
        write_png(&target, DynamicImage::ImageRgba8(matte(&image)))?;
        Ok(target)
    }

    /// Title art, made usable on a dark room: trimmed to its visible bounds so
    /// it aligns with text, and recoloured white if it's dark. Refused if it has
    /// no transparency, since a boxed logo looks wrong on a backdrop.
    pub fn logo(&self, source: &str) -> Result<PathBuf> {
        let target = self.file(source, "logo.png");
        if target.is_file() {
            return Ok(target);
        }
        let image = decode(&self.original(source)?)?;
        let logo = title_art(&image).ok_or(Error::BadArtwork)?;
        write_png(&target, DynamicImage::ImageRgba8(logo))?;
        Ok(target)
    }

    /// A title's signature colour, for accents. Saved as a small file beside the art.
    pub fn accent(&self, source: &str) -> Result<PathBuf> {
        let target = self.file(source, "accent");
        if target.is_file() {
            return Ok(target);
        }
        let image = decode(&self.original(source)?)?;
        atomic_write(&target, &accent(&image), false)?;
        Ok(target)
    }

    pub fn size(&self) -> u64 {
        entries(&self.dir).iter().map(|entry| entry.1).sum()
    }

    /// Removes least recently written files until the cache fits.
    pub fn evict(&self, limit: u64) -> Result<u64> {
        let mut files = entries(&self.dir);
        let mut total: u64 = files.iter().map(|entry| entry.1).sum();
        files.sort_by_key(|entry| entry.2);
        let mut removed = 0;
        for (path, size, _) in files {
            if total <= limit {
                break;
            }
            fs::remove_file(path)?;
            total -= size;
            removed += size;
        }
        Ok(removed)
    }

    pub fn clear(&self) -> Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

fn entries(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(read) = fs::read_dir(dir) else {
        return vec![];
    };
    read.flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                (
                    entry.path(),
                    meta.len(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
        })
        .collect()
}

fn decode(path: &Path) -> Result<DynamicImage> {
    // Cached originals have no meaningful extension, so sniff the contents.
    image::ImageReader::open(path)
        .map_err(|_| Error::BadArtwork)?
        .with_guessed_format()
        .map_err(|_| Error::BadArtwork)?
        .decode()
        .map_err(|_| Error::BadArtwork)
}

fn write_png(path: &Path, image: DynamicImage) -> Result<()> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|_| Error::BadArtwork)?;
    atomic_write(path, bytes.get_ref(), false)
}

/// Rec. 709 luma of an sRGB triple in 0–1, close enough for a brightness cap.
fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Saturate, then bring brightness into a band: dark enough that light text
/// holds contrast on top, bright enough that a dark poster still lights the room.
const AMBIENT_MAX_LUMA: f32 = 0.20;
const AMBIENT_MIN_PEAK: f32 = 0.11;
const AMBIENT_NEUTRAL_LUMA: f32 = 0.075;

fn grade(pixel: [f32; 3], gain: f32) -> [f32; 3] {
    let [r, g, b] = pixel;
    let grey = luma(r, g, b);
    let saturate = |c: f32| (grey + (c - grey) * 1.55).max(0.);
    let mut out = [saturate(r) * gain, saturate(g) * gain, saturate(b) * gain];
    // Colourless light would read as a flat grey slab, so it gets a lower ceiling.
    let (high, low) = (
        out[0].max(out[1]).max(out[2]),
        out[0].min(out[1]).min(out[2]),
    );
    let chroma = if high > 0. { (high - low) / high } else { 0. };
    let ceiling =
        AMBIENT_NEUTRAL_LUMA + (AMBIENT_MAX_LUMA - AMBIENT_NEUTRAL_LUMA) * (chroma * 2.).min(1.);
    let bright = luma(out[0], out[1], out[2]);
    if bright > ceiling {
        let scale = ceiling / bright;
        out = out.map(|c| c * scale);
    }
    out.map(|c| c.clamp(0., 1.))
}

/// A cheap, stable noise for dithering, so outputs are reproducible.
fn dither(x: u32, y: u32) -> f32 {
    let seed = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    let mixed = (seed ^ (seed >> 15)).wrapping_mul(0xC2B2_AE3D);
    ((mixed >> 16) & 0xFFFF) as f32 / 65535. - 0.5
}

pub fn ambient(source: &DynamicImage) -> RgbImage {
    const SMALL: (u32, u32) = (64, 36);
    const OUT: (u32, u32) = (360, 202);
    let small = source
        .resize_to_fill(SMALL.0, SMALL.1, FilterType::Triangle)
        .to_rgb32f();
    let peak = small
        .pixels()
        .map(|p| luma(p[0], p[1], p[2]))
        .fold(0f32, f32::max);
    // Near-black artwork is lifted toward visibility; a fully black image stays black.
    let gain = if peak > 0.01 && peak < AMBIENT_MIN_PEAK * 2. {
        (AMBIENT_MIN_PEAK * 2. / peak).min(4.)
    } else {
        1.
    };
    let graded = image::Rgb32FImage::from_fn(SMALL.0, SMALL.1, |x, y| {
        Rgb(grade(small.get_pixel(x, y).0, gain))
    });
    let blurred = imageops::blur(&graded, 5.);
    let large = imageops::resize(&blurred, OUT.0, OUT.1, FilterType::CatmullRom);
    let large = imageops::blur(&large, 7.);
    // Dark gradients band badly in eight bits. A little under one step of noise
    // is invisible as texture and removes the contour lines.
    RgbImage::from_fn(OUT.0, OUT.1, |x, y| {
        let p = large.get_pixel(x, y);
        let noise = dither(x, y) * 1.6;
        let channel = |c: f32| (c * 255. + noise).clamp(0., 255.).round() as u8;
        Rgb([channel(p[0]), channel(p[1]), channel(p[2])])
    })
}

pub fn title_art(source: &DynamicImage) -> Option<RgbaImage> {
    const DARK: f32 = 0.32;
    let image = source.to_rgba8();
    let (w, h) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (w, h, 0, 0);
    let (mut weight, mut lightness, mut clear) = (0f64, 0f64, 0u64);
    for (x, y, pixel) in image.enumerate_pixels() {
        let alpha = pixel[3] as f64 / 255.;
        if pixel[3] < 16 {
            clear += 1;
            continue;
        }
        (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
        weight += alpha;
        // Brightest channel, not luma: a saturated red logo reads fine on a dark room.
        let value = pixel[0].max(pixel[1]).max(pixel[2]) as f64 / 255.;
        lightness += alpha * value;
    }
    if weight == 0. || clear * 20 < (w as u64 * h as u64) {
        return None;
    }
    let mut out =
        imageops::crop_imm(&image, left, top, right - left + 1, bottom - top + 1).to_image();
    if ((lightness / weight) as f32) < DARK {
        for pixel in out.pixels_mut() {
            *pixel = Rgba([255, 255, 255, pixel[3]]);
        }
    }
    Some(out)
}

/// The artwork's most characteristic colour: an average weighted heavily toward
/// saturated, reasonably bright pixels, so a dark poster with a red logo gives red.
/// Colourless art gives a light neutral.
pub fn accent(source: &DynamicImage) -> [u8; 3] {
    let small = source
        .resize_to_fill(32, 18, FilterType::Triangle)
        .to_rgb32f();
    let (mut r, mut g, mut b, mut total) = (0f32, 0f32, 0f32, 0f32);
    for pixel in small.pixels() {
        let high = pixel[0].max(pixel[1]).max(pixel[2]);
        let low = pixel[0].min(pixel[1]).min(pixel[2]);
        let saturation = if high > 0. { (high - low) / high } else { 0. };
        let weight = saturation * saturation * high + 1e-4;
        r += pixel[0] * weight;
        g += pixel[1] * weight;
        b += pixel[2] * weight;
        total += weight;
    }
    [r, g, b].map(|channel| ((channel / total).clamp(0., 1.) * 255.) as u8)
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = ((value - from) / (to - from)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Bottom fades out completely; the lower left, where title and actions sit,
/// fades further, so text is on the room's light rather than on image detail.
pub fn matte(source: &DynamicImage) -> RgbaImage {
    const WIDTH: u32 = 1600;
    let (w, h) = source.dimensions();
    let resized = if w > WIDTH {
        source.resize(WIDTH, u32::MAX, FilterType::Triangle)
    } else {
        source.clone()
    };
    let mut out = resized.to_rgba8();
    let (w, h) = (out.width().max(1), out.height().max(1).max(h.min(1)));
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let u = x as f32 / w as f32;
        let v = y as f32 / h as f32;
        let bottom = 1. - smoothstep(0.42, 1.0, v);
        let side = 1. - (1. - smoothstep(0.0, 0.7, u)) * smoothstep(0.12, 0.6, v) * 0.96;
        let Rgba([r, g, b, a]) = *pixel;
        let alpha = (a as f32) * bottom * side;
        *pixel = Rgba([r, g, b, alpha.round() as u8]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageBuffer;

    fn solid(r: u8, g: u8, b: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(200, 120, Rgb([r, g, b])))
    }

    #[test]
    fn ambient_never_exceeds_the_brightness_text_depends_on() {
        for image in [
            solid(255, 255, 255),
            solid(255, 240, 10),
            solid(250, 30, 30),
        ] {
            let out = ambient(&image);
            for p in out.pixels() {
                let l = luma(p[0] as f32 / 255., p[1] as f32 / 255., p[2] as f32 / 255.);
                assert!(l <= AMBIENT_MAX_LUMA + 0.02, "luma {l}");
            }
        }
    }

    #[test]
    fn ambient_keeps_hue_and_lifts_dark_artwork() {
        let red = ambient(&solid(180, 20, 20));
        let p = red.get_pixel(100, 100);
        assert!(p[0] > p[1] * 3 && p[0] > p[2] * 3, "{p:?}");
        let dark = ambient(&solid(30, 10, 10));
        let d = dark.get_pixel(100, 100);
        assert!(d[0] > 30, "dark artwork should be lifted: {d:?}");
        let black = ambient(&solid(0, 0, 0));
        assert!(black.get_pixel(100, 100)[0] <= 1, "black stays black");
    }

    #[test]
    fn matte_is_opaque_at_top_right_and_gone_at_the_bottom() {
        let out = matte(&solid(100, 100, 100));
        let (w, h) = out.dimensions();
        assert_eq!(out.get_pixel(w - 1, 0)[3], 255);
        assert_eq!(out.get_pixel(w / 2, h - 1)[3], 0);
        let lower_left = out.get_pixel(0, h * 6 / 10)[3];
        let lower_right = out.get_pixel(w - 1, h * 6 / 10)[3];
        assert!(
            lower_left < lower_right / 2,
            "{lower_left} vs {lower_right}"
        );
    }

    #[test]
    fn title_art_trims_whitens_dark_logos_and_refuses_boxed_ones() {
        let mut dark = RgbaImage::from_pixel(100, 40, Rgba([0, 0, 0, 0]));
        for x in 20..60 {
            for y in 10..20 {
                dark.put_pixel(x, y, Rgba([10, 10, 10, 255]));
            }
        }
        let out = title_art(&DynamicImage::ImageRgba8(dark)).unwrap();
        assert_eq!(out.dimensions(), (40, 10), "trimmed to visible bounds");
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255], "whitened");

        let mut colour = RgbaImage::from_pixel(100, 40, Rgba([0, 0, 0, 0]));
        colour.put_pixel(5, 5, Rgba([220, 40, 40, 255]));
        let kept = title_art(&DynamicImage::ImageRgba8(colour)).unwrap();
        assert_eq!(
            kept.get_pixel(0, 0).0,
            [220, 40, 40, 255],
            "bright art keeps its colour"
        );

        assert!(
            title_art(&solid(200, 200, 200)).is_none(),
            "opaque box refused"
        );
    }

    #[test]
    fn neutral_artwork_gets_a_darker_light_than_coloured() {
        let neutral = ambient(&solid(235, 235, 235));
        let coloured = ambient(&solid(235, 60, 40));
        let l = |p: &Rgb<u8>| luma(p[0] as f32 / 255., p[1] as f32 / 255., p[2] as f32 / 255.);
        let neutral = l(neutral.get_pixel(100, 100));
        assert!(neutral <= AMBIENT_NEUTRAL_LUMA + 0.02, "{neutral}");
        assert!(neutral < l(coloured.get_pixel(100, 100)));
    }

    #[test]
    fn accents_find_the_colour_even_in_mostly_dark_or_neutral_art() {
        let mut art = RgbImage::from_pixel(200, 120, Rgb([12, 12, 12]));
        for x in 60..100 {
            for y in 40..80 {
                art.put_pixel(x, y, Rgb([210, 30, 30]));
            }
        }
        let [r, g, b] = accent(&DynamicImage::ImageRgb8(art));
        assert!(r > 120 && r > g * 3 && r > b * 3, "{r} {g} {b}");
        let [r, g, b] = accent(&solid(200, 200, 200));
        assert!(
            r.abs_diff(g) < 5 && g.abs_diff(b) < 5,
            "neutral stays neutral"
        );
    }

    #[test]
    fn eviction_removes_oldest_until_within_limit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtworkCache::new(StremioClient::new().unwrap(), dir.path().into());
        for (index, name) in ["a", "b", "c"].iter().enumerate() {
            let path = dir.path().join(name);
            fs::write(&path, vec![0u8; 100]).unwrap();
            let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + index as u64);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }
        assert_eq!(cache.size(), 300);
        assert_eq!(cache.evict(150).unwrap(), 200);
        assert!(dir.path().join("c").exists());
        assert!(!dir.path().join("a").exists());
    }
}

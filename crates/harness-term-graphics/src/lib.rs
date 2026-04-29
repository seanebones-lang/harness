//! Terminal inline image rendering for Harness.
//!
//! Supports three backends (auto-detected from environment):
//! - **Kitty**: `TERM=xterm-kitty` or `TERM_PROGRAM=kitty` — uses APC escape sequences
//! - **iTerm2**: `TERM_PROGRAM=iTerm.app` — uses DCS escape sequences  
//! - **Sixel**: `TERM` contains `sixel` or `VTE_VERSION` is set — uses DCS Sixel protocol
//!
//! # Usage
//! ```rust,no_run
//! use harness_term_graphics::{display_image, Backend};
//! let backend = Backend::detect();
//! display_image("screenshot.png", 80, 24, backend)?;
//! ```

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use image::imageops::FilterType;

// ── Backend detection ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    /// Kitty terminal graphics protocol.
    Kitty,
    /// iTerm2 inline image protocol.
    ITerm2,
    /// Sixel graphics.
    Sixel,
    /// No graphics support (fallback: show filepath).
    None,
}

impl Backend {
    /// Auto-detect best available terminal graphics backend.
    pub fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let term_gi = std::env::var("TERM_GRAPHICS_ID").unwrap_or_default();

        if term.contains("kitty") || term_program.to_lowercase().contains("kitty") || !term_gi.is_empty() {
            return Backend::Kitty;
        }
        if term_program.contains("iTerm") {
            return Backend::ITerm2;
        }
        if term.contains("sixel") || std::env::var("VTE_VERSION").is_ok() {
            return Backend::Sixel;
        }
        Backend::None
    }

    pub fn supported(&self) -> bool {
        !matches!(self, Backend::None)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Render an image file inline in the terminal.
/// `max_cols` and `max_rows` are the maximum terminal cell dimensions.
pub fn display_image(path: &str, max_cols: u32, max_rows: u32, backend: Backend) -> Result<()> {
    let img = image::open(path).context("loading image")?;
    // Resize to fit terminal (assume ~8px per row, ~4px per col for cell size)
    let target_w = max_cols * 8;
    let target_h = max_rows * 16;
    let img = img.resize(target_w, target_h, FilterType::Lanczos3);

    match backend {
        Backend::Kitty => render_kitty(&img),
        Backend::ITerm2 => render_iterm2(&img, path),
        Backend::Sixel => render_sixel(&img),
        Backend::None => {
            println!("[image: {path}]");
            Ok(())
        }
    }
}

/// Render raw image bytes (PNG or JPEG) inline.
pub fn display_image_bytes(data: &[u8], max_cols: u32, max_rows: u32, backend: Backend) -> Result<()> {
    let img = image::load_from_memory(data).context("decoding image")?;
    let target_w = max_cols * 8;
    let target_h = max_rows * 16;
    let img = img.resize(target_w, target_h, FilterType::Lanczos3);

    match backend {
        Backend::Kitty => render_kitty(&img),
        Backend::ITerm2 => render_iterm2_bytes(data),
        Backend::Sixel => render_sixel(&img),
        Backend::None => {
            println!("[image {} bytes]", data.len());
            Ok(())
        }
    }
}

// ── Kitty protocol ────────────────────────────────────────────────────────────

fn render_kitty(img: &image::DynamicImage) -> Result<()> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let raw = rgba.as_raw();
    let b64_data = B64.encode(raw);

    // Kitty APC: \x1b_Ga=T,f=32,s=W,v=H,m=1;<chunk>\x1b\\
    // Send in chunks of ≤4096 chars (base64)
    let chunk_size = 4096;
    let chunks: Vec<&str> = b64_data.as_bytes()
        .chunks(chunk_size)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();

    let total = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = if i + 1 < total { 1 } else { 0 };
        if i == 0 {
            print!("\x1b_Ga=T,f=32,s={w},v={h},m={more};{chunk}\x1b\\");
        } else {
            print!("\x1b_Gm={more};{chunk}\x1b\\");
        }
    }
    println!();
    Ok(())
}

// ── iTerm2 protocol ───────────────────────────────────────────────────────────

fn render_iterm2(img: &image::DynamicImage, _name: &str) -> Result<()> {
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)?;
    render_iterm2_bytes(&buf)
}

fn render_iterm2_bytes(data: &[u8]) -> Result<()> {
    let b64 = B64.encode(data);
    let len = data.len();
    // iTerm2: \x1b]1337;File=inline=1;size=LEN;<base64>\x07
    print!("\x1b]1337;File=inline=1;size={len}:{b64}\x07");
    println!();
    Ok(())
}

// ── Sixel protocol ────────────────────────────────────────────────────────────

fn render_sixel(img: &image::DynamicImage) -> Result<()> {
    // Sixel: DCS P1;P2;P3 q <sixel data> ST
    // Simple implementation: convert to 256-color palette and encode
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();

    // Quantize to 16 colors (simplified — a full implementation would use median-cut)
    let palette = build_simple_palette(&rgb);
    let indexed = quantize_to_palette(&rgb, &palette);

    // Encode as Sixel
    let mut sixel = String::new();
    sixel.push_str("\x1bPq"); // DCS q (sixel introducer)

    // Define palette colors
    for (i, (r, g, b)) in palette.iter().enumerate() {
        let r_pct = (*r as u32 * 100) / 255;
        let g_pct = (*g as u32 * 100) / 255;
        let b_pct = (*b as u32 * 100) / 255;
        sixel.push_str(&format!("#{i};2;{r_pct};{g_pct};{b_pct}"));
    }

    // Encode rows (6 rows of pixels per sixel band)
    for band_y in 0..(h / 6 + 1) {
        for color_idx in 0..palette.len() {
            let mut any = false;
            let mut row_str = format!("#{color_idx}");
            for x in 0..w {
                let mut sixel_char = 0u8;
                for bit in 0..6 {
                    let y = band_y * 6 + bit;
                    if y < h {
                        let pixel_idx = (y * w + x) as usize;
                        if pixel_idx < indexed.len() && indexed[pixel_idx] == color_idx as u8 {
                            sixel_char |= 1 << bit;
                            any = true;
                        }
                    }
                }
                row_str.push((b'?' + sixel_char) as char);
            }
            if any {
                sixel.push_str(&row_str);
            }
        }
        sixel.push('-'); // Next band (CR+NL in sixel)
    }

    sixel.push_str("\x1b\\"); // ST
    print!("{sixel}");
    println!();
    Ok(())
}

fn build_simple_palette(img: &image::RgbImage) -> Vec<(u8, u8, u8)> {
    // Sample 16 representative colors by dividing the color cube
    let mut palette = Vec::new();
    for r in &[0u8, 85, 170, 255] {
        for g in &[0u8, 85, 170, 255] {
            palette.push((*r, *g, 128u8));
        }
    }
    palette.truncate(16);
    let _ = img;
    palette
}

fn quantize_to_palette(img: &image::RgbImage, palette: &[(u8, u8, u8)]) -> Vec<u8> {
    img.pixels().map(|p| {
        let (r, g, b) = (p[0], p[1], p[2]);
        palette.iter().enumerate().min_by_key(|(_, (pr, pg, pb))| {
            let dr = (*pr as i32 - r as i32).abs();
            let dg = (*pg as i32 - g as i32).abs();
            let db = (*pb as i32 - b as i32).abs();
            dr + dg + db
        }).map(|(i, _)| i as u8).unwrap_or(0)
    }).collect()
}

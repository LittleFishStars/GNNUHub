//! 用真实样本验证位图字库识别效果。
//!
//! 用法：
//! ```bash
//! cargo run -p gnnuhub-ocr --example bench_bitmap -- captcha_samples
//! ```
//!
//! 输出：字库大小、字形级命中率、整图级命中率、以及未命中的明细。

use std::path::PathBuf;

use base64::Engine as _;
use gnnuhub_ocr::{BitmapLibrary, bitmap::extract_glyphs_for_bench};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "captcha_samples".into()),
    );
    let lib_path = dir.join("bitmap_lib.json");

    let lib = BitmapLibrary::from_path(&lib_path)?;
    println!("字库：{} 条（{}）", lib.len(), lib_path.display());

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();

    let (mut glyph_total, mut glyph_hit) = (0usize, 0usize);
    let (mut img_ok, mut img_total) = (0usize, 0usize);
    let (mut exact, mut fuzzy) = (0usize, 0usize);
    let mut misses = Vec::new();

    for f in &files {
        let bytes = std::fs::read(f)?;
        let b64 = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        let glyphs = extract_glyphs_for_bench(&b64)?;
        img_total += 1;

        let mut code = String::new();
        let mut all_hit = true;
        for g in &glyphs {
            glyph_total += 1;
            if let Some(c) = lib.lookup_exact(g.w, g.h, &g.bits) {
                exact += 1;
                glyph_hit += 1;
                code.push(c);
            } else if let Some((c, d)) = lib.lookup_fuzzy(g.w, g.h, &g.bits, 2) {
                fuzzy += 1;
                glyph_hit += 1;
                code.push(c);
                misses.push(format!(
                    "{} {}x{} 模糊命中 {:?}（距离 {}）",
                    f.file_name().unwrap().to_string_lossy(),
                    g.w,
                    g.h,
                    c,
                    d
                ));
            } else {
                all_hit = false;
                code.push('?');
                misses.push(format!(
                    "{} {}x{} 未命中",
                    f.file_name().unwrap().to_string_lossy(),
                    g.w,
                    g.h
                ));
            }
        }
        if all_hit {
            img_ok += 1;
        }
        println!("  {} -> {}", f.file_name().unwrap().to_string_lossy(), code);
    }

    println!();
    println!(
        "字形级：{}/{} = {:.1}%（精确 {}，模糊 {}）",
        glyph_hit,
        glyph_total,
        glyph_hit as f64 / glyph_total.max(1) as f64 * 100.0,
        exact,
        fuzzy
    );
    println!(
        "整图级：{}/{} = {:.1}%",
        img_ok,
        img_total,
        img_ok as f64 / img_total.max(1) as f64 * 100.0
    );
    if !misses.is_empty() {
        println!("\n未精确命中的字形：");
        for m in &misses {
            println!("  {m}");
        }
    }
    Ok(())
}

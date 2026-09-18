//! 用真实样本验证位图字库识别效果。
//!
//! 用法：
//! ```bash
//! cargo run -p gnnuhub-ocr --example bench_bitmap -- captcha_samples
//! ```
//!
//! 输出：字库大小、字形级命中率、整图级命中率、以及未命中的明细。
//!
//! # 字库来源
//!
//! 默认用 **crate 内嵌的资产库**（`logic/ocr/assets/bitmap_lib.json`），
//! 也就是真正出货给用户的那个。
//!
//! 这一点是刻意的：早期版本读的是样本目录下的 `captcha_samples/bitmap_lib.json`，
//! 那是构建字库时留下的**副本**。结果改完资产库、跑基准却发现「改动没生效」，
//! 白白绕了一大圈排查。**基准必须测出货产物，不能测中间副本。**
//!
//! 若要做对照（例如评估「重新生成的字库比出货版好多少」），用第二个位置参数
//! 显式给一个路径：
//!
//! ```bash
//! cargo run -p gnnuhub-ocr --example bench_bitmap -- captcha_samples .scratch/new_lib.json
//! ```

use std::path::PathBuf;

use base64::Engine as _;
use gnnuhub_ocr::{BitmapLibrary, bitmap::extract_glyphs_for_bench};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "captcha_samples".into()),
    );

    // 默认内嵌资产库；显式传第二个参数才读外部文件做对照。
    let (lib, source) = match std::env::args().nth(2) {
        Some(p) => {
            let path = PathBuf::from(&p);
            let lib = BitmapLibrary::from_path(&path)?;
            let src = path.display().to_string();
            (lib, src)
        }
        None => (gnnuhub_ocr::embedded_library()?, "内嵌资产库".to_string()),
    };
    println!("字库：{} 条（{source}）", lib.len());

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

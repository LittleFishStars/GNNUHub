//! 导出 Rust 提取器对样本图片的字形点阵，用于与 Python 参考实现比对。
//!
//! 用法：
//! ```bash
//! cargo run -p gnnuhub-ocr --example dump_glyphs -- captcha_samples /tmp/rust_glyphs.json
//! ```
//!
//! 输出 JSON 数组，每项包含 `src` / `slot` / `w` / `h` / `rows`。

use std::path::PathBuf;

use base64::Engine as _;
use gnnuhub_ocr::bitmap::extract_glyphs_for_bench;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "captcha_samples".into()));
    let out = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "/tmp/rust_glyphs.json".into()),
    );

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();

    let mut all = Vec::new();
    let mut bad = 0usize;
    for f in &files {
        let bytes = std::fs::read(f)?;
        let b64 = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        match extract_glyphs_for_bench(&b64) {
            Ok(gs) => {
                if gs.len() != 4 {
                    eprintln!("!! {} 提取到 {} 个字形", f.display(), gs.len());
                    bad += 1;
                }
                for (i, g) in gs.iter().enumerate() {
                    let rows: Vec<String> = (0..g.h)
                        .map(|r| {
                            g.bits[r as usize * g.w as usize..(r as usize + 1) * g.w as usize]
                                .to_string()
                        })
                        .collect();
                    all.push(serde_json::json!({
                        "src": f.file_name().unwrap().to_string_lossy(),
                        "slot": i,
                        "w": g.w,
                        "h": g.h,
                        "rows": rows,
                    }));
                }
            }
            Err(e) => {
                eprintln!("!! {} 提取失败: {e}", f.display());
                bad += 1;
            }
        }
    }

    std::fs::write(&out, serde_json::to_string_pretty(&all)?)?;
    println!(
        "写入 {} 条字形到 {}（{} 张图，异常 {}）",
        all.len(),
        out.display(),
        files.len(),
        bad
    );
    Ok(())
}

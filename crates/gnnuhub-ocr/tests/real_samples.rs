//! 用真实样本回归位图识别引擎。
//!
//! 这些测试依赖 `captcha_samples/`（仓库内的真实抓取样本，共 60 张）
//! 与由它构建的 `captcha_samples/bitmap_lib.json`。
//!
//! 之所以把它做成集成测试而不是单元测试：字库是「数据」，不是「代码」，
//! 一旦字库被重新生成或误改，这里会立刻发现。

use std::path::{Path, PathBuf};

use base64::Engine as _;
use gnnuhub_ocr::{BitmapLibrary, bitmap::extract_glyphs_for_bench};

fn repo_root() -> PathBuf {
    // crates/gnnuhub-ocr -> 仓库根
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("应有仓库根目录")
        .to_path_buf()
}

fn encode(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("读取样本");
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    )
}

#[test]
fn embedded_library_is_complete() {
    let lib = gnnuhub_ocr::embedded_library().expect("内嵌字库应能解析");
    assert!(
        lib.len() >= 60,
        "内嵌字库条目过少（{}），assets 可能被截断",
        lib.len()
    );
}

#[test]
fn recognizes_all_samples_with_embedded_library() {
    let dir = repo_root().join("captcha_samples");
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect(),
        Err(_) => {
            eprintln!("跳过：{} 不存在", dir.display());
            return;
        }
    };
    files.sort();

    let engine = gnnuhub_ocr::BitmapOcr::embedded().expect("内嵌字库");
    let mut ok = 0usize;
    for f in &files {
        if let Ok(code) = engine.recognize_image(&encode(f)) {
            assert_eq!(code.chars().count(), 4, "{} 应识别出 4 个字符", f.display());
            ok += 1;
        }
    }
    assert_eq!(
        ok,
        files.len(),
        "用内嵌字库应识别全部样本（{ok}/{}）",
        files.len()
    );
}

#[test]
fn library_loads_and_covers_expected_charset() {
    let lib_path = repo_root().join("captcha_samples/bitmap_lib.json");
    if !lib_path.exists() {
        eprintln!("跳过：{} 不存在", lib_path.display());
        return;
    }
    let lib = BitmapLibrary::from_path(&lib_path).expect("字库应能加载");
    assert!(lib.len() >= 60, "字库条目过少（{}），可能被截断", lib.len());

    // 抽样确认关键字符（含大小写区分与易混字符）都在
    let text = std::fs::read_to_string(&lib_path).expect("读字库");
    for ch in ["I", "l", "O", "o", "q", "p", "L", "Z", "z"] {
        assert!(
            text.contains(&format!("\"label\": \"{ch}\"")),
            "字库应包含字符 {ch:?}"
        );
    }
}

#[test]
fn extracts_four_glyphs_from_every_real_sample() {
    let dir = repo_root().join("captcha_samples");
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect(),
        Err(_) => {
            eprintln!("跳过：{} 不存在", dir.display());
            return;
        }
    };
    files.sort();
    assert!(!files.is_empty(), "样本目录不应为空");

    for f in &files {
        let glyphs = extract_glyphs_for_bench(&encode(f))
            .unwrap_or_else(|e| panic!("{} 提取失败: {e}", f.display()));
        assert_eq!(
            glyphs.len(),
            4,
            "{} 应提取到 4 个字形，实际 {}",
            f.display(),
            glyphs.len()
        );
    }
}

#[test]
fn recognizes_all_real_samples_exactly() {
    let root = repo_root();
    let lib_path = root.join("captcha_samples/bitmap_lib.json");
    let dir = root.join("captcha_samples");
    if !lib_path.exists() {
        eprintln!("跳过：字库不存在");
        return;
    }
    let lib = BitmapLibrary::from_path(&lib_path).expect("字库应能加载");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("样本目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();

    let (mut glyph_total, mut glyph_hit, mut img_ok) = (0usize, 0usize, 0usize);
    let mut failures = Vec::new();

    for f in &files {
        let glyphs = extract_glyphs_for_bench(&encode(f)).expect("提取");
        let mut all = true;
        for g in &glyphs {
            glyph_total += 1;
            match lib.lookup_exact(g.w, g.h, &g.bits) {
                Some(_) => glyph_hit += 1,
                None => {
                    all = false;
                    failures.push(format!(
                        "{} {}x{} 未命中",
                        f.file_name().unwrap().to_string_lossy(),
                        g.w,
                        g.h
                    ));
                }
            }
        }
        if all {
            img_ok += 1;
        }
    }

    assert_eq!(
        glyph_hit,
        glyph_total,
        "字形应全部精确命中，未命中：{:?}",
        &failures[..failures.len().min(10)]
    );
    assert_eq!(
        img_ok,
        files.len(),
        "整图应全部识别成功（{img_ok}/{}）",
        files.len()
    );
}

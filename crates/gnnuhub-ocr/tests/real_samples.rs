//! 用真实样本回归位图识别引擎。
//!
//! 这些测试依赖 `captcha_samples/`（仓库内的真实抓取样本，共 60 张）
//! 与由它构建的 `captcha_samples/bitmap_lib.json`。
//!
//! 之所以把它做成集成测试而不是单元测试：字库是「数据」，不是「代码」，
//! 一旦字库被重新生成或误改，这里会立刻发现。

use std::path::{Path, PathBuf};

use base64::Engine as _;
use gnnuhub_ocr::bitmap::extract_glyphs_for_bench;

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
    let lib = gnnuhub_ocr::embedded_library().expect("内嵌字库应能加载");
    assert!(lib.len() >= 60, "字库条目过少（{}），可能被截断", lib.len());

    // 抽样确认关键字符（含大小写区分与易混字符）都在
    let labels: std::collections::HashSet<char> = lib.labels().collect();
    for ch in ['I', 'l', 'O', 'o', 'q', 'p', 'L', 'Z', 'z'] {
        assert!(labels.contains(&ch), "字库应包含字符 {ch:?}");
    }
}

/// 数字 `0` 与 `1` 必须在字库里，且**不能**被大写 `O` / `L` 顶替
///
/// # 为什么单列一条测试
///
/// 真值验证（`examples/ocr_verify.rs`）实测发现：字库曾把 `0` 标成 `O`、
/// 把 `1` 标成 `L`，导致**只要真值里含 `0` 或 `1` 就必然读错**。当时 20 轮
/// 里错的 3 轮，每一轮都恰好含一个被误标的字符，且 17 轮全对里这两个字符
/// 出现 0 次——相关性 100%。
///
/// 这条测试守住四个字符的**共存**，因为丢掉任一个都会静默退化成 85% 的
/// 准确率，而字形级指标仍是 100% 命中，从指标上看不出来。
#[test]
fn library_contains_digit_zero_and_one() {
    let lib = gnnuhub_ocr::embedded_library().expect("内嵌字库应能加载");
    let labels: std::collections::HashSet<char> = lib.labels().collect();

    for ch in ['0', '1', 'O', 'L', 'I', 'l'] {
        assert!(labels.contains(&ch), "字库应包含字符 {ch:?}");
    }

    // 数字全在：0-9 一个都不能少
    for d in '0'..='9' {
        assert!(labels.contains(&d), "字库应包含数字 {d:?}");
    }
}

/// 内嵌字库必须覆盖 [`CAPTCHA_ALPHABET`] 的每一个字符
///
/// # 这条测试为什么重要
///
/// 字库漏字符是**静默失败**：字形级命中率可能仍是 100%（因为缺的那个字符
/// 根本没被试到），只有整图准确率会掉，而且掉得不明显。实测就发生过：
/// 字库漏了 `0` 和 `1`，字形级指标 100%、整图却只有 85%。
///
/// 由于字库是采样得来的，漏字符是正常现象，所以这里断言的是「字库 ⊇ 字母表」
/// 这一**不变式**——字母表是本项目对服务端字符集的承诺，字库必须兑现它。
#[test]
fn embedded_library_covers_whole_alphabet() {
    let lib = gnnuhub_ocr::embedded_library().expect("内嵌字库应能解析");
    let labels: std::collections::HashSet<char> = lib.labels().collect();

    let missing: Vec<char> = gnnuhub_ocr::CAPTCHA_ALPHABET
        .iter()
        .copied()
        .filter(|c| !labels.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "内嵌字库缺少字母表中的字符 {missing:?}（共 {} 个字符、{} 条字形）",
        labels.len(),
        lib.len()
    );
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
    let dir = repo_root().join("captcha_samples");
    // 用**内嵌资产库**——那才是出货产物。早期这里读样本目录下的副本，
    // 导致改完资产库、跑测试却「没生效」，白绕一圈。
    let lib = gnnuhub_ocr::embedded_library().expect("内嵌字库应能加载");

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

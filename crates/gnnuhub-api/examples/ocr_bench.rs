//! 在本地样本上跑识别引擎，输出结果供比对
//!
//! 纯离线：不访问网络，只读 `captcha_samples/` 下的 PNG。
//!
//! # 运行
//!
//! ```bash
//! cargo run -p gnnuhub-api --example ocr_bench --features gnnuhub-ocr/tesseract -- captcha_samples
//! ```
//!
//! # 用途
//!
//! 1. 核对 Rust 实现与离线 Python 分析结论是否一致
//! 2. 统计识别覆盖率（能读出合法 4 字符的比例）
//! 3. 为「用登录接口验真值」筛选出可用样本
//!
//! 结果写入 `<dir>/bench.json`。

use std::path::PathBuf;

use base64::Engine as _;
use gnnuhub_ocr::{OcrEngine, TesseractOcr};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("gnnuhub_ocr=debug")
        .with_target(false)
        .init();

    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "captcha_samples".into()),
    );

    let engine = TesseractOcr::new();
    if !engine.is_available() {
        eprintln!("未检测到 tesseract，无法运行");
        std::process::exit(1);
    }

    // 收集所有 png，按文件名排序保证可复现
    let mut pngs: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();

    let mut entries = Vec::new();
    let mut ok = 0usize;

    for path in &pngs {
        let bytes = std::fs::read(path)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let started = std::time::Instant::now();
        // 不带回调：识别不出来就直接失败，方便统计真实覆盖率
        let result = engine.recognize(&b64, None);
        let elapsed = started.elapsed();

        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        match result {
            Ok(code) => {
                ok += 1;
                println!("{name} -> {code}  ({}ms)", elapsed.as_millis());
                entries.push(serde_json::json!({
                    "file": name,
                    "code": code,
                    "ms": elapsed.as_millis() as u64,
                }));
            }
            Err(e) => {
                println!("{name} -> 失败: {e}");
                entries.push(serde_json::json!({
                    "file": name,
                    "code": serde_json::Value::Null,
                    "error": e.to_string(),
                }));
            }
        }
    }

    println!();
    println!("识别成功: {ok}/{}", pngs.len());
    println!(
        "覆盖率: {:.1}%",
        ok as f64 / pngs.len().max(1) as f64 * 100.0
    );

    let out = dir.join("bench.json");
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&serde_json::json!({
            "total": pngs.len(),
            "ok": ok,
            "entries": entries,
        }))?,
    )?;
    println!("结果已写入 {}", out.display());

    Ok(())
}

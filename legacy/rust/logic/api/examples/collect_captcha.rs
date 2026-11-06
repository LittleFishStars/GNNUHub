//! 采集验证码样本，用于构建模板匹配字型库
//!
//! 这是**唯一**会为验证码发网络请求的工具，采集完成后全部识别工作
//! 都在离线进行。
//!
//! 与 `tools/captcha-collector/collect.py` 的区别：本工具复用
//! [`Client::throttled`] 的节流策略，请求节奏与 SDK 其他部分一致，
//! 不会因为脚本被单独使用而超发。
//!
//! # 运行
//!
//! ```bash
//! cargo run -p gnnuhub-api --example collect_captcha -- 60 captcha_samples
//! ```
//!
//! 参数依次为「采集数量」与「输出目录」，均可省略。
//!
//! # 产物
//!
//! - `NNNN.png`：验证码原图
//! - `NNNN.json`：该图的 uid 与元信息
//!
//! 不需要人工标注——识别器的字型库由 [`captcha_fonts`] 示例在
//! 这些样本上自动聚类得到。

use std::path::PathBuf;
use std::time::Duration;

use gnnuhub_api::Client;

/// 相邻请求的最小间隔
///
/// 取 1 秒，比 [`ClientConfig`] 的默认值更保守。验证码接口虽然不像
/// 登录那样敏感，但同样走前置 WAF，没必要压着上限打。
const INTERVAL: Duration = Duration::from_secs(1);

/// 单次采集的硬上限
const MAX_COUNT: usize = 200;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("gnnuhub_api=warn")
        .with_target(false)
        .init();

    let mut args = std::env::args().skip(1);
    let count: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(60);
    let out_dir = PathBuf::from(args.next().unwrap_or_else(|| "captcha_samples".into()));

    if count > MAX_COUNT {
        eprintln!("单次采集上限 {MAX_COUNT} 张，已收敛为 {MAX_COUNT}");
    }
    let count = count.min(MAX_COUNT);
    std::fs::create_dir_all(&out_dir)?;

    // 从已有样本数推算起始序号，支持断点续采
    let start = std::fs::read_dir(&out_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .count();
    if start > 0 {
        println!("检测到已有 {start} 张样本，从 {start} 继续");
    }

    println!(
        "采集 {count} 张，间隔 {} 秒，输出到 {}/",
        INTERVAL.as_secs(),
        out_dir.display()
    );

    let client = Client::with_defaults()?;
    let mut saved = 0usize;

    for i in start..start + count {
        // 采集本身不需要登录，直接打验证码接口即可
        let captcha = match gnnuhub_api::login::fetch_captcha(&client).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[{i:04}] 采集失败: {e}");
                continue;
            }
        };

        // 存原图而非 data URL，便于用任意图片工具查看
        let img = gnnuhub_ocr::decode_image(&captcha.image)?;
        img.save(out_dir.join(format!("{i:04}.png")))
            .map_err(|e| gnnuhub_core::Error::Image(format!("保存失败: {e}")))?;

        // uid 一并落盘，方便日后复现或补采
        std::fs::write(
            out_dir.join(format!("{i:04}.json")),
            serde_json::json!({ "uid": captcha.uid, "index": i }).to_string(),
        )?;

        saved += 1;
        if saved % 10 == 0 {
            println!("  已采集 {saved}/{count}");
        }

        tokio::time::sleep(INTERVAL).await;
    }

    println!("\n完成：新增 {saved} 张，目录 {}/", out_dir.display());
    println!(
        "下一步：cargo run -p gnnuhub-api --example captcha_fonts -- {} captcha_fonts.json",
        out_dir.display()
    );
    Ok(())
}

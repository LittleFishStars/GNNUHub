//! 教学执行计划（N153540）查询探测 + 移动端课表 POST 重验
//!
//! # 目标
//!
//! 1. 教学执行计划：页面表单 POST 到自身
//!    （`/jxzxjhgl/jxzxjhck_cxJxzxjhckIndex.html`），字段
//!    dl（类别）/jg_id（学院）/kch（课程号）等。本轮发空参数
//!    看默认返回形态（HTML 表格还是 JSON）。
//! 2. 移动端课表 POST 重验：此前判「服务端失效」的
//!    `xskbcxMobile_cxXsKb.html` 实际一直走 GET，用 POST 重验。
//!
//! # 请求预算
//!
//! 登录 ~5 + 计划 1 + 移动端 1 = 7。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_plan
//! ```

use gnnuhub_api::{Client, Session};
use gnnuhub_ocr::BitmapOcr;

const OUT_DIR: &str = "tools/js-recon/out";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("gnnuhub_api=warn")),
        )
        .with_target(false)
        .init();
    let _ = std::fs::create_dir_all(format!("{OUT_DIR}/pages"));

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    println!("[1/3] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    // ---- 1. 教学执行计划（空参数默认视图）----
    println!("[2/3] 教学执行计划 POST（空参数）...");
    let form = [
        ("dl", ""),
        ("jg_id", ""),
        ("kch", ""),
        ("kcmc", ""),
        ("jyxdxnm_cx", "2026"),
        ("jyxdxqm_cx", "3"),
    ];
    match session
        .post_form_for_probe(
            "/jxzxjhgl/jxzxjhck_cxJxzxjhckIndex.html",
            &[("gnmkdm", "N153540")],
            &form,
            &[],
        )
        .await
    {
        Ok(body) => {
            let _ = std::fs::write(format!("{OUT_DIR}/pages/plan_index.html"), &body)?;
            let is_json = body.trim().starts_with('{') || body.trim().starts_with('[');
            println!(
                "    {} 字节，{}",
                body.len(),
                if is_json { "JSON 响应" } else { "HTML 页面" }
            );
            if is_json {
                println!("    开头 {}", body.trim().chars().take(200).collect::<String>());
            } else {
                // 提取表格行数
                let rows = body.matches("<tr").count();
                println!("    HTML，<tr> 行数 {rows}");
            }
        }
        Err(e) => println!("    ❌ {e}"),
    }

    // ---- 2. 移动端课表 POST 重验 ----
    println!("\n[3/3] 移动端课表 POST 重验（此前判「失效」存疑）...");
    let form = [
        ("xnm", "2026"),
        ("xqm", "3"),
        ("zs", "3"),
        ("doType", "app"),
        ("kblx", "1"),
        ("xh", ""),
    ];
    match session
        .post_form_for_probe(
            "/kbcx/xskbcxMobile_cxXsKb.html",
            &[("gnmkdm", "N2154")],
            &form,
            &[],
        )
        .await
    {
        Ok(body) => {
            let _ = std::fs::write(format!("{OUT_DIR}/pages/kb_mobile_post.json"), &body)?;
            let t = body.trim();
            println!(
                "    {} 字节，开头 {}",
                t.len(),
                t.chars().take(100).collect::<String>()
            );
        }
        Err(e) => println!("    ❌ {e}"),
    }

    println!("\n抓取完成。");
    Ok(())
}

//! 通知正文 + 空闲教室数据抓取探针
//!
//! # 目标
//!
//! 1. 通知正文页样本：`/xtgl/xwck_ckXw.html?xwbh=<id>&doType=save`
//!    （解析器需要正文页的 DOM 结构样本）
//! 2. 空闲教室数据：`POST /cdjy/cdjy_cxKxcdlb.html?gnmkdm=N2155&doType=query`
//!    （附 `zd_fzdm=N211205-kxcdlb`，离线逆向 kxcdlb.js），表单参数取自
//!    `djks_index.html` 的 searchForm
//!
//! # 请求预算
//!
//! 登录 ~5 + 通知列表 1 + 正文 1 + 教室 1 = 8。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_notice_room
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

    // ---- 通知列表 + 第一条正文 ----
    println!("[2/3] 通知列表与正文...");
    let notices_html = session
        .fetch_raw("/xtgl/index_cxNews.html?localeKey=zh_CN")
        .await?;
    std::fs::write(format!("{OUT_DIR}/pages/notice_list.html"), &notices_html)?;

    let notices = gnnuhub_api::parse::parse_notices(&notices_html)?;
    println!("    通知 {} 条", notices.len());
    if let Some(first) = notices.first() {
        println!(
            "    抓取第一条正文: {}（{}）",
            first.title, first.detail_path
        );
        let content = session.fetch_raw(&first.detail_path).await?;
        std::fs::write(format!("{OUT_DIR}/pages/notice_content.html"), &content)?;
        println!(
            "    正文 {} 字节，已存档 notice_content.html",
            content.len()
        );

        // 解析实测（与桌面端同路径）
        match gnnuhub_api::parse::parse_notice_detail(&content) {
            Ok(d) => {
                println!("    解析成功:");
                println!("      标题: {}", d.title);
                println!(
                    "      发布人: {}，时间: {}，浏览: {}",
                    d.publisher, d.publish_time, d.views
                );
                println!(
                    "      正文前 120 字: {}",
                    d.content.chars().take(120).collect::<String>()
                );
            }
            Err(e) => println!("    ❌ 解析失败: {e}"),
        }
    }

    // ---- 空闲教室 ----
    println!("\n[3/3] 空闲教室数据...");
    let form = [
        ("xnm", "2026"),
        ("xqm", "3"),
        ("kcdlb", ""), // 空闲教室列表的批次代码（zd_fzdm 指向 N211205）
        ("xqh_id", "1"),
        ("jxl_id", ""),
        ("dyq", ""),
        ("zds", ""),
    ];
    match session
        .post_form_for_probe(
            "/cdjy/cdjy_cxKxcdlb.html",
            &[("gnmkdm", "N2155"), ("doType", "query")],
            &form,
            &[],
        )
        .await
    {
        Ok(body) => {
            std::fs::write(format!("{OUT_DIR}/pages/room_query.json"), &body)?;
            println!(
                "    {} 字节，开头 {}",
                body.len(),
                body.trim().chars().take(100).collect::<String>()
            );
        }
        Err(e) => println!("    ❌ {e}"),
    }

    println!("\n抓取完成。");
    Ok(())
}

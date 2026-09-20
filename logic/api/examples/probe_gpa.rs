//! 学业情况（GPA / 加权平均分）统计页抓取探针
//!
//! # 接口链路（离线逆向 xsxyqk_gpa_view.html）
//!
//! GPA 视图对话框页（空壳）→ `GET /xsxy/xsxyqk_cxKczxAllIndex.html?xh_id=`
//! （「各课程性质」学分/绩点统计，服务端渲染 HTML）→
//! 「指定课程性质」tab 为 `xsxyqk_cxKczxOnlyIndex.html`。
//!
//! # 请求预算
//!
//! 登录 ~5 + GPA 1 = 6。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_gpa
//! ```

use gnnuhub_api::{Client, Session};
use gnnuhub_core::model::AcademicTerm;
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
    let _ = std::fs::create_dir_all(OUT_DIR);

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    println!("[1/2] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功");

    // 会话 Cookie 落盘（供 Python 侧对照）
    let cookies: serde_json::Map<String, serde_json::Value> = session
        .cookies()
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    let _ = std::fs::write(
        ".scratch/rust_session_cookies.json",
        serde_json::to_string_pretty(&cookies)?,
    );

    // ---- GPA 统计页（各课程性质）----
    println!("\n[2/2] GPA 统计页（各课程性质）...");
    let body = session
        .fetch_raw("/xsxy/xsxyqk_cxKczxAllIndex.html?gnmkdm=N105515&xh_id=")
        .await?;
    std::fs::write(format!("{OUT_DIR}/pages/gpa_kczx_all.html"), &body)?;
    println!("    {} 字节，已存档 gpa_kczx_all.html", body.len());

    // 摘要：提取表格行文本
    let mut lines = Vec::new();
    for row in body.split("<tr") {
        if row.contains("<td") {
            let plain: String = row
                .split("<td")
                .skip(1)
                .map(|seg| {
                    let start = seg.find('>').map(|i| i + 1).unwrap_or(0);
                    let end = seg.find("</td>").unwrap_or(seg.len());
                    seg[start..end].trim().to_string()
                })
                .collect::<Vec<_>>()
                .join(" | ");
            lines.push(plain);
        }
    }
    for l in lines.iter().take(10) {
        println!("    {l}");
    }
    println!("    共 {} 个数据行", lines.len());

    // 校历实测：学期起止 + 推算当前周
    match session.academic_calendar().await {
        Ok(cal) => {
            let today = chrono::Local::now().date_naive();
            let week = cal.current_week(today);
            println!(
                "\n校历: {} 学年{}学期 {} ~ {}，今天为第 {:?} 周 ✅",
                cal.year, cal.term, cal.start_date, cal.end_date, week
            );
        }
        Err(e) => println!("\n校历: ❌ {e}"),
    }

    // 空闲教室实测（jyfs 修复后的 session 路径验证）
    match session
        .free_rooms(
            gnnuhub_core::model::AcademicTerm::first(2026),
            "2026-09-21",
            "2026-09-25",
            "08:00:00",
            "22:00:00",
        )
        .await
    {
        Ok(rooms) => {
            println!(
                "\n空闲教室: {} 条 ✅（首条 {}）",
                rooms.len(),
                rooms.first().map(|r| r.name.as_str()).unwrap_or("-")
            );
        }
        Err(e) => println!("\n空闲教室: ❌ {e}"),
    }

    // 顺带验证重修课程（回归）
    match session.retake_courses(AcademicTerm::first(2026)).await {
        Ok(c) => println!("回归：重修课程库 {} 条 ✅", c.len()),
        Err(e) => println!("回归：重修课程库 ❌ {e}"),
    }
    Ok(())
}

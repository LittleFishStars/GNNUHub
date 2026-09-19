//! 选课情况确认（N2158）与学业/学籍预警提醒的实测探针
//!
//! # 接口形状（离线逆向 xkqr_index.html 内联 JS）
//!
//! - 字段字典：`POST /kbcx/xskbqr_cxXskbqrIndex.html?doType=cxZdxxList`
//!   → `[{zdm, zdms, zdywms}, ...]`（指标代码→中文名）
//! - 指标值：`POST ...?doType=cxXkqkxx`，body
//!   `{doWhat: <zdm>, xnm, xqm}` → 该指标的当前值
//! - 首页学业预警提醒：`GET /xtgl/index_cxXsxyyjtx.html`
//! - 首页学籍预警确认状态：`GET /xtgl/index_cxXjyjqrzt.html`
//!
//! # 请求预算
//!
//! 登录 ~5 + 字典 1 + 指标 N(约 6~10) + 预警 2 ≈ 15。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_xkqr
//! ```

use gnnuhub_api::{Client, Session};
use gnnuhub_core::model::AcademicTerm;
use gnnuhub_ocr::BitmapOcr;

const BASE_PATH: &str = "/kbcx/xskbqr_cxXskbqrIndex.html";
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

    println!("[1/3] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    let term = AcademicTerm::first(2026);
    let xnm = term.as_xnm();
    let xqm = term.as_xqm().to_string();

    // ---- 字段字典 ----
    println!("[2/3] 指标字段字典（cxZdxxList）...");
    let dict_body = session
        .post_form_for_probe(
            BASE_PATH,
            &[("gnmkdm", "N2158"), ("doType", "cxZdxxList")],
            &[],
            &[],
        )
        .await?;
    println!("    {} 字节：{}", dict_body.len(), {
        dict_body.trim().chars().take(200).collect::<String>()
    });
    std::fs::write(format!("{OUT_DIR}/pages/xkqr_zdxx.json"), &dict_body)?;

    let dict: Vec<serde_json::Value> = match serde_json::from_str(&dict_body) {
        Ok(v) => v,
        Err(e) => {
            println!("    ⚠️ 字典不是数组（{e}），跳过指标逐查");
            println!("\n[3/3] 预警提醒...");
            warn_probe(&session).await?;
            return Ok(());
        }
    };

    // ---- 逐指标查询 ----
    println!("\n[3/3] 逐指标查询（cxXkqkxx）...");
    let mut results: Vec<(String, String, String)> = Vec::new();
    for entry in &dict {
        let Some(zdm) = entry.get("zdm").and_then(|v| v.as_str()) else {
            continue;
        };
        let zdms = entry
            .get("zdms")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let form = [
            ("doWhat", zdm),
            ("xnm", xnm.as_str()),
            ("xqm", xqm.as_str()),
        ];
        let body = session
            .post_form_for_probe(
                BASE_PATH,
                &[("gnmkdm", "N2158"), ("doType", "cxXkqkxx")],
                &form,
                &[],
            )
            .await?;
        let value = body.trim().trim_matches('"').to_string();
        println!("    {zdms}({zdm}) = {value}");
        results.push((zdm.to_string(), zdms, value));
    }
    std::fs::write(
        format!("{OUT_DIR}/pages/xkqr_values.json"),
        serde_json::to_string_pretty(&results)?,
    )?;

    // ---- 预警提醒 ----
    println!("\n[3b] 学业/学籍预警提醒...");
    warn_probe(&session).await?;

    Ok(())
}

async fn warn_probe(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    for (name, path) in [
        ("学业预警提醒", "/xtgl/index_cxXsxyyjtx.html"),
        ("学籍预警确认状态", "/xtgl/index_cxXjyjqrzt.html"),
    ] {
        match session.fetch_raw(path).await {
            Ok(body) => {
                let file = format!(
                    "{OUT_DIR}/pages/{}_{}",
                    name,
                    path.rsplit('/').next().unwrap_or("x")
                );
                std::fs::write(&file, &body)?;
                println!(
                    "    {name}: {} 字节，开头 {}",
                    body.len(),
                    body.trim().chars().take(80).collect::<String>()
                );
            }
            Err(e) => println!("    {name}: ❌ {e}"),
        }
    }
    Ok(())
}

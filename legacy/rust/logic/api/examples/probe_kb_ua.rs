//! 课表接口复验探针：登录后直接请求课表
//!
//! # 背景
//!
//! 历史上多个假设（WAF TLS 指纹、浏览器仿真等）均被证伪，最终根因
//! 是 `Session::send` 写死 GET 方法（课表接口只认 POST，见
//! `probe_kb_null.rs` 结尾的更正记录）。本探针保留为课表接口的
//! 快速复验工具。
//!
//! 请求预算：登录 ~5 + POST 1 = 6。

use gnnuhub_api::Client;
use gnnuhub_ocr::BitmapOcr;

/// 用户浏览器的完整 UA（HAR 实录）
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("gnnuhub_api=warn")),
        )
        .with_target(false)
        .init();

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    println!("[N] 登录（wreq 浏览器仿真）...");
    let client = Client::with_defaults()?;

    // 指纹自检：访问 TLS 指纹检测服务，确认 emulation 生效
    println!("    检测 TLS 指纹（tls.peet.ws）...");
    match client
        .http()
        .get("https://tls.browserleaks.com/json")
        .send()
        .await
    {
        Ok(resp) => {
            let text = resp.text().await?;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let ja3 = v
                    .pointer("/ja3_hash")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?");
                let ja4 = v.pointer("/ja4").and_then(|x| x.as_str()).unwrap_or("?");
                let ua = v
                    .pointer("/akamai_hash")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?");
                println!("    JA3={ja3} JA4={ja4} Akamai={ua}");
            } else {
                println!("    指纹检测响应异常（{} 字节）", text.len());
            }
        }
        Err(e) => println!("    指纹检测失败（不影响后续实验）: {e}"),
    }

    let session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功: {}\n", session.student_id());

    // 重修课程库实测（新功能的接口验证）
    match session
        .retake_courses(gnnuhub_core::model::AcademicTerm::first(2026))
        .await
    {
        Ok(courses) => println!("    重修课程库: {} 条 ✅", courses.len()),
        Err(e) => println!("    重修课程库: ❌ {e}"),
    }

    // 把本会话 Cookie 落盘，供 Python 侧对照实验使用
    let cookies: serde_json::Map<String, serde_json::Value> = session
        .cookies()
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    let _ = std::fs::write(
        ".scratch/rust_session_cookies.json",
        serde_json::to_string_pretty(&cookies)?,
    );
    println!("    Cookie 已写入 .scratch/rust_session_cookies.json\n");

    let form = [
        ("xnm", "2026"),
        ("xqm", "3"),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let body = session
        .post_form_for_probe(
            "/kbcx/xskbcx_cxXsgrkb.html",
            &[("gnmkdm", "N2151")],
            &form,
            &[],
        )
        .await?;
    let t = body.trim();
    if t.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
    {
        let n = v.get("kbList").and_then(|k| k.as_array()).map(|a| a.len());
        println!("[N] → kbList={n:?}（{} 字节）", t.len());
        if n.unwrap_or(0) > 0 {
            let _ = std::fs::write("tools/js-recon/out/pages/kb_verify.json", t);
            println!("    ✅ 课表接口正常");
        }
    } else {
        println!(
            "[N] → {}（{} 字节），UA 不是根因，指向 TLS 指纹层",
            t,
            t.len()
        );
    }
    Ok(())
}

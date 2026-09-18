//! 课表接口复验探针：登录后直接请求课表，验证浏览器仿真是否生效
//!
//! # 背景
//!
//! 根因已定性（见 probe_kb_null.rs）：学校 WAF 对课表接口做 TLS
//! 指纹校验。HTTP 栈换用 wreq（浏览器仿真，Chrome 档案）后，本探针
//! 用于确认课表数据可正常获取。
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
            println!("    ✅✅ 根因确认为 UA 指纹：换 Firefox UA 即可修复");
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

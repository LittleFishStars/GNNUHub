//! [N] 实验：UA 与会话指纹一致性
//!
//! # 背景
//!
//! M 实验发现：用户浏览器会话的 Cookie 由本探针（Chrome UA）使用时
//! 立即被 WAF 以非标准状态码 901 拦截——WAF 在做**会话与客户端指纹
//! 绑定校验**。由此推论：程序化登录（Chrome/120 伪装 UA）建立的
//! 会话，可能因 UA/TLS 指纹评分不足，在敏感接口（课表）被静默拦截。
//!
//! 本实验用**完整 Firefox UA** 重新登录并请求课表：
//! - 若拿到课表 → 根因是 UA 指纹（修复 = 换 UA）；
//! - 若仍 null → 根因更可能在 TLS 指纹层（需 impersonate 类客户端）。
//!
//! 请求预算：登录 ~5 + POST 1 = 6。

use gnnuhub_api::{Client, ClientConfig};
use gnnuhub_ocr::BitmapOcr;

/// 用户浏览器的完整 UA（HAR 实录）
const FIREFOX_UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:155.0) Gecko/20100101 Firefox/155.0";

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

    println!("[N] 用 Firefox UA 登录（与会话指纹一致）...");
    let config = ClientConfig {
        user_agent: FIREFOX_UA.to_string(),
        ..ClientConfig::default()
    };
    let client = Client::new(config)?;
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
            let _ = std::fs::write("tools/js-recon/out/pages/kb_FIREFOX_UA.json", t);
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

//! 请求层差异二分：同一会话 Cookie，不同客户端变体
//!
//! # 背景
//!
//! Python 用 Rust 会话的 Cookie 请求课表成功（kbList=18），Rust 自己
//! 发同一请求却得到 null——问题锁定在 Rust 请求层。本探针对以下
//! 变体逐个测试（全部复用同一 Cookie，不发登录请求）：
//!
//! - V0 对照：requests 同款形态（native-tls + python UA）
//! - V1：V0 + `http1_only`（强制 HTTP/1.1，排除 ALPN h2 差异）
//! - V2：V0 但接受 `gzip, deflate`（reqwest gzip feature 的默认值
//!   是 `gzip`，python-requests 是 `gzip, deflate`）
//!
//! Cookie 来源：`.scratch/rust_session_cookies.json`（probe_kb_ua 落盘）。

use std::time::Duration;

// 直接使用 reqwest（native-tls），与业务 client.rs 的后端一致。
use reqwest::Client;

const COOKIE_FILE: &str = ".scratch/rust_session_cookies.json";
const URL: &str = "https://jwgl.gnnu.edu.cn/kbcx/xskbcx_cxXsgrkb.html?gnmkdm=N2151";
const BODY: &str = "xnm=2026&xqm=3&kzlx=ck&xsdm=&kclbdm=&kclxdm=";
const COOKIE_HEADER: &str = "Cookie";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let jar: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(COOKIE_FILE)?)?;
    let cookie_value = jar
        .iter()
        .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("; ");
    println!("Cookie: {cookie_value}\n");

    let variants: Vec<(&str, Client)> = vec![
        (
            "V0 对照(native-tls+python UA)",
            Client::builder()
                .user_agent("python-requests/2.34.2")
                .timeout(Duration::from_secs(30))
                .build()?,
        ),
        (
            "V1 强制 HTTP/1.1",
            Client::builder()
                .user_agent("python-requests/2.34.2")
                .http1_only()
                .timeout(Duration::from_secs(30))
                .build()?,
        ),
        (
            "V2 Accept-Encoding: gzip, deflate",
            Client::builder()
                .user_agent("python-requests/2.34.2")
                .default_headers({
                    let mut h = reqwest::header::HeaderMap::new();
                    h.insert(
                        reqwest::header::ACCEPT_ENCODING,
                        reqwest::header::HeaderValue::from_static("gzip, deflate"),
                    );
                    h
                })
                .timeout(Duration::from_secs(30))
                .build()?,
        ),
    ];

    for (tag, client) in variants {
        let resp = client
            .post(URL)
            .header(COOKIE_HEADER, &cookie_value)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(BODY)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        let t = text.trim();
        let detail = if t == "null" {
            "null ❌".to_string()
        } else if t.starts_with('{') {
            format!(
                "kbList={:?} ✅",
                serde_json::from_str::<serde_json::Value>(t)
                    .ok()
                    .and_then(|v| v.get("kbList").and_then(|k| k.as_array()).map(|a| a.len()))
            )
        } else {
            format!("其他: {}", t.chars().take(40).collect::<String>())
        };
        println!("[{tag}] HTTP {status} → {detail}");
    }
    Ok(())
}

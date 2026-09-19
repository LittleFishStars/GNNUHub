//! gnio.gnnu.edu.cn（JeewX 信息服务门户）探测探针
//!
//! # 背景
//!
//! 学校还有一套与教务系统分离的信息服务门户 `gnio.gnnu.edu.cn`
//! （JeewX 平台），入口为 `/jeewx/openv1/caslogin.do`——与本系统
//! 同源的 CAS 单点登录。门户上可能有教务系统没有的数据源
//! （通知公告、办事流程等）。
//!
//! # 流程
//!
//! 1. GET caslogin.do（无票据）→ 从 302 Location 提取 CAS 的
//!    `service` 参数（该值必须与后续签发 ticket 的 service 一致）
//! 2. 复用 [`gnnuhub_api`] 的验证码 + CAS 登录，以该 service 签发 ticket
//! 3. GET caslogin.do?ticket=... → 手动跟随重定向链（每跳的
//!    Set-Cookie 由 cookie_store 自动保管），直至落地门户页
//! 4. 抓取门户首页落盘供离线分析
//!
//! # 请求预算
//!
//! 1 + 4（CAS 登录）+ 3（票据交换跳转）+ 1（落地页）≈ 9 次。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_gnio
//! ```

use gnnuhub_api::login::{LoginOutcome, login_with_retry};
use gnnuhub_api::{Client, ClientConfig};
use gnnuhub_ocr::BitmapOcr;

const GNIO_BASE: &str = "https://gnio.gnnu.edu.cn";
const CAS_LOGIN: &str = "/jeewx/openv1/caslogin.do";
const OUT_DIR: &str = "tools/js-recon/out/gnio";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing::Level::INFO.to_string().into()),
        )
        .with_target(false)
        .init();
    let _ = std::fs::create_dir_all(OUT_DIR);

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    // gnio 专用客户端：cookie_store 自动管理 Cookie（本探针不手动
    // 设 COOKIE 头，无叠加冲突）；重定向手动跟以便记录每一跳
    let http = reqwest::Client::builder()
        .user_agent(concat!("gnnuhub/", env!("CARGO_PKG_VERSION")))
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // ---- 1. 发现 CAS service 参数 ----
    println!("[1] 探测 caslogin.do 的 service 参数...");
    let resp = http.get(format!("{GNIO_BASE}{CAS_LOGIN}")).send().await?;
    let status = resp.status();
    let location = resp
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    println!("    HTTP {status}，Location: {location:?}");

    let Some(location) = location else {
        // 不是 302？可能是 200 的登录页——存档分析
        let body = resp.text().await?;
        let _ = std::fs::write(format!("{OUT_DIR}/caslogin_direct.html"), &body);
        println!(
            "    非 302，已存档 caslogin_direct.html（{} 字节）",
            body.len()
        );
        return Ok(());
    };

    let service = url::Url::parse(&location)?
        .query_pairs()
        .find(|(k, _)| k == "service")
        .map(|(_, v)| v.to_string())
        .ok_or("302 Location 中未找到 service 参数")?;
    println!("    service = {service}\n");

    // ---- 2. CAS 登录签发 ticket ----
    println!("[2] CAS 登录（复用教务系统的验证码与票据流程）...");
    let client = Client::new(ClientConfig::default().with_max_login_retries(3))?;
    let ocr = BitmapOcr::embedded()?;
    let outcome =
        login_with_retry(&client, &student_id.to_string(), &password, &service, &ocr).await?;
    let ticket = match outcome {
        LoginOutcome::Success { ticket, .. } => ticket,
        other => return Err(format!("CAS 登录未成功: {other:?}").into()),
    };
    println!("    ticket = {}\n", &ticket[..12.min(ticket.len())]);

    // ---- 3. 票据交换：手动跟随重定向链（最多 5 跳）----
    println!("[3] caslogin.do?ticket 票据交换...");
    let mut url = format!("{GNIO_BASE}{CAS_LOGIN}?ticket={ticket}");
    let mut final_body = String::new();
    for hop in 1..=5 {
        let resp = http.get(&url).send().await?;
        let status = resp.status();
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).to_string())
            .collect();
        let location = resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        println!(
            "    跳{hop}: {url}\n      → HTTP {status}，Set-Cookie: {set_cookies:?}，Location: {location:?}"
        );

        match location {
            Some(next) if status.is_redirection() => {
                // 相对跳转补全
                url = if next.starts_with("http") {
                    next
                } else {
                    format!("{GNIO_BASE}{next}")
                };
            }
            _ => {
                final_body = resp.text().await?;
                let _ = std::fs::write(format!("{OUT_DIR}/portal_landing.html"), &final_body);
                println!(
                    "      落地页 {} 字节，已存档 portal_landing.html",
                    final_body.len()
                );
                break;
            }
        }
    }

    // ---- 4. 抓取门户首页 ----
    println!("\n[4] 抓取门户首页...");
    let resp = http.get(format!("{GNIO_BASE}/")).send().await?;
    let body = resp.text().await?;
    let _ = std::fs::write(format!("{OUT_DIR}/portal_home.html"), &body);
    println!("    {} 字节，已存档 portal_home.html", body.len());

    println!(
        "\n下一步：离线分析 portal_landing.html / portal_home.html 的菜单与 iframe，挖掘门户 API。"
    );
    Ok(())
}

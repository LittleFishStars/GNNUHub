//! 一次性抓取教务系统的前端静态资源，供离线逆向
//!
//! 设计原则：**只跑一次，产物落盘，之后全部离线分析。**
//!
//! 登录逻辑（加密、跳转、Cookie 作用域、请求头要求）都写在前端 JS 里，
//! 读代码比反复发请求试探高效得多，也不会触发风控。
//!
//! # 验证码的两种处理方式
//!
//! - `interval`（默认）：分两次运行，先跑 `captcha` 拿图，人工识别后
//!   再用 `submit` 提交。适合手工调试。
//! - `once`：通过**文件桥**在单个进程内完成——把验证码写入
//!   `tools/js-recon/captcha.png`，轮询等待 `captcha.txt` 出现。
//!   会话不会中断，抓取更省事。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run -p gnnuhub-api --example js_recon
//!
//! # 交互式（单进程）模式
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run -p gnnuhub-api --example js_recon -- once
//! ```
//!
//! # 产物
//!
//! 统一落在 `tools/js-recon/out/`：
//!
//! - `pages/`     各页面原始 HTML
//! - `scripts/`   从页面中提取并下载的 JS
//! - `index.json` 抓取清单
//!
//! 抓取完成后运行 `./tools/js-recon/analyze.sh` 做离线分析（不联网）。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gnnuhub_api::Client;
use gnnuhub_ocr::{InteractiveFn, ManualOcr};

/// 产物根目录
const OUT_DIR: &str = "tools/js-recon/out";

/// 验证码落盘位置（供人工查看）
const CAPTCHA_PNG: &str = "tools/js-recon/captcha.png";

/// 人工输入回填位置（文件桥模式）
const CAPTCHA_TXT: &str = "tools/js-recon/captcha.txt";

/// 等待人工输入的最长时间
const INPUT_TIMEOUT: Duration = Duration::from_secs(600);

/// 需要抓取的页面清单
///
/// 覆盖登录、首页与主要功能模块；接口路径通常散落在这些页面的 JS 里。
const PAGES: &[(&str, &str)] = &[
    // 登录相关（最关键）
    ("login_slogin", "/xtgl/login_slogin.html"),
    ("index_initMenu", "/xtgl/index_initMenu.html"),
    ("index_cxYhxx", "/xtgl/index_cxYhxxIndex.html"),
    // 学籍与课表
    ("xsgrxx", "/xsxxxggl/xsgrxxwh_cxXsgrxx.html"),
    (
        "kbcx",
        "/kbcx/xskbcx_cxXskbcxIndex.html?gnmkdm=N2151&layout=default",
    ),
    // 首页（不带参数，取默认框架）
    ("home", "/xtgl/index.html"),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("gnnuhub_api=info")),
        )
        .with_target(false)
        .init();

    // 只有传 `once` 才启用文件桥交互；其余情况走默认的手工输入回调
    let interactive = std::env::args().nth(1).as_deref() == Some("once");
    let callback: Option<&InteractiveFn> = interactive.then_some(&FILE_BRIDGE);

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    let out = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(out.join("pages"))?;
    std::fs::create_dir_all(out.join("scripts"))?;

    // ---- 登录 ----
    println!("[1/3] 登录中...");
    let client = Client::with_defaults()?;
    let session = client
        .login(student_id, &password, &ManualOcr::new(), callback)
        .await?;
    println!("      登录成功，学号 {}", session.student_id());

    // ---- 抓取页面 ----
    println!("[2/3] 抓取页面（{} 个）...", PAGES.len());
    let mut entries: Vec<Entry> = Vec::new();
    let mut all_scripts: BTreeSet<String> = BTreeSet::new();

    for (name, path) in PAGES {
        match session.fetch_raw(path).await {
            Ok(html) => {
                let file = format!("pages/{name}.html");
                std::fs::write(out.join(&file), &html)?;
                let found = extract_scripts(&html);
                println!(
                    "      {name}: {} 字节, {} 个脚本引用",
                    html.len(),
                    found.len()
                );
                all_scripts.extend(found.iter().cloned());
                entries.push(Entry {
                    name: name.to_string(),
                    path: path.to_string(),
                    bytes: html.len(),
                    file,
                    scripts: found,
                });
            }
            Err(e) => {
                println!("      {name}: 失败 - {e}");
                entries.push(Entry {
                    name: name.to_string(),
                    path: path.to_string(),
                    bytes: 0,
                    file: String::new(),
                    scripts: Vec::new(),
                });
            }
        }
    }

    // ---- 抓取 JS ----
    println!("[3/3] 下载 JS（去重后 {} 个）...", all_scripts.len());
    let mut ok = 0usize;
    for src in &all_scripts {
        // 只处理站内脚本
        let Some(path) = localize(src) else {
            continue;
        };
        match session.fetch_raw(&path).await {
            Ok(js) => {
                let file = format!("scripts/{}", sanitize(src));
                std::fs::write(out.join(&file), &js)?;
                ok += 1;
            }
            Err(e) => println!("      {src}: 失败 - {e}"),
        }
    }
    println!("      成功下载 {ok} 个");

    // ---- 写清单 ----
    let index = serde_json::json!({
        "pages": entries.iter().map(|e| serde_json::json!({
            "name": e.name,
            "path": e.path,
            "bytes": e.bytes,
            "file": e.file,
            "scripts": e.scripts,
        })).collect::<Vec<_>>(),
        "scripts": all_scripts.iter().collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("index.json"),
        serde_json::to_string_pretty(&index)?,
    )?;

    println!("\n产物: {OUT_DIR}/");
    println!("下一步: ./tools/js-recon/analyze.sh   （离线分析，不联网）");
    Ok(())
}

/// 文件桥交互回调
///
/// 把验证码写入 `CAPTCHA_PNG` 并清理旧的输入文件，然后轮询等待
/// `CAPTCHA_TXT` 出现。这样整个抓取流程在**一个进程内**跑完，会话不断。
static FILE_BRIDGE: fn(&str) -> Result<Option<String>, gnnuhub_core::Error> = |image: &str| {
    let img = gnnuhub_ocr::decode_image(image)?;
    img.save(CAPTCHA_PNG)
        .map_err(|e| gnnuhub_core::Error::Image(format!("保存验证码失败: {e}")))?;
    let _ = std::fs::remove_file(CAPTCHA_TXT);

    println!("      验证码已写入 {CAPTCHA_PNG}");
    println!("      请查看图片，把 4 位字符写入 {CAPTCHA_TXT}");

    let start = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(CAPTCHA_TXT) {
            let text = text.trim().to_string();
            if !text.is_empty() {
                println!("      读到输入: {text}");
                let _ = std::fs::remove_file(CAPTCHA_TXT);
                return Ok(Some(text));
            }
        }
        if start.elapsed() > INPUT_TIMEOUT {
            return Err(gnnuhub_core::Error::CaptchaRequiresManualInput);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
};

/// 抓取清单中的一条页面记录
struct Entry {
    name: String,
    path: String,
    bytes: usize,
    file: String,
    scripts: Vec<String>,
}

/// 从 HTML 中提取 `<script src="...">`
///
/// 只用简单字符串扫描，不引入完整 HTML 解析器——页面结构稳定，
/// 且这里的目标是「尽量多抓到」而非精确解析。
fn extract_scripts(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;

    while let Some(pos) = rest.find("<script") {
        rest = &rest[pos..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];

        if let Some(src) = attr_value(tag, "src")
            && !src.is_empty()
            && !src.starts_with("http://")
            && !src.starts_with("https://")
            && !src.starts_with("//")
        {
            out.push(src.to_string());
        }
        rest = &rest[end..];
    }
    out.sort();
    out.dedup();
    out
}

/// 取标签中某个属性的值，兼容单双引号
///
/// `tag` 是完整的开标签（形如 `<script src="/a.js">`），因此无引号
/// 属性的取值需要显式剥掉结尾的 `>`。
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let body = &rest[1..];
        let end = body.find(quote)?;
        Some(body[..end].to_string())
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// 把脚本引用转成可请求的站内路径
///
/// 处理相对路径与 `../`，跳过外部域名。
fn localize(src: &str) -> Option<String> {
    if src.starts_with("http") || src.starts_with("//") {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in src.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("/{}", parts.join("/")))
}

/// 把 URL 转成安全的文件名
fn sanitize(src: &str) -> String {
    let trimmed = src.trim_start_matches('/');
    let mut name: String = trimmed
        .chars()
        .map(|c| {
            if matches!(c, '/' | '?' | '&' | '=') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if name.len() > 120 {
        // 过长时保留尾部（文件名通常在末尾）
        name = name[name.len() - 120..].to_string();
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脚本引用应被提取、去重并排序
    #[test]
    fn extracts_script_srcs() {
        let html = r#"<script src="/a.js"></script>
                      <script src='/b.js'></script>
                      <script src="/a.js"></script>
                      <script src="https://cdn.example.com/c.js"></script>
                      <script>inline()</script>"#;
        assert_eq!(extract_scripts(html), vec!["/a.js", "/b.js"]);
    }

    /// 属性取值应兼容单引号、双引号与无引号
    #[test]
    fn reads_attribute_values() {
        assert_eq!(
            attr_value(r#"<a href="/x">"#, "href").as_deref(),
            Some("/x")
        );
        assert_eq!(attr_value("<a href='/x'>", "href").as_deref(), Some("/x"));
        assert_eq!(attr_value("<a href=/x>", "href").as_deref(), Some("/x"));
        assert_eq!(attr_value("<a>", "href"), None);
    }

    /// 相对路径应被规整，外部域名应被跳过
    #[test]
    fn localizes_only_site_paths() {
        assert_eq!(localize("../../js/main.js").as_deref(), Some("/js/main.js"));
        assert_eq!(localize("/js/main.js").as_deref(), Some("/js/main.js"));
        assert_eq!(localize("https://cdn.example.com/a.js"), None);
        assert_eq!(localize("//cdn.example.com/a.js"), None);
    }

    /// URL 中的分隔符应被替换成下划线
    #[test]
    fn sanitizes_urls_into_filenames() {
        assert_eq!(sanitize("/js/a.js?ver=1"), "js_a.js_ver_1");
        assert_eq!(sanitize("/js/a.js"), "js_a.js");
    }
}

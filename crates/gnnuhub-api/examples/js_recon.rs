//! 一次性抓取教务系统的前端静态资源，供离线逆向
//!
//! 设计原则：**只跑一次，产物落盘，之后全部离线分析。**
//!
//! 登录逻辑（加密、跳转、Cookie 作用域、请求头要求）都写在前端 JS 里，
//! 读代码比反复发请求试探高效得多，也不会触发风控。
//!
//! 运行：
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run -p gnnuhub-api --example js_recon
//! ```
//!
//! 产物：`tools/js-recon/out/`
//!
//! - `pages/`   各页面原始 HTML
//! - `scripts/` 从页面中提取并下载的 JS
//! - `index.json` 抓取清单

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gnnuhub_api::Client;

/// 产物根目录
const OUT_DIR: &str = "tools/js-recon/out";

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
        .with_env_filter("gnnuhub_api=info")
        .with_target(false)
        .init();

    let student_id = std::env::var("GNNU_STUDENT_ID")?;
    let password = std::env::var("GNNU_PASSWORD")?;

    let out = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(out.join("pages"))?;
    std::fs::create_dir_all(out.join("scripts"))?;

    // ---- 登录 ----
    println!("[1/3] 登录中...");
    let client = Client::with_defaults()?;
    let session = client
        .login(
            student_id.parse()?,
            &password,
            &gnnuhub_ocr::ManualOcr::new(),
            None,
        )
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
                println!("      {name}: {} 字节, {} 个脚本引用", html.len(), found.len());
                for s in &found {
                    all_scripts.insert(s.clone());
                }
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
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
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
        .map(|c| if c == '/' || c == '?' || c == '&' || c == '=' { '_' } else { c })
        .collect();
    if name.len() > 120 {
        // 过长时保留尾部（文件名通常在末尾）
        name = name[name.len() - 120..].to_string();
    }
    name
}

/// 保留：供后续扩展判断路径是否为本地资源
#[allow(dead_code)]
fn is_local(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "js")
}

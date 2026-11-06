//! 「信息查询」组批量页面探测：一次登录抓齐剩余查询功能的页面与专属 JS
//!
//! # 目标（2026-09-19 完整菜单解析，信息查询组 13 项中未实现的 8 项）
//!
//! | 模块码 | 功能 | 页面 |
//! |---|---|---|
//! | N105505 | 学籍预警查询 | /xjyj/xjyj_cxXjyjIndex.html |
//! | N2158 | 学生选课情况确认 | /kbcx/xskbqr_cxXskbqrIndex.html?doType=details |
//! | N255720 | 重修课程查询 | /cxkccx/cxkccx_cxCxkccxIndex.html（已存档） |
//! | N305516 | 学业预警处理查询 | /xyyjjk/xyyjclcx_cxXyyjclcxxsIndex.html |
//! | N105515 | 学生学业情况查询 | /xsxy/xsxyqk_cxXsxyqkIndex.html |
//! | N558020 | 学生成绩总表打印 | /bysxxcx/xscjzbdy_cxXscjzbdyIndex.html |
//! | N153540 | 教学执行计划查看 | /jxzxjhgl/jxzxjhck_cxJxzxjhckIndex.html |
//! | N2155 | 查询空闲教室 | /cdjy/cdjy_cxKxcdlb.html（已存档） |
//!
//! # 设计原则
//!
//! 只抓页面与 JS 落盘（不调数据接口），离线分析出各接口的
//! `paramMap` / `url` 后再行实测。专属 JS 已存档的跳过。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_query_pages
//! ```

use std::collections::BTreeSet;
use std::path::PathBuf;

use gnnuhub_api::{Client, Session};
use gnnuhub_ocr::BitmapOcr;

const OUT_DIR: &str = "tools/js-recon/out";

/// 待抓取模块：(短名, 模块码, 菜单路径)
const MODULES: &[(&str, &str, &str)] = &[
    ("xjyj", "N105505", "/xjyj/xjyj_cxXjyjIndex.html"),
    (
        "xkqr",
        "N2158",
        "/kbcx/xskbqr_cxXskbqrIndex.html?doType=details",
    ),
    ("xyyj", "N305516", "/xyyjjk/xyyjclcx_cxXyyjclcxxsIndex.html"),
    ("xsxyqk", "N105515", "/xsxy/xsxyqk_cxXsxyqkIndex.html"),
    (
        "cjzbdy",
        "N558020",
        "/bysxxcx/xscjzbdy_cxXscjzbdyIndex.html",
    ),
    (
        "jxzxjh",
        "N153540",
        "/jxzxjhgl/jxzxjhck_cxJxzxjhckIndex.html",
    ),
];

fn page_path(gnmkdm: &str, menu_path: &str) -> String {
    if menu_path.contains('?') {
        format!("{menu_path}&gnmkdm={gnmkdm}")
    } else {
        format!("{menu_path}?gnmkdm={gnmkdm}")
    }
}

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

    let out = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(out.join("pages"))?;
    std::fs::create_dir_all(out.join("scripts"))?;

    println!("[1/3] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    // ---- 抓页面 ----
    println!("[2/3] 抓取查询页面（{} 个）...", MODULES.len());
    let mut collected: Vec<(&str, BTreeSet<String>)> = Vec::new();

    for (name, gnmkdm, menu_path) in MODULES {
        let path = page_path(gnmkdm, menu_path);
        match session.fetch_raw(&path).await {
            Ok(html) => {
                let file = format!("pages/{name}_index.html");
                std::fs::write(out.join(&file), &html)?;
                let scripts = extract_scripts(&html);
                println!(
                    "    {name}({gnmkdm}): {} 字节, {} 个脚本",
                    html.len(),
                    scripts.len()
                );
                collected.push((gnmkdm, scripts));
            }
            Err(e) => println!("    {name}({gnmkdm}): ❌ {e}"),
        }
    }

    // ---- 下载专属 JS（按功能目录过滤，跳过已存档）----
    println!("\n[3/3] 下载模块专属 JS...");
    for (gnmkdm, scripts) in &collected {
        for src in scripts {
            // 专属 JS 判定：路径含功能目录（排除公共框架/UI 库）
            let is_module_js = src.contains("jwglxt/comp/");
            if !is_module_js {
                continue;
            }
            let Some(path) = localize(src) else { continue };
            let dest = out.join("scripts").join(sanitize(src));
            if dest.exists() {
                continue;
            }
            match session.fetch_raw(&path).await {
                Ok(js) => {
                    std::fs::write(&dest, &js)?;
                    println!("    + {}（{} 字节）", sanitize(src), js.len());
                }
                Err(e) => println!("    {src}: ❌ {e}"),
            }
        }
        let _ = gnmkdm;
    }

    println!("\n抓取完成。下一步：离线分析各 JS 的 url/paramMap/colModel。");
    Ok(())
}

/// 从 HTML 提取 `<script src>`
fn extract_scripts(html: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = html;
    while let Some(pos) = rest.find("<script") {
        rest = &rest[pos..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        if let Some(src) = attr_value(tag, "src")
            && !src.is_empty()
            && !src.starts_with("http")
            && !src.starts_with("//")
        {
            out.insert(src.to_string());
        }
        rest = &rest[end..];
    }
    out
}

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
        None
    }
}

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
        None
    } else {
        Some(format!("/{}", parts.join("/")))
    }
}

fn sanitize(src: &str) -> String {
    let name: String = src
        .trim_start_matches('/')
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
        name[name.len() - 120..].to_string()
    } else {
        name
    }
}

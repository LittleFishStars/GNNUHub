//! 新功能模块探针：抓取候选功能的页面与专属 JS，供离线逆向
//!
//! # 目标（来自 `index_initMenu.html` 菜单的离线挖掘）
//!
//! | 模块码 | 功能 | 菜单路径 |
//! |---|---|---|
//! | N358105 | 考试信息查询 | `/kwgl/kscx_cxXsksxxIndex.html` |
//! | N255720 | 全校课程查询 | `/cxkccx/cxkccx_cxCxkccxIndex.html` |
//! | N2155 | 等级考试（成绩定级） | `/cdjy/cdjy_cxKxcdlb.html` |
//! | N401605 | 学生评教 | `/xspjgl/xspj_cxXspjIndex.html` |
//!
//! # 设计原则（沿用项目纪律）
//!
//! 1. **抓取与分析分离**：本探针第一段只抓页面与 JS 落盘，
//!    **不调任何数据接口**；离线读 JS 确定 `paramMap` 与接口路径后，
//!    才用 `--probe` 第二段做数据试探。
//! 2. **请求预算**：登录 ~5 + 页面 4 + 专属 JS ~4 ≈ 13 次。
//! 3. 专属 JS 若已存档则跳过，不重复下载。
//!
//! # 静态分析摘要
//!
//! 抓取结束后对每个 JS 提取两类线索并打印：
//! - 接口路径：`*_cx*.html`（正方查询接口命名惯例）
//! - 参数上下文：`paramMap` / `form` / `queryModel` 出现处的前后文
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_features           # 抓取
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_features -- --probe xnm xqm # 试探
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gnnuhub_api::{Client, Session};
use gnnuhub_ocr::BitmapOcr;

/// 产物根目录
const OUT_DIR: &str = "tools/js-recon/out";

/// 候选模块清单：(短名, 模块码, 页面路径)
const MODULES: &[(&str, &str, &str)] = &[
    ("ksap", "N358105", "/kwgl/kscx_cxXsksxxIndex.html"),
    ("qkccx", "N255720", "/cxkccx/cxkccx_cxCxkccxIndex.html"),
    ("djks", "N2155", "/cdjy/cdjy_cxKxcdlb.html"),
    (
        "xspj",
        "N401605",
        "/xspjgl/xspj_cxXspjIndex.html?doType=details",
    ),
];

/// 模块页面已有 fetch_raw 可用，但菜单路径必须带 gnmkdm，
/// 这里统一补上（不带会被权限过滤器拒绝）
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
    let do_probe = std::env::args().any(|a| a == "--probe");
    let args: Vec<String> = std::env::args().collect();
    let (xnm, xqm) = match args.len() >= 4 && do_probe {
        true => (args[2].clone(), args[3].clone()),
        false => ("2026".to_string(), "3".to_string()),
    };

    let out = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(out.join("pages"))?;
    std::fs::create_dir_all(out.join("scripts"))?;

    println!("[1/3] 登录中...");
    let client = Client::with_defaults()?;
    let ocr = BitmapOcr::embedded()?;
    let session = client.login(student_id, &password, &ocr).await?;
    println!("      登录成功，学号 {}\n", session.student_id());

    // ---- 抓取模块页面 ----
    println!("[2/3] 抓取模块页面（{} 个）...", MODULES.len());
    let mut module_scripts: Vec<(&str, BTreeSet<String>)> = Vec::new();

    for (name, gnmkdm, menu_path) in MODULES {
        let path = page_path(gnmkdm, menu_path);
        match session.fetch_raw(&path).await {
            Ok(html) => {
                let file = format!("pages/{name}_index.html");
                std::fs::write(out.join(&file), &html)?;
                let scripts = extract_scripts(&html);
                println!(
                    "      {name}({gnmkdm}): {} 字节, {} 个脚本",
                    html.len(),
                    scripts.len()
                );
                module_scripts.push((gnmkdm, scripts));
            }
            Err(e) => println!("      {name}({gnmkdm}): ❌ {e}"),
        }
    }

    // ---- 下载专属 JS（跳过已存档） ----
    println!("\n[3/3] 下载模块专属 JS...");
    let mut total = 0usize;
    for (gnmkdm, scripts) in &module_scripts {
        for src in scripts {
            let Some(path) = localize(src) else { continue };
            // 模块专属 JS 一般在其功能目录下；公共框架 JS 已存档过则跳过
            let dest = out.join("scripts").join(sanitize(src));
            if dest.exists() {
                continue;
            }
            match session.fetch_raw(&path).await {
                Ok(js) => {
                    std::fs::write(&dest, &js)?;
                    total += 1;
                    println!("      + {}", sanitize(src));
                }
                Err(e) => println!("      {src}: ❌ {e}"),
            }
        }
        let _ = gnmkdm;
    }
    println!("      新下载 {total} 个");

    // ---- 静态分析摘要（纯本地） ----
    println!("\n=== JS 静态分析摘要 ===");
    for (gnmkdm, scripts) in &module_scripts {
        println!("\n## 模块 {gnmkdm}");
        for src in scripts {
            let dest = out.join("scripts").join(sanitize(src));
            let Ok(js) = std::fs::read_to_string(&dest) else {
                continue;
            };
            report_endpoints(&js);
        }
    }

    if do_probe {
        println!("\n=== 第二段：数据接口试探（每个模块 1 发）===");
        probe_modules(&session, &xnm, &xqm).await;
    } else {
        println!("\n（抓取阶段完成；离线确认参数后加 `--probe <xnm> <xqm>` 试探数据接口）");
    }

    Ok(())
}

/// 一次数据接口试探的定义：(短名, 路径, 查询串, 表单)
type DataProbe<'a> = (
    &'a str,
    &'a str,
    &'a [(&'a str, &'a str)],
    &'a [(&'a str, &'a str)],
);

/// 第二段：按离线分析得到的接口形状试探数据接口
///
/// 仅调用「读查询」接口（`doType=query`），每个模块 1 发。
/// 形状全部来自第一轮抓取的 JS 离线分析（paramMap + remoteParams +
/// jqGrid 参数重映射），见各条目注释。
async fn probe_modules(session: &Session, xnm: &str, xqm: &str) {
    // (短名, 数据接口路径, 查询串, 表单)
    const DATA_ENDPOINTS: &[DataProbe] = &[
        // 考试安排：cxXsksxxIndex.js 的 paramMap()（学生分支）+ remoteParams
        // zd_fzdm="N358105-xs"；URL 即菜单页面 + doType=query
        (
            "ksap",
            "/kwgl/kscx_cxXsksxxIndex.html",
            &[("gnmkdm", "N358105"), ("doType", "query")],
            &[
                ("xnm", ""),
                ("xqm", ""),
                ("ksmcdmb_id", ""),
                ("kch", ""),
                ("kc", ""),
                ("ksrq", ""),
                ("kkbm_id", ""),
                ("zd_fzdm", "N358105-xs"),
                ("queryModel.showCount", "100"),
                ("queryModel.currentPage", "1"),
            ],
        ),
        // 全校课程：cxCxkccxIndex.js 的 paramMap()；cxxnm/cxxqm 取页面
        // 下拉框的默认选中值
        (
            "qkccx",
            "/cxkccx/cxkccx_cxCxkccxIndex.html",
            &[("gnmkdm", "N255720"), ("doType", "query")],
            &[
                ("cxxnm", ""),
                ("cxxqm", ""),
                ("kkbm_id", ""),
                ("kclbdm", ""),
                ("bxqsfkk_cx", ""),
                ("kkxb_id", ""),
                ("kch", ""),
            ],
        ),
    ];

    for (name, path, query, form_def) in DATA_ENDPOINTS {
        let form: Vec<(&str, &str)> = form_def
            .iter()
            .map(|&(k, v)| match k {
                "xnm" | "cxxnm" => (k, xnm),
                "xqm" | "cxxqm" => (k, xqm),
                _ => (k, v),
            })
            .collect();
        match session.post_form_for_probe(path, query, &form, &[]).await {
            Ok(body) => {
                let preview: String = body.trim().chars().take(300).collect();
                println!("  [{name}] {path} → {} 字节\n      {preview}\n", body.len());
                let file = format!("pages/{name}_probe.json");
                let _ = std::fs::write(Path::new(OUT_DIR).join(file), &body);
            }
            Err(e) => println!("  [{name}] {path} → ❌ {e}"),
        }
    }
}

/// 打印 JS 中的接口路径与参数上下文
fn report_endpoints(js: &str) {
    // 正方查询接口命名惯例：xxx_cxXxx.html
    let mut endpoints: BTreeSet<String> = BTreeSet::new();
    let bytes = js.as_bytes();
    for (i, _) in js.match_indices(".html") {
        // 向前扫描到非法路径字符为止
        let start = bytes[..i]
            .iter()
            .rposition(|&b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'/'))
            .map_or(0, |p| p + 1);
        let candidate = &js[start..i + 5];
        if candidate.contains("_cx") || candidate.contains("cx") {
            endpoints.insert(candidate.to_string());
        }
    }
    if !endpoints.is_empty() {
        for ep in &endpoints {
            println!("  接口: {ep}");
        }
    }

    // paramMap / form 上下文（每个关键字最多看 2 处；按字符边界安全截取）
    for keyword in ["paramMap", "queryModel"] {
        for (idx, _hit) in js.match_indices(keyword).take(2) {
            let from = char_floor(js, idx.saturating_sub(60));
            let to = char_floor(js, (idx + 320).min(js.len()));
            println!(
                "  --- {keyword} #{} ---\n  {}",
                idx + 1,
                js[from..to].replace('\n', " ")
            );
        }
    }
}

/// 返回 ≤ target 的最近字符边界（避免多字节中文被切在中间）
fn char_floor(s: &str, target: usize) -> usize {
    let mut i = target.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 从 HTML 中提取 `<script src="...">`（同 js_recon）
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

/// 取标签属性值（同 js_recon）
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

/// 相对路径规整（同 js_recon）
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

/// URL → 安全文件名（同 js_recon）
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

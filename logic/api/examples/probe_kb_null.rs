//! 定位「课表接口返回字面量 null」的触发条件
//!
//! # 背景
//!
//! 同一套参数（xnm=2026, xqm=3, kzlx=ck）在 2026-09-18 早些的
//! `verify_endpoints` 实测中取到 9 门课，但在桌面端会话里返回
//! 字面量 `null`（parse 报 `invalid type: null`）。本探针用最小
//! 对照实验定位差异：
//!
//! | 实验 | 假设 |
//! |---|---|
//! | A | 直接 POST（与 verify_endpoints 完全同形状）——若成功则是**瞬态** |
//! | B | A 为 null 时立即重试一次——判断是否「会话首次业务请求被吞」 |
//! | C | 先 GET 课表 Index 页面再 POST——判断是否依赖页面建立的服务端上下文 |
//! | D | xnm/xqm 传空串——判断「空值 = 当前学期默认」语义 |
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_kb_null
//! ```
//!
//! 请求预算：登录 ~5 + A1 + B1 + C2 + D1 ≤ 10。

use gnnuhub_api::{Client, Session};
use gnnuhub_ocr::BitmapOcr;

const PATH: &str = "/kbcx/xskbcx_cxXsgrkb.html";

/// 描述一次 POST 的结果
async fn post_kb(
    session: &Session,
    xnm: &str,
    xqm: &str,
    tag: &str,
) -> Result<usize, gnnuhub_core::Error> {
    let form = [
        ("xnm", xnm),
        ("xqm", xqm),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
    ];
    let body = session
        .post_form_for_probe(PATH, &[("gnmkdm", "N2151")], &form, &[])
        .await?;
    let trimmed = body.trim();
    let len = trimmed.len();
    let kind = if trimmed == "null" {
        "null"
    } else if trimmed.starts_with('{') {
        "json 对象"
    } else {
        "其他"
    };
    println!("  [{tag}] xnm={xnm:?} xqm={xqm:?} → {kind}，{len} 字节");
    if kind == "json 对象" {
        // 打印 kbList 长度
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let n = v
                .get("kbList")
                .and_then(|k| k.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            println!("        kbList = {n} 条");
        }
    }
    Ok(trimmed.len())
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

    println!("[0] 登录...");
    let client = Client::with_defaults()?;
    let session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    // A: 与 verify_endpoints 完全同形状
    println!("[A] 直接 POST（同 verify_endpoints 形状）");
    let a_len = post_kb(&session, "2026", "3", "A").await?;

    // B: A 为 null 时重试
    if a_len == 4 {
        println!("[B] A 返回 null，立即重试");
        let b_len = post_kb(&session, "2026", "3", "B").await?;
        if b_len == 4 {
            // C: 先访问课表页面建立上下文
            println!("[C] 仍为 null；先 GET 课表 Index 页再 POST");
            let _ = session
                .fetch_raw("/kbcx/xskbcx_cxXskbcxIndex.html?gnmkdm=N2151&layout=default")
                .await?;
            let c_len = post_kb(&session, "2026", "3", "C").await?;
            if c_len == 4 {
                // D: 空值语义
                println!("[D] 仍为 null；改传 xnm/xqm 空串");
                post_kb(&session, "", "", "D").await?;
            }
        }
    } else {
        println!("    → A 直接成功，判定为瞬态 null（建议：解析层容忍 null + 会话层重试一次）");
        return Ok(());
    }

    // E: 拉当前课表 Index 页，看 JS ver 是否更新、页面是否出现 #validate 验证码
    println!("\n[E] GET 课表 Index 页（检查 ver 与 #validate）");
    let html = session
        .fetch_raw("/kbcx/xskbcx_cxXskbcxIndex.html?gnmkdm=N2151&layout=default")
        .await?;
    let _ = std::fs::write("tools/js-recon/out/pages/kbcx_index_now.html", &html);
    println!(
        "    {} 字节，含 #validate: {}",
        html.len(),
        html.contains("\"validate\"") || html.contains("id=\"validate\"")
    );
    let vers: Vec<&str> = html
        .match_indices("ver=")
        .filter_map(|(i, _)| html.get(i + 4..i + 14))
        .map(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit()))
        .collect();
    println!("    ver 引用: {vers:?}");

    // F: 下载新版课表 JS（ver 与存档 29769310 不同才下）
    for src in extract_js(&html) {
        if src.contains("kbcx/xskbcx") && !src.contains("ver=29769310") {
            let path = if src.starts_with('/') {
                src.clone()
            } else {
                format!("/js/comp/jwglxt/pkgl/kbcx/{src}")
            };
            match session.fetch_raw(&path).await {
                Ok(js) => {
                    let name = src.replace(['/', '?'], "_");
                    let _ = std::fs::write(format!("tools/js-recon/out/scripts/{name}"), &js);
                    println!("    新版 JS 已存档: {name}（{} 字节）", js.len());
                }
                Err(e) => println!("    {src}: ❌ {e}"),
            }
        }
    }

    // G: 补上 kclxdm（前端 paramMap 有 6 个字段，我们只发了 5 个）
    println!("\n[G] 补 kclxdm 再试");
    post_kb_with_kclxdm(&session, "2026", "3").await?;

    // H: 改用 xskbcx.js 里 jqGrid 真正的 url（首页/个人课表查询接口）
    println!("\n[H] 改用 sykbcx_cxSykbcxxsIndex.html?doType=query");
    post_sykbcx(&session, "2026", "3").await?;

    // I: 历史学期对照
    probe_history(&session).await?;

    // J: 完整浏览器模拟——gnmkdm 只放 Referer（课表接口的过滤器
    //    可能与学籍接口不同：只认 Referer）
    println!("\n[J] Referer 模拟（gnmkdm 不在查询串）");
    let form = [
        ("xnm", "2025"),
        ("xqm", "12"),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let body = session
        .post_form_for_probe(
            PATH,
            &[],
            &form,
            &[(
                "Referer",
                "https://jwgl.gnnu.edu.cn/kbcx/xskbcx_cxXskbcxIndex.html?gnmkdm=N2151&layout=default",
            )],
        )
        .await?;
    let t = body.trim();
    println!(
        "  [J] → {} 字节，开头: {}",
        t.len(),
        t.chars().take(120).collect::<String>()
    );

    // K: AJAX 指纹矩阵——jQuery $.ajax 自带 X-Requested-With，
    //    服务端更新后可能开始校验 AJAX 指纹（此前从未实验过）
    println!("\n[K] AJAX 指纹矩阵");
    let form = [
        ("xnm", "2025"),
        ("xqm", "12"),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let referer =
        "https://jwgl.gnnu.edu.cn/kbcx/xskbcx_cxXskbcxIndex.html?gnmkdm=N2151&layout=default";
    let origin = "https://jwgl.gnnu.edu.cn";
    let matrix: &[(&str, &[(&str, &str)])] = &[
        ("K1 XRW", &[("X-Requested-With", "XMLHttpRequest")]),
        (
            "K2 XRW+Ref",
            &[("X-Requested-With", "XMLHttpRequest"), ("Referer", referer)],
        ),
        (
            "K3 XRW+Ref+Origin+Accept",
            &[
                ("X-Requested-With", "XMLHttpRequest"),
                ("Referer", referer),
                ("Origin", origin),
                ("Accept", "application/json, text/javascript, */*; q=0.01"),
            ],
        ),
    ];
    for (tag, headers) in matrix {
        let body = session
            .post_form_for_probe(PATH, &[("gnmkdm", "N2151")], &form, headers)
            .await?;
        let t = body.trim();
        let mut detail = format!(
            "{} 字节，开头: {}",
            t.len(),
            t.chars().take(60).collect::<String>()
        );
        if t.starts_with('{')
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
            && let Some(list) = v.get("kbList").and_then(|k| k.as_array())
        {
            detail = format!("{} 字节，kbList={}", t.len(), list.len());
            if !list.is_empty() {
                let _ = std::fs::write("tools/js-recon/out/pages/kb_SUCCESS.json", t);
            }
        }
        println!("  [{tag}] → {detail}");
    }

    // L: 补齐浏览器登录后的上下文——首页三件套（index/initMenu/用户信息）
    //    可能在服务端初始化会话属性，或 Set-Cookie 额外值
    println!("\n[L] 补齐首页上下文后再试");
    for path in [
        "/xtgl/index.html",
        "/xtgl/index_initMenu.html",
        "/xtgl/index_cxYhxxIndex.html",
    ] {
        match session.fetch_raw(path).await {
            Ok(html) => println!("    GET {path} → {} 字节", html.len()),
            Err(e) => println!("    GET {path} → ❌ {e}"),
        }
    }
    // 打印当前会话 Cookie（对比浏览器用）
    let form2 = [
        ("xnm", "2025"),
        ("xqm", "12"),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let body = session
        .post_form_for_probe(PATH, &[("gnmkdm", "N2151")], &form2, &[])
        .await?;
    let t = body.trim();
    println!(
        "  [L] → {} 字节，开头: {}",
        t.len(),
        t.chars().take(80).collect::<String>()
    );

    Ok(())
}

/// 从 HTML 提取 script src
fn extract_js(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("<script") {
        rest = &rest[pos..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        if let Some(i) = tag.find("src=\"") {
            let body = &tag[i + 5..];
            if let Some(j) = body.find('"') {
                out.push(body[..j].to_string());
            }
        }
        rest = &rest[end..];
    }
    out
}

/// G 实验：带 kclxdm 的 POST
async fn post_kb_with_kclxdm(
    session: &Session,
    xnm: &str,
    xqm: &str,
) -> Result<(), gnnuhub_core::Error> {
    let form = [
        ("xnm", xnm),
        ("xqm", xqm),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let body = session
        .post_form_for_probe(PATH, &[("gnmkdm", "N2151")], &form, &[])
        .await?;
    println!("  [G] → {} 字节，开头: {}", body.trim().len(), {
        let t = body.trim();
        t.chars().take(80).collect::<String>()
    });
    Ok(())
}

/// H 实验：首页/个人课表数据源
async fn post_sykbcx(session: &Session, xnm: &str, xqm: &str) -> Result<(), gnnuhub_core::Error> {
    let form = [
        ("xnm", xnm),
        ("xqm", xqm),
        ("kzlx", "ck"),
        ("xsdm", ""),
        ("kclbdm", ""),
        ("kclxdm", ""),
    ];
    let body = session
        .post_form_for_probe(
            "/xssygl/sykbcx_cxSykbcxxsIndex.html",
            &[("gnmkdm", "N2151"), ("doType", "query")],
            &form,
            &[],
        )
        .await?;
    let t = body.trim();
    println!(
        "  [H] → {} 字节，开头: {}",
        t.len(),
        t.chars().take(120).collect::<String>()
    );
    if t.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
        && let Some(list) = v.get("kbList").and_then(|k| k.as_array())
    {
        println!("      kbList = {} 条", list.len());
        if let Some(first) = list.first() {
            println!(
                "      首条: {}",
                serde_json::to_string(first)
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>()
            );
        }
        let _ = std::fs::write("tools/js-recon/out/pages/kb_sykb.json", t);
    }
    Ok(())
}

/// I 实验：查历史学期——决定「null = 2026 学年课表未发布」还是接口层面问题
async fn probe_history(session: &Session) -> Result<(), gnnuhub_core::Error> {
    println!("\n[I] 历史学期对照（此前 2025-2026-2 有 9 门课）");
    for (xnm, xqm, label) in [
        ("2025", "12", "2025-2026 第二学期"),
        ("2025", "3", "2025-2026 第一学期"),
        ("2026", "3", "2026-2027 第一学期(当前)"),
    ] {
        let form = [
            ("xnm", xnm),
            ("xqm", xqm),
            ("kzlx", "ck"),
            ("xsdm", ""),
            ("kclbdm", ""),
            ("kclxdm", ""),
        ];
        let body = session
            .post_form_for_probe(PATH, &[("gnmkdm", "N2151")], &form, &[])
            .await?;
        let t = body.trim();
        let info = if t == "null" {
            "null".to_string()
        } else {
            serde_json::from_str::<serde_json::Value>(t)
                .ok()
                .and_then(|v| {
                    let n = v.get("kbList").and_then(|k| k.as_array()).map(|a| a.len());
                    n.map(|n| format!("kbList={n}"))
                })
                .unwrap_or_else(|| "非JSON".into())
        };
        println!(
            "  [{label}] xnm={xnm} xqm={xqm} → {info}（{} 字节）",
            t.len()
        );
    }
    Ok(())
}

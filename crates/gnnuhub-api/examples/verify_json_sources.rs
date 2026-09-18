//! JSON 数据源验证工具（第二轮）
//!
//! # 背景
//!
//! 项目约定：**数据获取优先走返回 JSON 的接口，尽量不解析 HTML**
//! （用户 2026-09-18 明确要求）。第一轮探测留下两个失败点，本轮
//! 依据新的离线证据做最小改动复测：
//!
//! 1. **学籍信息 JSON**：`/xsxxxggl/xsxxwh_cxCkDgxsxx.html`
//!
//!    第一轮三种表单变体全部返回「无功能权限」（同一个 2.5KB
//!    错误页）。新的离线证据：本站能用的接口（课表 `N2151`、
//!    成绩 `N305005`）都在**查询串**里带 `gnmkdm`，而 `ckXsxx.js`
//!    调本接口时 URL 是裸路径——浏览器场景下 gnmkdm 藏在
//!    **Referer** 里（页面地址 `?gnmkdm=N100801`）。
//!    假设：正方权限过滤器从「查询串或 Referer」取 gnmkdm，
//!    两者皆无则拒绝。验证矩阵（逐级、命中即停）：
//!
//!    - **D**：查询串补 `gnmkdm=N100801`（相对第一轮 A 的单变量改动）
//!    - **E**：裸路径 + 浏览器 AJAX 指纹（`Referer` + `X-Requested-With`）
//!    - **E2**：两者都带（仅当前两个都失败时）
//!
//!    若任一形态命中，再回答第一轮遗留的两个问题：
//!    - **F1**：去掉 `xh_id_code` 还能用吗（能 → 免抓 HTML 页面取码）
//!    - **F2**：再去掉 `xnm`/`xqm`（能 → 参数最小集就是学号）
//!
//! 2. **单周课表**：放弃移动端 `xskbcxMobile_cxXsKb.html`（第一轮
//!    返回字面量 `null`，且 Python 原作者也从未实测过该路径），
//!    改走菜单里的正式功能 `N2154 学生课表查询（按周次）`。
//!    本轮先抓它的页面 `/kbcx/xskbcxZccx_cxXskbcxIndex.html`
//!    与其专属 JS 存档（该 JS 此前从未抓过），离线分析出真正的
//!    数据接口后再实测；顺带用真实页面验证 `parse_this_week`
//!    （`this_week()` 至今未实测）。
//!
//!    离线分析的结论：**Zccx 页的 JS 调用的正是那个移动端接口**
//!    （`cxXskbcx.js:108` → `xskbcxMobile_cxXsKb.html`），表单形状
//!    与第一轮逐字段一致。因此第 [4] 步在「已访问页面的同会话」里
//!    做 M1（浏览器复刻：AJAX 指纹头）/ M3（第一轮复刻：查询串
//!    gnmkdm、无头）对照，定位 null 的成因。
//!
//! 运行：
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example verify_json_sources
//! ```
//!
//! 网络请求预算：登录 ~5 + 学籍页 1 + cxCkDgxsxx 2~4 + Zccx 页 1
//! + Zccx JS ≤1 + 移动端课表 2 ≈ **12~14 次**。

use std::path::PathBuf;

use gnnuhub_api::parse;
use gnnuhub_api::{Client, Error, Session};
use gnnuhub_core::JWGL_BASE_URL;
use gnnuhub_ocr::BitmapOcr;

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
    let xh = student_id.to_string();

    println!("[0] 登录...");
    let client = Client::with_defaults()?;
    let session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功: {}\n", session.student_id());

    let pages = PathBuf::from("tools/js-recon/out/pages");
    let scripts = PathBuf::from("tools/js-recon/out/scripts");
    let _ = std::fs::create_dir_all(&pages);

    // ---------- 1. 学籍页：取 xh_id_code（也是浏览器流程里激活 N100801 的那次 GET） ----------
    println!("[1] GET 学籍页（取 xh_id_code）");
    let page = session
        .fetch_raw("/xsxxxggl/xsgrxxwh_cxXsgrxx.html?gnmkdm=N100801&layout=default")
        .await?;
    let code = extract_hidden_value(&page, "curXh_id_code").ok_or("学籍页里没有 curXh_id_code")?;
    println!("    xh_id_code = {code}\n");

    // ---------- 2. cxCkDgxsxx：先确定能通过权限的请求形状 ----------
    let full_form: Vec<(&str, &str)> = vec![
        ("xh_id", xh.as_str()),
        ("xh_id_code", code.as_str()),
        ("fromXh_id", ""),
        ("xnm", "2026"),
        ("xqm", "3"),
    ];
    let referer =
        format!("{JWGL_BASE_URL}/xsxxxggl/xsgrxxwh_cxXsgrxx.html?gnmkdm=N100801&layout=default");
    let ajax_headers: Vec<(&str, &str)> = vec![
        ("Referer", referer.as_str()),
        ("X-Requested-With", "XMLHttpRequest"),
    ];

    struct Shape<'a> {
        tag: &'a str,
        desc: &'a str,
        query: Vec<(&'a str, &'a str)>,
        headers: Vec<(&'a str, &'a str)>,
    }
    let candidates = [
        Shape {
            tag: "D",
            desc: "查询串补 gnmkdm=N100801（相对第一轮 A 的单变量改动）",
            query: vec![("gnmkdm", "N100801")],
            headers: vec![],
        },
        Shape {
            tag: "E",
            desc: "裸路径 + Referer/X-Requested-With（模拟浏览器 AJAX 指纹）",
            query: vec![],
            headers: ajax_headers.clone(),
        },
        Shape {
            tag: "E2",
            desc: "gnmkdm + AJAX 指纹双保险",
            query: vec![("gnmkdm", "N100801")],
            headers: ajax_headers.clone(),
        },
    ];

    // 命中的请求形状（供 2b 复用）
    struct Working<'a> {
        tag: &'a str,
        query: Vec<(&'a str, &'a str)>,
        headers: Vec<(&'a str, &'a str)>,
    }
    let mut shape: Option<Working> = None;
    for cs in &candidates {
        println!("[2] cxCkDgxsxx {}：{}", cs.tag, cs.desc);
        match probe_xsxj(&session, &cs.query, &full_form, &cs.headers).await {
            Ok(body) => {
                let _ = std::fs::write(pages.join(format!("xsxj_{}.json", cs.tag)), &body);
                match json_object_key_count(&body) {
                    Some(n) if n > 0 => {
                        println!("    ✅ JSON 对象（{n} 个顶层键）——通过权限！");
                        describe_json_keys(&body, "    ");
                        shape = Some(Working {
                            tag: cs.tag,
                            query: cs.query.clone(),
                            headers: cs.headers.clone(),
                        });
                    }
                    _ => {
                        let head: String = body.trim().chars().take(100).collect();
                        println!("    ❌ 非 JSON（开头: {head}）");
                    }
                }
            }
            Err(e) => println!("    ❌ 请求失败: {e}"),
        }
        if shape.is_some() {
            break;
        }
        println!();
    }

    // ---------- 2b. 表单最小化（仅在找到能进门的形状后） ----------
    if let Some(cs) = shape {
        println!(
            "\n[2b] {} 形态通过权限，最小化表单（回答第一轮遗留问题）：",
            cs.tag
        );
        let f1: Vec<(&str, &str)> = vec![
            ("xh_id", xh.as_str()),
            ("fromXh_id", ""),
            ("xnm", "2026"),
            ("xqm", "3"),
        ];
        let f2: Vec<(&str, &str)> = vec![("xh_id", xh.as_str()), ("fromXh_id", "")];
        for (tag2, desc, form) in [
            ("F1", "去掉 xh_id_code", &f1),
            ("F2", "再去掉 xnm/xqm", &f2),
        ] {
            println!("    --- {tag2}：{desc} ---");
            match probe_xsxj(&session, &cs.query, form, &cs.headers).await {
                Ok(body) => {
                    let _ = std::fs::write(pages.join(format!("xsxj_{tag2}.json")), &body);
                    match json_object_key_count(&body) {
                        Some(n) if n > 0 => println!("    ✅ 仍是 JSON（{n} 个键）→ 该参数可省"),
                        _ => println!("    ❌ 不再是 JSON → 该参数必需"),
                    }
                }
                Err(e) => println!("    ❌ 请求失败: {e}"),
            }
        }
    } else {
        println!("\n[2] ❌ D/E/E2 全部被拒——「gnmkdm 藏在查询串或 Referer」假设不成立，停止枚举。");
    }

    // ---------- 3. 按周次课表页（N2154） ----------
    println!("\n[3] GET 按周次课表页 /kbcx/xskbcxZccx_cxXskbcxIndex.html");
    let zccx = session
        .fetch_raw("/kbcx/xskbcxZccx_cxXskbcxIndex.html?gnmkdm=N2154&layout=default")
        .await?;
    println!("    页面 {} 字节", zccx.len());
    let _ = std::fs::write(pages.join("zccx_index.html"), &zccx);

    // this_week() 的解析源正是这个页面——首次实测
    match parse::parse_this_week(&zccx) {
        Ok(w) => println!("    ✅ parse_this_week 实测通过：当前第 {w} 周"),
        Err(e) => println!("    ❌ parse_this_week 失败: {e}"),
    }

    // 拉取页面引用的、与课表相关且尚未存档的 JS
    let js_urls = extract_js_urls(&zccx);
    let interesting: Vec<&String> = js_urls
        .iter()
        .filter(|u| {
            let l = u.to_lowercase();
            l.contains("kbcx") || l.contains("zccx")
        })
        .collect();
    println!(
        "    页面共引用 {} 个 JS，其中课表相关 {} 个",
        js_urls.len(),
        interesting.len()
    );
    for url in interesting.iter().take(2) {
        let name = js_file_name(url);
        if pages.join(&name).exists() || scripts.join(&name).exists() {
            println!("    已有存档，跳过: {name}");
            continue;
        }
        println!("    拉取 {url}");
        match session.fetch_raw(url).await {
            Ok(js) => {
                println!("    {} 字节 → pages/{name}", js.len());
                let _ = std::fs::write(pages.join(&name), &js);
            }
            Err(e) => println!("    ❌ 拉取失败: {e}"),
        }
    }

    // ---------- 4. 移动端单周课表：同会话复测 ----------
    // 浏览器的调用顺序是「先 GET Zccx 页 → 同会话内 POST 接口」，
    // 且请求带 Referer/X-Requested-With。第一轮是冷会话 + 无头的
    // 裸 POST，得到字面量 null。本步骤在同一会话内做两次对照：
    //   M1 浏览器复刻：无查询串 + AJAX 指纹头
    //   M3 第一轮复刻：查询串 gnmkdm + 无自定义头
    // 两者结果交叉即可区分「页面访问带来的会话状态」与「请求头」
    // 哪个才是 null 的真正原因。
    println!("\n[4] POST /kbcx/xskbcxMobile_cxXsKb.html（Zccx 页已访问的同会话内）");
    let kb_form: Vec<(&str, &str)> = vec![
        ("xnm", "2026"),
        ("xqm", "3"),
        ("zs", "2"),
        ("doType", "app"),
        ("kblx", "1"),
        ("xh", ""),
    ];
    let zccx_referer =
        format!("{JWGL_BASE_URL}/kbcx/xskbcxZccx_cxXskbcxIndex.html?gnmkdm=N2154&layout=default");
    let kb_headers: Vec<(&str, &str)> = vec![
        ("Referer", zccx_referer.as_str()),
        ("X-Requested-With", "XMLHttpRequest"),
    ];

    println!("    --- M1 浏览器复刻（无查询串 + Referer/X-Requested-With） ---");
    match session
        .post_form_for_probe("/kbcx/xskbcxMobile_cxXsKb.html", &[], &kb_form, &kb_headers)
        .await
    {
        Ok(body) => {
            let _ = std::fs::write(pages.join("kb_M1.json"), &body);
            report_kb(&body, "    ");
        }
        Err(e) => println!("    ❌ 请求失败: {e}"),
    }

    println!("    --- M3 第一轮复刻（查询串 gnmkdm、无自定义头） ---");
    match session
        .post_form_for_probe(
            "/kbcx/xskbcxMobile_cxXsKb.html",
            &[("gnmkdm", "N2154")],
            &kb_form,
            &[],
        )
        .await
    {
        Ok(body) => {
            let _ = std::fs::write(pages.join("kb_M3.json"), &body);
            report_kb(&body, "    ");
        }
        Err(e) => println!("    ❌ 请求失败: {e}"),
    }

    println!("\n完成。");
    Ok(())
}

/// 对 cxCkDgxsxx 发一次指定形状的 POST
async fn probe_xsxj(
    session: &Session,
    query: &[(&str, &str)],
    form: &[(&str, &str)],
    headers: &[(&str, &str)],
) -> Result<String, Error> {
    session
        .post_form_for_probe("/xsxxxggl/xsxxwh_cxCkDgxsxx.html", query, form, headers)
        .await
}

/// 若 body 是非空 JSON 对象，返回其顶层键数；否则 None
fn json_object_key_count(body: &str) -> Option<usize> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .as_object()
        .map(|o| o.len())
}

/// 打印 JSON 顶层键清单（学籍 JSON 的键就是字段名，这一步就是契约本身）
fn describe_json_keys(body: &str, indent: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    let Some(obj) = v.as_object() else {
        return;
    };
    for k in obj.keys() {
        let val = obj[k].to_string();
        let preview: String = val.chars().take(30).collect();
        println!("{indent}  {:<14} = {}", k, preview);
    }
}

/// 打印移动端课表响应的关键信息
fn report_kb(body: &str, indent: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        let head: String = body.trim().chars().take(100).collect();
        println!("{indent}❌ 非 JSON（开头: {head}）");
        return;
    };
    if v.is_null() {
        println!("{indent}❌ 字面量 null（与第一轮相同）");
        return;
    }
    let count = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(Vec::len)
            .unwrap_or(0)
    };
    println!(
        "{indent}✅ JSON 对象：kbList={} sjkList={} rqazcList={}",
        count("kbList"),
        count("sjkList"),
        count("rqazcList")
    );
    let keys: Vec<&String> = v
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    println!(
        "{indent}顶层键（{} 个）: {}",
        keys.len(),
        keys.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// 从 HTML 里提取所有 `<script src="...">` 的 JS 地址（绝对路径）
fn extract_js_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("src=\"") {
        rest = &rest[pos + 5..];
        let Some(end) = rest.find('"') else { break };
        let url = &rest[..end];
        if url.contains(".js") && url.starts_with('/') {
            urls.push(url.to_string());
        }
        rest = &rest[end + 1..];
    }
    urls.sort();
    urls.dedup();
    urls
}

/// 把 JS 地址转成存档文件名（与 tools/js-recon 的命名约定一致）：
/// `/js/comp/x.js?ver=123` → `js_comp_x.js_ver_123`
fn js_file_name(url: &str) -> String {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    let mut name = path.trim_start_matches('/').replace('/', "_");
    if let Some(q) = query {
        name.push('_');
        name.push_str(&q.replace('=', "_"));
    }
    name
}

/// 从 HTML 里取指定 id 的 hidden input 的 value
fn extract_hidden_value(html: &str, id: &str) -> Option<String> {
    let mut rest = html;
    while let Some(pos) = rest.find("<input") {
        rest = &rest[pos..];
        let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
        let tag = &rest[..end];
        rest = &rest[end..];
        let has_id = format!("id=\"{id}\"");
        if tag.contains(&has_id) {
            let needle = "value=\"";
            if let Some(s) = tag.find(needle) {
                let after = &tag[s + needle.len()..];
                if let Some(e) = after.find('"') {
                    return Some(after[..e].to_string());
                }
            }
            return Some(String::new());
        }
    }
    None
}

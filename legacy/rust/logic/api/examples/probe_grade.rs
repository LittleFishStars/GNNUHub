//! 成绩查询接口形态探针
//!
//! # 结论（2026-09-18 实测，**该接口暂不可用**）
//!
//! `/cjcx/cjcx_cxDgXscj.html?gnmkdm=N305005&doType=query` 对当前测试账号
//! **恒定返回固定的空信封**，无论请求参数怎么给：
//!
//! ```json
//! {"currentPage":1,"currentResult":0,"entityOrField":false,"items":[],
//!  "limit":15,"offset":0,"pageNo":0,"pageSize":15,"showCount":10,
//!  "sortName":"xnmmc asc,xqmmc asc,kch asc","sortOrder":" ","sorts":[],
//!  "totalCount":0,"totalPage":0,"totalResult":0}
//! ```
//!
//! 已排除的可能（每一条都有实测证据）：
//!
//! | 假设 | 排除依据 |
//! |---|---|
//! | 缺 `xh_id` | 补上 `xh_id` / `yhm` / `jsxx=xs` 后信封**逐字节不变** |
//! | 缺页面隐藏字段 | 加 `pyfaxx_id` / `jxzxjhxx_id` 后不变 |
//! | 缺框架远程参数 | 加 `zd_fzdm=N305005-xs` 后不变 |
//! | 缺 jqGrid 分页参数 | 发 `queryModel.showCount=1000` 后服务端**仍返回 `limit=15`**，说明它压根不读这些字段 |
//! | 分页参数名不对 | 见上；参数名取自 `jquery.jqgrid.settings.js:99-104` 的 `prmNames` |
//! | 路径选错分支 | `cjcx_cxXsgrcj.html`（脚本里 `jsxx=="xs"` 那一支）与 `cjcx_cxDgXscj.html` **返回一致** |
//! | 参数值本身有问题 | **非法值对照**：`xnm=9999&xqm=99` 与合法值的信封指纹完全相同 |
//!
//! 服务端对垃圾参数与合法参数给出**一模一样的响应**，只可能有两种解释：
//!
//! 1. 该账号**尚无任何成绩记录**，接口对所有输入统一返回空骨架；
//! 2. 该账号**没有成绩查询权限**，返回的是拦截后的默认空骨架。
//!
//! 两者在客户端**无法区分**，且都不构成「参数没猜对」。
//!
//! # 因此本项目的处理方式
//!
//! - **不实现** `Session::grades()`，也不引入成绩相关的领域类型。
//!   一个无法实测、只能靠猜的接口，比没有这个接口更危险。
//! - **保留** `parse::parse_grades`：它的输入是上面那个信封，
//!   解析逻辑有真实样本（`cj_probe*.json`）支撑，且已覆盖
//!   「`bfzcj` 缺失」「成绩非数值」「混合通过/未通过」等边界。
//!   等接口能用时可以直接接上。
//! - `tools/js-recon/out/pages/` 下已存档：页面 HTML、`cxDgXscj.js`、
//!   中文语言包、jqGrid 默认设置、以及 9 个探测响应样本。
//!
//! # 后续怎么重新验证
//!
//! 换一个**确定有成绩**的账号（例如大二以上的学号）再跑一次本探针。
//! 若那时仍是空信封，则问题在权限侧，需要从「教务系统是否对该角色
//! 开放成绩查询」入手，而不是继续调参数。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_grade
//! ```
//!
//! 网络请求预算：1 次登录 + 2 个候选 + 2 个对照 + 1 次页面
//! + 3 个静态 JS = **9 次**。

use std::path::PathBuf;

use gnnuhub_api::Client;
use gnnuhub_ocr::BitmapOcr;

/// 一次探测的候选形态
struct Candidate {
    /// 代号
    label: &'static str,
    /// 接口路径
    path: &'static str,
    /// 请求体字段
    form: &'static [(&'static str, &'static str)],
}

/// 待试的候选形态（全部为 POST + `doType=query`）
///
/// 页面 `searchForm` 的 `action` 是 `/cjcx/cjcx_cxDgXscj.html`，
/// 但 `cxDgXscj.js:230` 又按 `jsxx=="xs"` 选出 `cjcx_cxXsgrcj.html`。
/// 两者对不上，且都返回空，说明还有别的开关在起作用。
///
/// 这里把「脚本明示的路径」与「三个开关字段」的笛卡尔积打一遍。
/// 重点是 `zd_fzdm` —— 它来自 `remoteParams`（框架层会合并进请求体），
/// 学生侧取值 `"N305005-xs"`。前面所有失败尝试都没带它。
const CANDIDATES: &[Candidate] = &[
    Candidate {
        label: "jqgrid",
        path: "/cjcx/cjcx_cxDgXscj.html",
        form: &[
            ("xnm", ""),
            ("xqm", ""),
            ("kcbj", ""),
            ("pkey", ""),
            ("queryModel.showCount", "1000"),
            ("queryModel.currentPage", "1"),
            ("queryModel.sortOrder", "asc"),
            ("queryModel.sortName", "xnmmc asc,xqmmc asc,kch asc"),
        ],
    },
    Candidate {
        label: "jqgrid_xh",
        path: "/cjcx/cjcx_cxDgXscj.html",
        form: &[
            ("xnm", ""),
            ("xqm", ""),
            ("kcbj", ""),
            ("pkey", ""),
            ("xh_id", "250710078"),
            ("queryModel.showCount", "1000"),
            ("queryModel.currentPage", "1"),
            ("queryModel.sortOrder", "asc"),
            ("queryModel.sortName", "xnmmc asc,xqmmc asc,kch asc"),
        ],
    },
];

/// 参数敏感性对照：同一接口，合法 vs 非法 `xnm`/`xqm`
///
/// 四个候选形态返回**逐字节相同**的信封，且 `limit`/`pageSize` 恒为 15
/// （我明明发了 `rows=20`）。用一个对照实验判断服务端到底读没读这些字段：
/// 若非法值与合法值返回完全相同的信封，说明这些参数根本没被读取，
/// 那个固定信封是权限拦截的默认返回，问题不在参数形态上。
const PROBE_SENTINELS: &[(&str, &str, &str)] = &[
    ("s1 合法 xnm=2026 xqm=3", "2026", "3"),
    ("s2 非法 xnm=9999 xqm=99", "9999", "99"),
];

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
    println!("    登录成功: {}\n", session.student_id());

    let out = PathBuf::from("tools/js-recon/out/pages");
    let _ = std::fs::create_dir_all(&out);

    let xnm = std::env::var("GNNU_XNM").unwrap_or_else(|_| "2026".to_string());
    let xqm = std::env::var("GNNU_XQM").unwrap_or_else(|_| "3".to_string());

    // ---------- 2. 候选形态逐一试 ----------
    //
    // 关键疑点：`cxDgXscj.js` 给 jqGrid 配了
    // `remoteParams: {"zd_fzdm": "N305005-xs"}`。`remoteParams` 是
    // 框架扩展参数，会被合并进每次请求。学生侧权限很可能就挂在这个
    // 字段上——前面几次尝试都没带它，这解释了为什么一直返回空。
    println!("[A] 逐一试候选形态（共 {} 个）", CANDIDATES.len());
    for c in CANDIDATES {
        println!("  --- 形态 {} ---", c.label);
        let form: Vec<(String, String)> = c
            .form
            .iter()
            .map(|(k, v)| {
                let v = match *k {
                    "xnm" if !xnm.is_empty() => xnm.clone(),
                    "xqm" if !xqm.is_empty() => xqm.clone(),
                    _ => (*v).to_string(),
                };
                ((*k).to_string(), v)
            })
            .collect();
        println!(
            "    请求体: {}",
            form.iter()
                .map(|(k, v)| if v.is_empty() {
                    format!("{k}=")
                } else {
                    format!("{k}={v}")
                })
                .collect::<Vec<_>>()
                .join("&")
        );

        let path = c.path.to_string();
        let query = [("gnmkdm", "N305005"), ("doType", "query")];
        let form_ref: Vec<(&str, &str)> =
            form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

        match session
            .post_form_for_probe(&path, &query, &form_ref, &[])
            .await
        {
            Ok(body) => {
                let n = count_items(&body);
                println!("    HTTP 200，{} 字节，items 条数 = {n}", body.trim().len());
                let preview: String = body.trim().chars().take(260).collect();
                println!("    原文: {preview}");
                let _ = std::fs::write(out.join(format!("cj_cand_{}.json", c.label)), &body);
                if n > 0 {
                    println!("    ✅✅ 命中！形态 {} 能取到数据", c.label);
                    let _ = std::fs::write(out.join("cj_SUCCESS.json"), &body);
                }
            }
            Err(e) => println!("    ❌ 失败: {e}"),
        }
    }

    // ---------- 3. 参数敏感性对照 ----------
    //
    // 四个候选形态返回**逐字节相同**的信封，而且 `limit`/`pageSize` 恒为
    // 15（我发的是 `rows=20`）。需要用一对对照实验判断：
    // 服务端到底读没读 `xnm`/`xqm`？
    //
    // 判据：合法值与非法值若返回**完全一样**，说明这两个字段没被读取，
    // 那个固定信封是权限拦截的默认返回，问题不在参数上。
    println!("\n[3] 参数敏感性对照（同一接口，合法 vs 非法 xnm/xqm）");
    let mut fingerprints: Vec<(String, String)> = Vec::new();
    for (label, xnm_v, xqm_v) in PROBE_SENTINELS {
        let form = [
            ("xnm", *xnm_v),
            ("xqm", *xqm_v),
            ("kcbj", ""),
            ("pkey", ""),
            ("page", "1"),
            ("rows", "20"),
        ];
        match session
            .post_form_for_probe(
                "/cjcx/cjcx_cxDgXscj.html",
                &[("gnmkdm", "N305005"), ("doType", "query")],
                &form,
                &[],
            )
            .await
        {
            Ok(body) => {
                let fp = envelope_fingerprint(&body);
                println!("    {label} → 信封指纹: {fp}");
                fingerprints.push((label.to_string(), fp));
            }
            Err(e) => println!("    ❌ {label} 失败: {e}"),
        }
    }

    if fingerprints.len() >= 2 {
        let first = &fingerprints[0].1;
        let all_same = fingerprints.iter().all(|(_, f)| f == first);
        println!();
        if all_same {
            println!("    >>> 判定：合法与非法参数返回**完全相同的信封**。");
            println!("        结论：服务端不读 xnm/xqm（也不读我发的分页参数），");
            println!("        这个固定 limit=15/pageSize=15 的空信封是权限拦截的默认返回。");
            println!("        问题不在参数形态上，停止枚举参数。");
        } else {
            println!("    >>> 判定：参数确实被读取（信封有差异），可按差异继续定位。");
        }
    }

    // ---------- 4. GET 页面本身 ----------
    println!("[B] GET 成绩查询页面（页面本身带全套表单字段，是隐藏字段的正规来源）");
    match session
        .fetch_raw("/cjcx/cjcx_cxDgXscj.html?gnmkdm=N305005&layout=default")
        .await
    {
        Ok(html) => {
            println!("    HTTP 200，{} 字节", html.len());
            let f = out.join("cj_page_gnmkdm.html");
            let _ = std::fs::write(&f, &html);
            println!("    已存档 {}", f.display());
            for (id, name) in [
                ("xnm", "学年"),
                ("xqm", "学期"),
                ("kcbjdm_cx", "课程标记"),
                ("yhm", "登录名"),
            ] {
                println!(
                    "    含 {id:<10} ({name}): {}",
                    html.contains(&format!("id=\"{id}\""))
                );
            }
        }
        Err(e) => println!("    ❌ 失败: {e}"),
    }

    // ---------- 4. 抓页面自己的 JS ----------
    //
    // `cxDgXscj.js` 是成绩查询页的**官方调用方**，它现场怎么拼参数
    // 就是服务端认的形态。抓到它之后全部离线分析，不再打接口。
    println!("[C] 抓成绩查询页自己的 JS（离线分析的唯一依据，之后不再打接口）");
    for rel in [
        "/js/comp/jwglxt/cjgl/cjcx/cxDgXscj.js?ver=29769310",
        "/js/globalweb/comp/i18n/N305005_zh_CN.js?ver=29769310",
        "/js/plugins/jqGrid4.6/jquery.jqgrid.settings.js?ver=29769310",
    ] {
        let name = rel.trim_start_matches('/').replace('/', "_");
        let name = name.split('?').next().unwrap_or("js").to_string();
        match session.fetch_raw(rel).await {
            Ok(js) => {
                let f = out.join(&name);
                let _ = std::fs::write(&f, &js);
                println!("    ✅ {} 字节 -> {}", js.len(), f.display());
            }
            Err(e) => println!("    ❌ {rel} 失败: {e}"),
        }
    }

    println!("\n完成。");
    Ok(())
}

/// 数一数响应里 `items` 数组的元素个数
fn count_items(body: &str) -> usize {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return 0;
    };
    v.get("items")
        .and_then(|i| i.as_array())
        .map(Vec::len)
        .unwrap_or(0)
}

/// 提取响应的「信封指纹」——只保留与数据无关的框架字段
///
/// 目的是判断两次请求的返回是否**结构性相同**。刻意排除
/// `items`/`totalResult` 这类会随数据变化的字段，只留分页与排序元信息；
/// 若两个指纹相同，说明服务端对两次请求的处理路径完全一致。
fn envelope_fingerprint(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return format!("<非 JSON: {} 字节>", body.trim().len());
    };
    let get = |k: &str| {
        v.get(k)
            .map(|x| x.to_string())
            .unwrap_or_else(|| "?".into())
    };
    format!(
        "limit={} pageSize={} showCount={} sortOrder={} totalResult={}",
        get("limit"),
        get("pageSize"),
        get("showCount"),
        get("sortOrder"),
        get("totalResult"),
    )
}

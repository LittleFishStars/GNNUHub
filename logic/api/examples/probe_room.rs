//! 空闲教室（N2155）查询实测探针
//!
//! # 请求形状（离线逆向 djks_index.html 的 searchForm + kxcdlb.js）
//!
//! - POST `/cdjy/cdjy_cxKxcdlb.html?gnmkdm=N2155&doType=query`
//! - remoteParams：`zd_fzdm=N211205-kxcdlb`
//! - body = searchForm 全字段（含预填的当前学生 syr/syrxm）+
//!   jqGrid 分页参数（`queryModel.showCount`）
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_room
//! ```

use gnnuhub_api::{Client, Session};
use gnnuhub_ocr::BitmapOcr;

const OUT_DIR: &str = "tools/js-recon/out";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("gnnuhub_api=warn")),
        )
        .with_target(false)
        .init();
    let _ = std::fs::create_dir_all(format!("{OUT_DIR}/pages"));

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    let student_id_str = student_id.to_string();
    println!("[1/2] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    println!("[2/2] 空闲教室查询——形状对照矩阵...");
    // 基础字段（searchForm serialize 的核心部分）
    let base: &[(&str, &str)] = &[
        ("xnm", "2026"),
        ("xqm", "3"),
        ("cdyysqkz", "0"),
        ("glrycdyysfxysp", "1"),
        ("xqFlag", "0"),
        ("syr", student_id_str.as_str()),
        ("syrxm", ""),
        ("yuyjsctbj", "1"),
        ("dm", ""),
        ("xqh_id", ""),
        ("lh", ""),
        ("cdlb_id", ""),
        ("cdejlb_id", ""),
    ];
    let page: &[(&str, &str)] = &[
        ("queryModel.showCount", "20"),
        ("queryModel.currentPage", "1"),
    ];
    // 各形状 = base 的裁剪/增补
    let mut variants: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "R1 全字段+zd_fzdm+分页（上轮失败形状）",
            base.iter()
                .chain(page.iter())
                .map(|(k, v)| (*k, *v))
                .chain([("zd_fzdm", "N211205-kxcdlb")])
                .collect(),
        ),
        (
            "R2 全字段+分页（无 zd_fzdm）",
            base.iter()
                .chain(page.iter())
                .map(|(k, v)| (*k, *v))
                .collect(),
        ),
        (
            "R3 极简：xnm/xqm/zd_fzdm/分页",
            [
                ("xnm", "2026"),
                ("xqm", "3"),
                ("zd_fzdm", "N211205-kxcdlb"),
                ("queryModel.showCount", "20"),
                ("queryModel.currentPage", "1"),
            ]
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect(),
        ),
        (
            "R4 全字段+pkey+localeKey+zd_fzdm",
            base.iter()
                .chain(page.iter())
                .map(|(k, v)| (*k, *v))
                .chain([
                    ("pkey", ""),
                    ("localeKey", "zh_CN"),
                    ("zd_fzdm", "N211205-kxcdlb"),
                ])
                .collect(),
        ),
        (
            "R5 极简+syryj&userModel 风格（走页面初始 local 语义）",
            [
                ("xnm", "2026"),
                ("xqm", "3"),
                ("xqh_id", "1"),
                ("zd_fzdm", "N211205-kxcdlb"),
                ("queryModel.showCount", "15"),
                ("queryModel.currentPage", "1"),
            ]
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect(),
        ),
    ];

    // S 系列：补时间段（WdatePicker 格式——qssj/jssj 日期，qssd/jssd HH:mm:00）
    let time_fields: &[(&str, &str)] = &[
        ("qssj", "2026-09-21"),
        ("jssj", "2026-09-25"),
        ("qssd", "08:00:00"),
        ("jssd", "22:00:00"),
    ];
    let mut s1: Vec<(&str, &str)> = base
        .iter()
        .chain(page.iter())
        .map(|(k, v)| (*k, *v))
        .chain([("zd_fzdm", "N211205-kxcdlb"), ("jyfs", "1")])
        .collect();
    s1.extend(time_fields.iter().map(|(k, v)| (*k, *v)));
    let mut s1b = s1.clone();
    s1b.retain(|(k, _)| *k != "jyfs");
    s1b.push(("jyfs", "3"));
    variants.push(("S1 全字段+时间段+jyfs=1", s1));
    variants.push(("S1b 全字段+时间段+jyfs=3", s1b));

    for (tag, form) in &variants {
        let form: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, *v)).collect();
        let outcome = session
            .post_form_for_probe(
                "/cdjy/cdjy_cxKxcdlb.html",
                &[("gnmkdm", "N2155"), ("doType", "query")],
                &form,
                &[],
            )
            .await;
        match outcome {
            Ok(body) => {
                let t = body.trim();
                let kind = if t.starts_with('{') || t.starts_with('[') {
                    "JSON".to_string()
                } else if t.contains("错误提示") {
                    "错误页".to_string()
                } else if t.starts_with('<') {
                    "HTML".to_string()
                } else {
                    "其他".to_string()
                };
                let mut extra = String::new();
                if kind == "JSON" {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
                        let items = v.pointer("/items").and_then(|x| x.as_array());
                        extra = format!("，items={:?}", items.map(|a| a.len()));
                        if let Some(a) = items
                            && !a.is_empty()
                        {
                            let _ = std::fs::write(format!("{OUT_DIR}/pages/room_SUCCESS.json"), t);
                        }
                    }
                }
                println!("    [{tag}] → {kind} {} 字节{extra}", t.len());
            }
            Err(e) => println!("    [{tag}] → ❌ {e}"),
        }
    }
    Ok(())
}

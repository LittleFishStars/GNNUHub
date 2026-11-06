//! 第二批探测：首页组件 JS（通知/消息数据源）+ 学业情况 GPA 视图 + 学业预警
//!
//! # 目标
//!
//! 1. 首页组件 loader（`index_v5.js` / `jwglxt_index.js`）——
//!    通知公告栏（#newsnotice）与 area 组件的数据端点就在其中
//! 2. 学业情况 GPA 视图：`xsxyqk_cxShowGpaView.html` /
//!    `xsxyqk_cxShowJqPjfView.html`（对话框页，确认数据是服务端
//!    渲染还是二次 AJAX）
//! 3. 学业预警检查：`xyyjgl/xyyj_cxZjxgsfyd.html`（本人在校是否预警）
//!
//! # 请求预算
//!
//! 登录 ~5 + JS 2 + GPA 2 + 预警 1 = 10。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run --release -p gnnuhub-api --example probe_more
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

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    println!("[1/3] 登录...");
    let client = Client::with_defaults()?;
    let session: Session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功\n");

    // ---- 1. 首页组件 JS ----
    println!("[2/3] 首页组件 JS 与 GPA/预警探测...");
    for (path, name) in [
        ("/js/globalweb/comp/xtgl/index_v5.js", "js_xtgl_index_v5.js"),
        (
            "/js/globalweb/comp/xtgl/jwglxt_index.js",
            "js_xtgl_jwglxt_index.js",
        ),
    ] {
        match session.fetch_raw(path).await {
            Ok(js) => {
                std::fs::write(format!("{OUT_DIR}/scripts/{name}"), &js)?;
                println!("    + {name}（{} 字节）", js.len());
            }
            Err(e) => println!("    {path}: ❌ {e}"),
        }
    }

    // GPA 视图（对话框页）
    for (path, name) in [
        ("/xsxy/xsxyqk_cxShowGpaView.html", "xsxyqk_gpa_view.html"),
        ("/xsxy/xsxyqk_cxShowJqPjfView.html", "xsxyqk_pjf_view.html"),
    ] {
        match session
            .post_form_for_probe(path, &[("gnmkdm", "N105515")], &[], &[])
            .await
        {
            Ok(html) => {
                std::fs::write(format!("{OUT_DIR}/pages/{name}"), &html)?;
                println!("    {name}: {} 字节", html.len());
            }
            Err(e) => println!("    {path}: ❌ {e}"),
        }
    }

    // 学业预警检查（POST，参数形态未知——先空表单试）
    match session
        .post_form_for_probe(
            "/xyyjgl/xyyj_cxZjxgsfyd.html",
            &[("gnmkdm", "N305516")],
            &[],
            &[],
        )
        .await
    {
        Ok(body) => {
            std::fs::write(format!("{OUT_DIR}/pages/xyyj_check.json"), &body)?;
            println!(
                "    学业预警检查: {} 字节，开头 {}",
                body.len(),
                body.trim().chars().take(80).collect::<String>()
            );
        }
        Err(e) => println!("    学业预警检查: ❌ {e}"),
    }

    // ---- 通知/消息/首页区域数据（离线已定位端点，验证响应格式）----
    println!("\n[3/3] 通知与首页区域数据...");
    for (name, path) in [
        ("通知新闻", "/xtgl/index_cxNews.html?localeKey=zh_CN"),
        ("首页区域一", "/xtgl/index_cxAreaOne.html?localeKey=zh_CN"),
        ("首页区域二", "/xtgl/index_cxAreaTwo.html?localeKey=zh_CN"),
        ("首页区域五", "/xtgl/index_cxAreaFive.html?localeKey=zh_CN"),
        ("系统提醒消息", "/xtgl/index_cxXttsxx.html?bj=2"),
    ] {
        match session.fetch_raw(path).await {
            Ok(body) => {
                let file = format!(
                    "{OUT_DIR}/pages/{}_{}",
                    name,
                    path.rsplit('/')
                        .next()
                        .unwrap_or("x")
                        .replace(['?', '='], "_")
                );
                std::fs::write(&file, &body)?;
                println!(
                    "    {name}: {} 字节 → {}",
                    body.len(),
                    body.trim().chars().take(100).collect::<String>()
                );
            }
            Err(e) => println!("    {name}: ❌ {e}"),
        }
    }

    println!("\n抓取完成，全部已落盘。");
    Ok(())
}

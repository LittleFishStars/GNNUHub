//! 会话接口实测工具（一次会话，验证多个接口）
//!
//! 目的：用**一次登录**把尚未验证的接口全部打一遍，把请求数压到最低。
//!
//! 验证项：
//! 1. 课表接口 `/kbcx/xskbcx_cxXsgrkb.html` —— 确认 `csrftoken` 是否被校验
//! 2. 学籍详细信息 `/xsxxxggl/xsgrxxwh_cxXsgrxx.html` —— 确认字段解析
//!
//! 运行：
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
//!   cargo run -p gnnuhub-api --example verify_endpoints
//! ```
//!
//! 验证码由内嵌位图字库自动识别，无需人工介入。

use std::path::PathBuf;

use gnnuhub_api::Client;
use gnnuhub_core::model::AcademicTerm;
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

    println!("[0] 登录...");
    let client = Client::with_defaults()?;
    let session = client
        .login(student_id, &password, &BitmapOcr::embedded()?)
        .await?;
    println!("    登录成功: {}\n", session.student_id());

    // ---------- 1. 课表接口 ----------
    println!("[1] 课表接口 /kbcx/xskbcx_cxXsgrkb.html");
    // 2026-2027 学年第一学期（xnm=2026, xqm=3）
    let term = AcademicTerm::new(2026, gnnuhub_core::model::Term::First);
    match session.class_schedule(term, None).await {
        Ok(schedule) => {
            println!("    ✅ 返回成功");
            println!("    课程数: {}", schedule.course_count());
            for (name, entries) in schedule.courses.iter().take(5) {
                for e in entries {
                    println!(
                        "      - {} @ {} {} 第{}-{}节 周次={}",
                        name,
                        e.position,
                        e.time.weekday,
                        e.time.periods.start,
                        e.time.periods.end,
                        e.time.weeks
                    );
                }
            }
            if schedule.courses.is_empty() {
                println!("    ⚠️ 课程列表为空——可能是 csrftoken 被校验，或该学期无课");
            }
        }
        Err(e) => println!("    ❌ 失败: {e}"),
    }
    println!();

    // ---------- 2. 学籍详细信息 ----------
    println!("[2] 学籍接口 /xsxxxggl/xsgrxxwh_cxXsgrxx.html");
    match session.fetch_student_info().await {
        Ok(info) => {
            println!("    ✅ 返回成功");
            println!("      姓名:     {:?}", info.name);
            println!("      性别:     {:?}", info.gender);
            println!(
                "      证件类型: {:?}",
                info.document.as_ref().map(|d| &d.kind)
            );
            println!(
                "      证件号码: {:?}",
                info.document.as_ref().map(|d| &d.number)
            );
            println!("      出生日期: {:?}", info.birthday);
            println!("      民族:     {:?}", info.ethnicity);
            println!("      政治面貌: {:?}", info.political_status);
            println!("      学院:     {:?}", info.college);
            println!("      专业:     {:?}", info.major);
            println!("      班级:     {:?}", info.class_name);
            println!("      通讯地址: {:?}", info.address);
            println!("      招生年度: {:?}", info.enrollment_year);

            let filled = [
                info.name.is_some(),
                info.gender.is_some(),
                info.document.is_some(),
                info.birthday.is_some(),
                info.ethnicity.is_some(),
                info.political_status.is_some(),
                info.college.is_some(),
                info.major.is_some(),
                info.class_name.is_some(),
                info.address.is_some(),
                info.enrollment_year.is_some(),
            ]
            .iter()
            .filter(|x| **x)
            .count();
            println!("      → 12 个映射字段中解析出 {filled} 个");
        }
        Err(e) => println!("    ❌ 失败: {e}"),
    }

    // ---------- 3. 原始 HTML 存档（供离线核对字段解析）----------
    println!("\n[3] 存档原始 HTML 供离线分析");
    let out = PathBuf::from("tools/js-recon/out/pages");
    let _ = std::fs::create_dir_all(&out);
    match session
        .fetch_raw("/xsxxxggl/xsgrxxwh_cxXsgrxx.html?gnmkdm=N100801&layout=default")
        .await
    {
        Ok(html) => {
            let f = out.join("xsgrxx_filled.html");
            std::fs::write(&f, &html)?;
            println!("    已保存 {} 字节到 {}", html.len(), f.display());
        }
        Err(e) => println!("    失败: {e}"),
    }

    println!("\n完成。");
    Ok(())
}

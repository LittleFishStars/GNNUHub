//! 用真实抓取的学籍页 HTML 验证字段解析
//!
//! 夹具取自真实响应（已脱敏），只保留 12 个被映射字段的容器。
//! 用于回归保护：一旦页面结构或字段 id 变化，这里会先报警。
//!
//! 夹具来源与生成方式见 `tools/js-recon/README.md`。

use std::path::Path;

/// 读取夹具文件
///
/// `cargo test` 的工作目录是 crate 根目录，因此路径相对本 crate。
fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let data = std::fs::read(&p).unwrap_or_else(|e| panic!("读取夹具 {} 失败: {e}", p.display()));
    String::from_utf8(data).expect("夹具应为合法 UTF-8")
}

/// 12 个字段应全部解析成功
#[test]
fn parses_all_mapped_student_fields() {
    let html = fixture("xsgrxx.html");
    let info = gnnuhub_api::parse::parse_student_info(&html).unwrap();

    assert_eq!(info.name.as_deref(), Some("张三"), "姓名");
    assert_eq!(info.gender.as_deref(), Some("男"), "性别");
    assert_eq!(info.birthday.as_deref(), Some("2000-01-01"), "出生日期");
    assert_eq!(info.ethnicity.as_deref(), Some("汉族"), "民族");
    assert_eq!(
        info.political_status.as_deref(),
        Some("中国共产主义青年团团员"),
        "政治面貌"
    );
    assert_eq!(
        info.college.as_deref(),
        Some("数学与计算机科学学院"),
        "学院"
    );
    assert_eq!(
        info.class_name.as_deref(),
        Some("数学与应用数学(非师范)2502"),
        "班级"
    );
    assert_eq!(info.enrollment_year.as_deref(), Some("2025"), "招生年度");

    let doc = info.document.as_ref().expect("证件信息");
    assert_eq!(doc.kind, "居民身份证");
    assert_eq!(doc.number, "360101200001011234");
}

/// 专业名应去掉末尾 6 字符的代码后缀
///
/// 真实值形如 `数学与应用数学(非师范)(0710)`，
/// 去掉 `(0710)` 后得到干净的 `数学与应用数学(非师范)`。
#[test]
fn strips_major_code_suffix() {
    let html = fixture("xsgrxx.html");
    let info = gnnuhub_api::parse::parse_student_info(&html).unwrap();
    assert_eq!(
        info.major.as_deref(),
        Some("数学与应用数学(非师范)"),
        "专业名应剥掉 6 字符代码后缀"
    );
}

/// 政治面貌字段的真实 id 是 `col_zzmmm`（两个 z），
/// 此处显式记录，避免日后误改为 `col_zzzmmm`。
#[test]
fn political_status_field_id_is_zzmmm() {
    let html = fixture("xsgrxx.html");
    assert!(
        html.contains(r#"id="col_zzmmm""#),
        "夹具应包含 col_zzmmm（政治面貌）"
    );
    assert!(
        !html.contains(r#"id="col_zzzmmm""#),
        "col_zzzmmm 不存在，勿加第三个 z"
    );

    let info = gnnuhub_api::parse::parse_student_info(&html).unwrap();
    assert!(info.political_status.is_some(), "政治面貌应解析成功");
}

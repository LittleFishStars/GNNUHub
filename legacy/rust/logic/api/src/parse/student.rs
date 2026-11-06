//! 学籍与个人信息解析
//!
//! 三个数据源各自提供 [`StudentInfo`] 的一部分字段，靠
//! [`StudentInfo::merge_from`] 逐次拼齐：
//!
//! - 首页基本资料页（HTML，[`parse_basic_info`]）
//! - 学籍信息页（HTML，[`parse_student_info`]）
//! - 学籍 JSON 接口（[`parse_student_profile_json`]，字段最全）

use gnnuhub_core::model::{Document, StudentInfo};
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

/// 从个人信息页抽取基本资料
///
/// 页面结构（Python 版依赖的选择器）：
///
/// - `//h4/text()` → `"姓名 身份"`
/// - `//p/text()` → `"学院 班级"`
/// - `//img/@src` → 头像相对路径
///
/// 页面结构可能随教务系统升级而变化，此处对每个字段都做了容错，
/// 缺失的字段返回 `None` 而不是整体失败。
pub fn parse_basic_info(html: &str) -> Result<StudentInfo> {
    let doc = scraper::Html::parse_document(html);
    let mut info = StudentInfo::default();

    let h4_selector = selector("h4")?;
    if let Some(h4) = doc.select(&h4_selector).next() {
        let text = h4.text().collect::<String>();
        // 原实现用两个不换行空格分隔，这里兼容普通空格
        let parts: Vec<&str> = text
            .split(['\u{a0}', ' '])
            .filter(|s| !s.trim().is_empty())
            .collect();
        if let Some(name) = parts.first() {
            info.name = Some(name.trim().to_string());
        }
        if let Some(identity) = parts.get(1) {
            info.identity = Some(identity.trim().to_string());
        }
    }

    let p_selector = selector("p")?;
    if let Some(p) = doc.select(&p_selector).next() {
        let text = p.text().collect::<String>();
        let parts: Vec<&str> = text.split_whitespace().collect();
        if let Some(college) = parts.first() {
            info.college = Some(college.to_string());
        }
        if let Some(class) = parts.get(1) {
            info.class_name = Some(class.to_string());
        }
    }

    let img_selector = selector("img")?;
    if let Some(img) = doc.select(&img_selector).next()
        && let Some(src) = img.value().attr("src")
    {
        info.avatar = Some(format!("{}{}", gnnuhub_core::JWGL_BASE_URL, src));
    }

    Ok(info)
}

/// 学籍信息页的字段选择器
///
/// 每个元素是 `(选择器 id, 写入 StudentInfo 的方式)`。
/// 独立成表方便后续接口变更时集中维护。
const STUDENT_INFO_FIELDS: &[(&str, StudentField)] = &[
    ("col_xm", StudentField::Name),
    ("col_xbm", StudentField::Gender),
    ("col_zjlxm", StudentField::DocumentKind),
    ("col_zjhm", StudentField::DocumentNumber),
    ("col_csrq", StudentField::Birthday),
    ("col_mzm", StudentField::Ethnicity),
    ("col_zzmmm", StudentField::PoliticalStatus),
    ("col_jg_id", StudentField::College),
    ("col_zyh_id", StudentField::Major),
    ("col_bh_id", StudentField::ClassName),
    ("col_txdz", StudentField::Address),
    ("col_zsnddm", StudentField::EnrollmentYear),
];

/// 学籍信息字段
#[derive(Debug, Clone, Copy)]
enum StudentField {
    Name,
    Gender,
    DocumentKind,
    DocumentNumber,
    Birthday,
    Ethnicity,
    PoliticalStatus,
    College,
    Major,
    ClassName,
    Address,
    EnrollmentYear,
}

/// 从学籍信息页抽取详细资料
///
/// 对应 Python 版 `_get_student_info`。原实现有几个字段做了字符串
/// 截取（专业去掉末尾 6 字符、入学年份取前 4 位），这里保留同样
/// 的处理以保证行为一致。
pub fn parse_student_info(html: &str) -> Result<StudentInfo> {
    let doc = scraper::Html::parse_document(html);
    let mut info = StudentInfo::default();

    for (id, field) in STUDENT_INFO_FIELDS {
        let sel = selector(&format!("#{id} p"))?;
        let Some(node) = doc.select(&sel).next() else {
            continue;
        };
        let text = node.text().collect::<String>().trim().to_string();
        if text.is_empty() {
            continue;
        }

        match field {
            StudentField::Name => info.name = Some(text),
            StudentField::Gender => info.gender = Some(text),
            StudentField::DocumentKind => {
                let doc_info = info.document.get_or_insert_with(|| Document {
                    kind: String::new(),
                    number: String::new(),
                });
                doc_info.kind = text;
            }
            StudentField::DocumentNumber => {
                let doc_info = info.document.get_or_insert_with(|| Document {
                    kind: String::new(),
                    number: String::new(),
                });
                doc_info.number = text;
            }
            StudentField::Birthday => info.birthday = Some(text),
            StudentField::Ethnicity => info.ethnicity = Some(text),
            StudentField::PoliticalStatus => info.political_status = Some(text),
            StudentField::College => info.college = Some(text),
            StudentField::Major => {
                // 原实现去掉末尾 6 个字符（通常是专业代码后缀，如
                // "计算机科学与技术080901" -> "计算机科学与技术"）。
                // 这里按字符截取，避免多字节字符被切断。
                let chars: Vec<char> = text.chars().collect();
                let trimmed = if chars.len() > 6 {
                    chars[..chars.len() - 6].iter().collect::<String>()
                } else {
                    text.clone()
                };
                info.major = Some(trimmed);
            }
            StudentField::ClassName => info.class_name = Some(text),
            StudentField::Address => info.address = Some(text),
            StudentField::EnrollmentYear => {
                // 原实现取前 4 位作为入学年份
                info.enrollment_year = Some(text.chars().take(4).collect());
            }
        }
    }

    Ok(info)
}

/// 学籍信息 JSON 接口（`xsxxwh_cxCkDgxsxx.html`）的记录字段
///
/// 键名规律 = 学籍页 HTML 字段 id 去掉 `col_` 前缀（2026-09 实测，
/// 响应共 64 个顶层键）。与展示无关的信封键（`queryModel` /
/// `userModel` / `date*` / `pageTotal` 等）不在结构体里，由 serde
/// 忽略未知字段的默认行为处理。
#[derive(Debug, Default, Deserialize)]
struct RawStudentProfile {
    /// 学号
    #[serde(default)]
    xh: String,
    /// 姓名
    #[serde(default)]
    xm: String,
    /// 性别（接口直接返回文本，例如「男」）
    #[serde(default)]
    xbm: String,
    /// 学院
    #[serde(default, rename = "jg_id")]
    college: String,
    /// 专业（干净名称，无需剥代码后缀）
    #[serde(default)]
    zyh_id: String,
    /// 班级
    #[serde(default)]
    bh_id: String,
    /// 入学年份（例如 `"2025"`）
    #[serde(default)]
    zsnddm: String,
    /// 出生日期
    #[serde(default)]
    csrq: String,
    /// 民族
    #[serde(default)]
    mzm: String,
    /// 政治面貌
    #[serde(default)]
    zzmmm: String,
    /// 联系地址
    #[serde(default)]
    txdz: String,
    /// 辅导员
    #[serde(default)]
    fdyjgh: String,
    /// 证件类型
    #[serde(default)]
    zjlxm: String,
    /// 证件号码
    #[serde(default)]
    zjhm: String,
    /// 培养层次（本科 / 研究生）
    #[serde(default)]
    pyccdm: String,
}

/// 空白文本一律视为「未提供」
fn non_empty(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 从学籍 JSON 接口响应解析详细资料
///
/// 输入是 `xsxxwh_cxCkDgxsxx.html`（模块码 `N100801`）的响应。
/// 权限过滤器的规则（2026-09 实验证实）：从请求查询串或 Referer
/// 读取 `gnmkdm`，两者皆无则返回「无功能权限」错误页——程序化请求
/// 显式在查询串带上 `gnmkdm=N100801` 即可通过。最小表单为
/// `{xh_id, fromXh_id: ""}`：`xh_id_code`（32 位码）与 `xnm`/`xqm`
/// 全部可省，无需先抓 HTML 页面换取码。
///
/// 相比学籍 HTML 页（[`parse_student_info`]）：
///
/// - **字段更多**：`fdyjgh`（辅导员）、`pyccdm`（培养层次）、
///   `xjztdm`（学籍状态）、`xz`（学制）、`rxzf`（入学总分）等
///   HTML 上没有的键；
/// - **专业名干净**：`zyh_id` 直接是「数学与应用数学(非师范)」，
///   无需像 HTML 那样剥 6 字符代码后缀。
pub fn parse_student_profile_json(body: &str) -> Result<StudentInfo> {
    let raw: RawStudentProfile = serde_json::from_str(body).map_err(Error::Json)?;

    let mut info = StudentInfo {
        student_id: non_empty(&raw.xh),
        name: non_empty(&raw.xm),
        gender: non_empty(&raw.xbm),
        college: non_empty(&raw.college),
        major: non_empty(&raw.zyh_id),
        class_name: non_empty(&raw.bh_id),
        enrollment_year: non_empty(&raw.zsnddm),
        birthday: non_empty(&raw.csrq),
        ethnicity: non_empty(&raw.mzm),
        political_status: non_empty(&raw.zzmmm),
        address: non_empty(&raw.txdz),
        instructor: non_empty(&raw.fdyjgh),
        identity: non_empty(&raw.pyccdm),
        ..Default::default()
    };

    let kind = non_empty(&raw.zjlxm);
    let number = non_empty(&raw.zjhm);
    if kind.is_some() || number.is_some() {
        info.document = Some(Document {
            kind: kind.unwrap_or_default(),
            number: number.unwrap_or_default(),
        });
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 基本信息解析：h4 分行 + 头像补全域名
    #[test]
    fn parses_basic_info() {
        let html = r#"
            <html><body>
              <img src="/img/avatar.png">
              <h4>张三&nbsp;&nbsp;本科生</h4>
              <p>信息学院 计算机2201班</p>
            </body></html>
        "#;
        let info = parse_basic_info(html).unwrap();
        assert_eq!(info.name.as_deref(), Some("张三"));
        assert_eq!(info.identity.as_deref(), Some("本科生"));
        assert_eq!(info.college.as_deref(), Some("信息学院"));
        assert_eq!(info.class_name.as_deref(), Some("计算机2201班"));
        assert_eq!(
            info.avatar.as_deref(),
            Some("https://jwgl.gnnu.edu.cn/img/avatar.png")
        );
    }

    /// 页面结构缺失时应返回空结果而不是报错
    #[test]
    fn tolerates_missing_basic_info() {
        let info = parse_basic_info("<html><body></body></html>").unwrap();
        assert!(info.name.is_none());
        assert!(info.avatar.is_none());
    }

    /// 学籍信息应正确抽取并处理专业后缀
    #[test]
    fn parses_student_info() {
        let html = r#"
            <html><body>
              <div id="col_xm"><p>张三</p></div>
              <div id="col_xbm"><p>男</p></div>
              <div id="col_zjlxm"><p>居民身份证</p></div>
              <div id="col_zjhm"><p>360100200001011234</p></div>
              <div id="col_csrq"><p>2000-01-01</p></div>
              <div id="col_mzm"><p>汉族</p></div>
              <div id="col_zzmmm"><p>共青团员</p></div>
              <div id="col_jg_id"><p>信息与通信学院</p></div>
              <div id="col_zyh_id"><p>计算机科学与技术080901</p></div>
              <div id="col_bh_id"><p>计算机2201班</p></div>
              <div id="col_txdz"><p>江西省赣州市</p></div>
              <div id="col_zsnddm"><p>2022-09-01</p></div>
            </body></html>
        "#;
        let info = parse_student_info(html).unwrap();
        assert_eq!(info.name.as_deref(), Some("张三"));
        assert_eq!(info.gender.as_deref(), Some("男"));
        assert_eq!(info.birthday.as_deref(), Some("2000-01-01"));
        assert_eq!(info.ethnicity.as_deref(), Some("汉族"));
        assert_eq!(info.political_status.as_deref(), Some("共青团员"));
        assert_eq!(info.college.as_deref(), Some("信息与通信学院"));
        assert_eq!(info.class_name.as_deref(), Some("计算机2201班"));
        assert_eq!(info.address.as_deref(), Some("江西省赣州市"));
        assert_eq!(info.enrollment_year.as_deref(), Some("2022"));
        let doc = info.document.unwrap();
        assert_eq!(doc.kind, "居民身份证");
        assert_eq!(doc.number, "360100200001011234");
    }

    /// 学籍 JSON 接口解析（脱敏样例，键集对齐 2026-09 实测响应）
    #[test]
    fn parses_student_profile_json() {
        let body = r#"{
            "xh_id": "250700001", "xh": "250700001",
            "xm": "李四", "xmpy": "Li Si", "xbm": "女",
            "jg_id": "数学与计算机科学学院",
            "zyh_id": "数学与应用数学(非师范)",
            "bh_id": "数学与应用数学(非师范)2501",
            "zsnddm": "2025", "csrq": "2007-01-02",
            "mzm": "汉族", "zzmmm": "中国共产主义青年团团员",
            "txdz": "江西省某市某区某街道1号",
            "fdyjgh": "王辅导", "zjlxm": "居民身份证",
            "zjhm": "360100200701021234", "pyccdm": "本科",
            "xjztdm": "在读", "xz": "4", "rxzf": "514",
            "syd": "江西省", "jg": "江西省", "zskl": "物理类",
            "queryModel": {"currentPage": 1},
            "userModel": {"monitor": false},
            "date": "二○二六年九月十八日",
            "pageTotal": 0
        }"#;
        let info = parse_student_profile_json(body).unwrap();

        assert_eq!(info.student_id.as_deref(), Some("250700001"));
        assert_eq!(info.name.as_deref(), Some("李四"));
        assert_eq!(info.gender.as_deref(), Some("女"));
        assert_eq!(info.college.as_deref(), Some("数学与计算机科学学院"));
        assert_eq!(
            info.major.as_deref(),
            Some("数学与应用数学(非师范)"),
            "JSON 的专业名是干净的，无需剥后缀"
        );
        assert_eq!(
            info.class_name.as_deref(),
            Some("数学与应用数学(非师范)2501")
        );
        assert_eq!(info.enrollment_year.as_deref(), Some("2025"));
        assert_eq!(info.birthday.as_deref(), Some("2007-01-02"));
        assert_eq!(info.ethnicity.as_deref(), Some("汉族"));
        assert_eq!(
            info.political_status.as_deref(),
            Some("中国共产主义青年团团员")
        );
        assert_eq!(info.address.as_deref(), Some("江西省某市某区某街道1号"));
        assert_eq!(info.instructor.as_deref(), Some("王辅导"));
        assert_eq!(info.identity.as_deref(), Some("本科"));
        let doc = info.document.unwrap();
        assert_eq!(doc.kind, "居民身份证");
        assert_eq!(doc.number, "360100200701021234");
    }

    /// 空字符串与缺失字段都应表现为「未提供」
    #[test]
    fn tolerates_blank_fields_in_profile_json() {
        let body = r#"{"xh": "250700001", "xm": "李四", "zyh_id": "", "zjhm": ""}"#;
        let info = parse_student_profile_json(body).unwrap();
        assert_eq!(info.student_id.as_deref(), Some("250700001"));
        assert_eq!(info.name.as_deref(), Some("李四"));
        assert!(info.major.is_none());
        assert!(info.document.is_none(), "证件号缺失时不应造出半份证件");
    }

    /// 专业名末尾的 6 位代码应被去掉
    #[test]
    fn trims_major_code_suffix() {
        let html = r#"<div id="col_zyh_id"><p>计算机科学与技术080901</p></div>"#;
        let info = parse_student_info(html).unwrap();
        assert_eq!(info.major.as_deref(), Some("计算机科学与技术"));
    }

    /// 专业名过短时不应触发越界截取
    #[test]
    fn handles_short_major_name() {
        let html = r#"<div id="col_zyh_id"><p>数学</p></div>"#;
        let info = parse_student_info(html).unwrap();
        assert_eq!(info.major.as_deref(), Some("数学"));
    }
}

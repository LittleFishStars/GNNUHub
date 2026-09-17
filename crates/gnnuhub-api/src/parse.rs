//! 教务系统页面与接口数据的解析
//!
//! 教务系统返回两类数据：
//!
//! - **HTML 页面**：个人信息、教学周等需要从 DOM 中抽取
//! - **JSON 接口**：课表、节次时间等
//!
//! 对应 Python 版 `api.py` 中 `_get_basic_info` / `_get_student_info` /
//! `parse_class_schedule` / `get_timetable` / `this_week` 的解析逻辑。

use std::collections::BTreeMap;

use gnnuhub_core::model::{
    AcademicTerm, ClassSchedule, ClassTime, CourseEntry, Document, PeriodRange, PeriodTime,
    StudentInfo,
};
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

/// 课表接口返回的顶层结构
#[derive(Debug, Deserialize)]
struct RawScheduleResponse {
    /// 学生信息
    #[serde(default)]
    xsxx: Option<RawStudentBrief>,
    /// 课程列表（整学期接口字段名）
    #[serde(default, rename = "kbList")]
    kb_list: Vec<RawCourse>,
    /// 部分接口用 `kbListnew` 字段
    #[serde(default, rename = "kbListnew")]
    kb_list_new: Vec<RawCourse>,
}

/// 课表里的学生简要信息
#[derive(Debug, Default, Deserialize)]
struct RawStudentBrief {
    #[serde(default, rename = "XM")]
    name: String,
    #[serde(default, rename = "ZYMC")]
    major: String,
    #[serde(default, rename = "BJMC")]
    class_name: String,
    #[serde(default, rename = "JSXM")]
    instructor: String,
}

/// 课表里的一门课
#[derive(Debug, Deserialize)]
struct RawCourse {
    /// 课程名称
    #[serde(rename = "kcmc")]
    name: String,
    /// 上课地点
    #[serde(default, rename = "cdmc")]
    position: String,
    /// 教师姓名，多个用 `,` 分隔
    #[serde(default, rename = "xm")]
    teacher: String,
    /// 周次描述，例如 `"1-16周"`
    #[serde(default, rename = "zcd")]
    weeks: String,
    /// 星期，例如 `"星期一"`
    #[serde(default, rename = "xqjmc")]
    weekday: String,
    /// 节次，例如 `"1-2"`
    #[serde(default, rename = "jcs")]
    periods: String,
    /// 教学班组成，用 `;` 分隔
    #[serde(default, rename = "jxbzc")]
    classes: String,
    /// 教学楼
    #[serde(default, rename = "lh")]
    building: String,
    /// 课程性质（必修 / 选修）
    #[serde(default, rename = "kcxz")]
    nature: String,
    /// 课程类别
    #[serde(default, rename = "kclb")]
    category: String,
    /// 考核方式
    #[serde(default, rename = "khfsmc")]
    exam_mode: String,
    /// 上课校区
    #[serde(default, rename = "xqmc")]
    campus: String,
    /// 学分，接口以字符串返回
    #[serde(default, rename = "xf")]
    credit: String,
}

/// 解析课表接口响应
///
/// 返回解析后的课表与学生简要信息（课表接口同时携带姓名、
/// 学院、辅导员等信息，可用来填充 [`StudentInfo`]）。
pub fn parse_class_schedule(body: &str) -> Result<(ClassSchedule, StudentInfo)> {
    let raw: RawScheduleResponse = serde_json::from_str(body).map_err(Error::Json)?;

    let mut info = StudentInfo::default();
    if let Some(brief) = &raw.xsxx {
        if !brief.name.is_empty() {
            info.name = Some(brief.name.clone());
        }
        if !brief.major.is_empty() {
            info.college = Some(brief.major.clone());
        }
        if !brief.class_name.is_empty() {
            info.class_name = Some(brief.class_name.clone());
        }
        if !brief.instructor.is_empty() {
            info.instructor = Some(brief.instructor.clone());
        }
    }

    let mut schedule = ClassSchedule::default();
    let courses = if raw.kb_list.is_empty() {
        raw.kb_list_new
    } else {
        raw.kb_list
    };

    for course in courses {
        let entry = convert_course(&course);
        schedule.courses.entry(course.name).or_default().push(entry);
    }

    Ok((schedule, info))
}

/// 把原始课程转换为领域模型
fn convert_course(raw: &RawCourse) -> CourseEntry {
    let teachers = if raw.teacher.is_empty() {
        Vec::new()
    } else {
        raw.teacher
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let classes = if raw.classes.is_empty() {
        Vec::new()
    } else {
        raw.classes
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    // 节次解析失败时退化为第 0 节，避免整条记录丢失
    let periods = PeriodRange::parse(&raw.periods).unwrap_or(PeriodRange { start: 0, end: 0 });

    CourseEntry {
        position: raw.position.clone(),
        teachers,
        time: ClassTime {
            weeks: raw.weeks.clone(),
            weekday: raw.weekday.clone(),
            periods,
        },
        classes,
        building: raw.building.clone(),
        nature: raw.nature.clone(),
        category: raw.category.clone(),
        exam_mode: raw.exam_mode.clone(),
        campus: raw.campus.clone(),
        credit: raw.credit.parse().unwrap_or(0.0),
    }
}

/// 节次时间表的原始条目
#[derive(Debug, Deserialize)]
struct RawPeriodTime {
    /// 开始时间
    #[serde(default, rename = "qssj")]
    start: String,
    /// 结束时间
    #[serde(default, rename = "jssj")]
    end: String,
}

/// 解析节次时间表
pub fn parse_timetable(body: &str) -> Result<Vec<PeriodTime>> {
    let raw: Vec<RawPeriodTime> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw
        .into_iter()
        .map(|p| PeriodTime {
            start: p.start,
            end: p.end,
        })
        .collect())
}

/// 从教学周页面抽取当前周
///
/// 原实现从 `#zs` 下拉框中已选中的 option 文本取周次，
/// 文本形如 `"5(第5周)"`，因此取 `(` 之前的部分。
pub fn parse_this_week(html: &str) -> Result<u8> {
    let doc = scraper::Html::parse_document(html);

    let selected = selector("#zs option[selected]")?;
    let node = doc
        .select(&selected)
        .next()
        .ok_or_else(|| Error::PageParse("未找到已选中的教学周选项".to_string()))?;

    let text = node.text().collect::<String>();
    let week_part = text.split('(').next().unwrap_or(&text).trim();
    week_part
        .parse()
        .map_err(|_| Error::PageParse(format!("无法解析教学周: {text:?}")))
}

/// 构造一个 CSS 选择器
fn selector(css: &str) -> Result<scraper::Selector> {
    scraper::Selector::parse(css)
        .map_err(|e| Error::PageParse(format!("非法的选择器 {css}: {e:?}")))
}

/// 把学年学期信息写入课表
pub fn attach_academic_term(schedule: &mut ClassSchedule, term: AcademicTerm) {
    schedule.academic_term = Some(term);
}

/// 按周一到周日分组课表条目，便于界面渲染
pub fn group_by_weekday(schedule: &ClassSchedule) -> BTreeMap<String, Vec<(String, CourseEntry)>> {
    let mut grouped: BTreeMap<String, Vec<(String, CourseEntry)>> = BTreeMap::new();
    for (name, entries) in &schedule.courses {
        for entry in entries {
            grouped
                .entry(entry.time.weekday.clone())
                .or_default()
                .push((name.clone(), entry.clone()));
        }
    }
    grouped
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

    /// 课表解析
    #[test]
    fn parses_class_schedule() {        let body = r#"{
            "xsxx": {"XM":"张三","ZYMC":"信息学院","BJMC":"计算机2201班","JSXM":"李老师"},
            "kbList": [
              {
                "kcmc":"高等数学","cdmc":"十教201","xm":"王老师,刘老师",
                "zcd":"1-16周","xqjmc":"星期一","jcs":"1-2",
                "jxbzc":"计算机2201班;计算机2202班","lh":"十教",
                "kcxz":"必修","kclb":"公共基础课","khfsmc":"考试",
                "xqmc":"黄金校区","xf":"4.0"
              }
            ]
        }"#;
        let (schedule, info) = parse_class_schedule(body).unwrap();
        assert_eq!(info.name.as_deref(), Some("张三"));
        assert_eq!(info.instructor.as_deref(), Some("李老师"));
        assert_eq!(schedule.course_count(), 1);

        let entries = schedule.course("高等数学");
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.position, "十教201");
        assert_eq!(e.teachers, vec!["王老师", "刘老师"]);
        assert_eq!(e.classes, vec!["计算机2201班", "计算机2202班"]);
        assert_eq!(e.time.periods.start, 1);
        assert_eq!(e.time.periods.end, 2);
        assert_eq!(e.credit, 4.0);
    }

    /// 同一课程多次排课应聚合到同一键下
    #[test]
    fn aggregates_multiple_entries_per_course() {
        let body = r#"{
            "kbList": [
              {"kcmc":"大学英语","xqjmc":"星期一","jcs":"1-2"},
              {"kcmc":"大学英语","xqjmc":"星期三","jcs":"3-4"}
            ]
        }"#;
        let (schedule, _) = parse_class_schedule(body).unwrap();
        assert_eq!(schedule.course_count(), 1);
        assert_eq!(schedule.course("大学英语").len(), 2);
    }

    /// 空课表应能正常解析
    #[test]
    fn parses_empty_schedule() {
        let (schedule, _) = parse_class_schedule(r#"{"kbList":[]}"#).unwrap();
        assert_eq!(schedule.course_count(), 0);
    }

    /// 节次字段畸形时不应导致整条记录丢失
    #[test]
    fn tolerates_malformed_periods() {
        let body = r#"{"kbList":[{"kcmc":"体育","jcs":"?","xf":"1"}]}"#;
        let (schedule, _) = parse_class_schedule(body).unwrap();
        let e = &schedule.course("体育")[0];
        assert_eq!(e.time.periods.start, 0);
        assert_eq!(e.credit, 1.0);
    }

    /// 学分字段非法时退化为 0
    #[test]
    fn tolerates_invalid_credit() {
        let body = r#"{"kbList":[{"kcmc":"选修","xf":"未知"}]}"#;
        let (schedule, _) = parse_class_schedule(body).unwrap();
        assert_eq!(schedule.course("选修")[0].credit, 0.0);
    }

    /// 节次时间表解析
    #[test]
    fn parses_timetable() {
        let body = r#"[{"qssj":"08:00","jssj":"08:45"},{"qssj":"08:55","jssj":"09:40"}]"#;
        let times = parse_timetable(body).unwrap();
        assert_eq!(times.len(), 2);
        assert_eq!(times[0].start, "08:00");
        assert_eq!(times[1].end, "09:40");
    }

    /// 教学周解析
    #[test]
    fn parses_this_week() {
        let html = r#"
            <html><body>
              <select id="zs">
                <option value="1">1(第1周)</option>
                <option value="5" selected="selected">5(第5周)</option>
              </select>
            </body></html>
        "#;
        assert_eq!(parse_this_week(html).unwrap(), 5);
    }

    /// 找不到教学周时应报错而不是返回 0
    #[test]
    fn errors_when_week_missing() {
        assert!(parse_this_week("<html><body></body></html>").is_err());
    }

    /// 按星期分组
    #[test]
    fn groups_by_weekday() {
        let body = r#"{
            "kbList": [
              {"kcmc":"数学","xqjmc":"星期一","jcs":"1-2"},
              {"kcmc":"英语","xqjmc":"星期一","jcs":"3-4"},
              {"kcmc":"物理","xqjmc":"星期二","jcs":"1-2"}
            ]
        }"#;
        let (schedule, _) = parse_class_schedule(body).unwrap();
        let grouped = group_by_weekday(&schedule);
        assert_eq!(grouped.get("星期一").unwrap().len(), 2);
        assert_eq!(grouped.get("星期二").unwrap().len(), 1);
    }
}

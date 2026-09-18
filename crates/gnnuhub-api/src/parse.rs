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
    AcademicTerm, ClassSchedule, ClassTime, CourseEntry, Document, GradeRecord, PeriodRange,
    PeriodTime, StudentInfo, parse_credit,
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
        let entry = CourseEntry::from(&course);
        schedule.courses.entry(course.name).or_default().push(entry);
    }

    Ok((schedule, info))
}

/// 把原始课程转换为领域模型
///
/// 字段逐一对齐接口返回；缺失或畸形的值退化处理而非报错，
/// 保证单条记录异常不会毁掉整张课表。
impl From<&RawCourse> for CourseEntry {
    fn from(raw: &RawCourse) -> Self {
        Self {
            position: raw.position.clone(),
            teachers: Self::split_list(&raw.teacher, ','),
            time: ClassTime {
                weeks: raw.weeks.clone(),
                weekday: raw.weekday.clone(),
                periods: PeriodRange::parse(&raw.periods).unwrap_or_default(),
            },
            classes: Self::split_list(&raw.classes, ';'),
            building: raw.building.clone(),
            nature: raw.nature.clone(),
            category: raw.category.clone(),
            exam_mode: raw.exam_mode.clone(),
            campus: raw.campus.clone(),
            credit: parse_credit(&raw.credit),
        }
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

/// 成绩查询接口返回的顶层结构
///
/// 该接口是 jqGrid 的远程数据源，外层是分页信封，真正的数据在 `items`。
/// 完整骨架（实测原文）：
///
/// ```json
/// {"currentPage":1,"currentResult":0,"entityOrField":false,"items":[],
///  "limit":15,"offset":0,"pageNo":0,"pageSize":15,"showCount":10,
///  "sortName":"xnmmc asc,xqmmc asc,kch asc","sortOrder":" ","sorts":[],
///  "totalCount":0,"totalPage":0,"totalResult":0}
/// ```
///
/// 注意 `items` **不是**数组之外的候选字段——`idx` / `dataList` 等名字
/// 在别的模块出现过，但成绩接口只认 `items`。这里仍保留 `#[serde(default)]`
/// 让空响应能解析成空列表而不是报错。
#[derive(Debug, Default, Deserialize)]
struct RawGradeResponse {
    /// 成绩条目
    #[serde(default)]
    items: Vec<RawGrade>,
}

/// 成绩查询接口里的一条记录
///
/// 字段名取自前端 `colModel` 的 `name`。除 `kcmc` 外全部可选：
/// 不同课程类型返回的字段集合并不一致（例如考查课没有 `bfzcj`），
/// 缺字段应该留空而不是让整条记录失败。
#[derive(Debug, Default, Deserialize)]
struct RawGrade {
    /// 学年，例如 `"2025-2026"`
    #[serde(default, rename = "xnmmc")]
    academic_year: String,
    /// 学期，例如 `"1"`
    #[serde(default, rename = "xqmmc")]
    semester: String,
    /// 课程代码
    #[serde(default, rename = "kch")]
    course_code: String,
    /// 课程名称
    #[serde(default, rename = "kcmc")]
    course_name: String,
    /// 课程性质
    #[serde(default, rename = "kcxzmc")]
    nature: String,
    /// 学分
    #[serde(default, rename = "xf")]
    credit: String,
    /// 成绩（展示用文本）
    #[serde(default, rename = "cj")]
    score: String,
    /// 成绩备注
    #[serde(default, rename = "cjbz")]
    score_note: String,
    /// 绩点
    #[serde(default, rename = "jd")]
    grade_point: String,
    /// 成绩性质
    #[serde(default, rename = "ksxz")]
    score_type: String,
    /// 是否成绩作废
    #[serde(default, rename = "cjsfzf")]
    score_voided: String,
    /// 是否学位课程
    #[serde(default, rename = "sfxwkc")]
    is_degree_course: String,
    /// 开课学院
    #[serde(default, rename = "kkbmmc")]
    college: String,
    /// 课程标记
    #[serde(default, rename = "kcbj")]
    course_mark: String,
    /// 课程类别
    #[serde(default, rename = "kclbmc")]
    category: String,
    /// 课程归属
    #[serde(default, rename = "kcgsmc")]
    attribution: String,
    /// 教学班
    #[serde(default, rename = "jxbmc")]
    class_name: String,
    /// 任课教师
    #[serde(default, rename = "jsxm")]
    teacher: String,
    /// 考核方式
    #[serde(default, rename = "khfsmc")]
    exam_mode: String,
    /// 学生标记
    #[serde(default, rename = "xsbjmc")]
    student_mark: String,
    /// 学分绩点
    #[serde(default, rename = "xfjd")]
    credit_grade_point: String,
    /// 百分制原始分；以**数值**返回，缺失时为 `null`
    #[serde(default, rename = "bfzcj")]
    raw_score: Option<f32>,
}

impl From<RawGrade> for GradeRecord {
    fn from(raw: RawGrade) -> Self {
        Self {
            academic_year: raw.academic_year,
            semester: raw.semester,
            course_code: raw.course_code,
            course_name: raw.course_name,
            nature: raw.nature,
            credit: parse_credit(&raw.credit),
            score: raw.score,
            score_note: raw.score_note,
            grade_point: raw.grade_point,
            score_type: raw.score_type,
            score_voided: raw.score_voided,
            is_degree_course: raw.is_degree_course,
            college: raw.college,
            course_mark: raw.course_mark,
            category: raw.category,
            attribution: raw.attribution,
            class_name: raw.class_name,
            teacher: raw.teacher,
            exam_mode: raw.exam_mode,
            student_mark: raw.student_mark,
            credit_grade_point: raw.credit_grade_point,
            raw_score: raw.raw_score,
        }
    }
}

/// 解析成绩查询接口响应
///
/// 输入是 jqGrid 分页信封的原文（数据在 `items` 里）。
/// 本函数**不做任何跨记录统计**，只逐条转换。
pub fn parse_grades(body: &str) -> Result<Vec<GradeRecord>> {
    let raw: RawGradeResponse = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(GradeRecord::from).collect())
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
    fn parses_class_schedule() {
        let body = r#"{
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

    /// 成绩接口的空响应（实测原文）应解析成空列表
    ///
    /// 这条骨架来自 2026-2027 学年第一学期的真实返回——该学期刚开始，
    /// 尚无成绩，因此 `items` 为空但信封字段俱全。
    #[test]
    fn parses_empty_grade_response() {
        let body = r#"{"currentPage":1,"currentResult":0,"entityOrField":false,
            "items":[],"limit":15,"offset":0,"pageNo":0,"pageSize":15,"showCount":10,
            "sortName":"xnmmc asc,xqmmc asc,kch asc","sortOrder":" ","sorts":[],
            "totalCount":0,"totalPage":0,"totalResult":0}"#;
        assert!(parse_grades(body).unwrap().is_empty());
    }

    /// 成绩记录应逐字段落到 `GradeRecord`
    #[test]
    fn parses_grade_record() {
        let body = r#"{
            "items": [{
              "xnmmc":"2025-2026","xqmmc":"1","kch":"B1200010",
              "kcmc":"数学分析Ⅰ","kcxzmc":"必修","xf":"5.0",
              "cj":"87","cjbz":"","jd":"3.7","ksxz":"正常考试",
              "cjsfzf":"否","sfxwkc":"是","kkbmmc":"数学与计算机科学学院",
              "kcbj":"主修","kclbmc":"学科基础课","kcgsmc":"数学系",
              "jxbmc":"数学与应用数学2502","jsxm":"李老师",
              "khfsmc":"考试","xsbjmc":"","xfjd":"18.5",
              "bfzcj":87
            }]
        }"#;
        let records = parse_grades(body).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.academic_year, "2025-2026");
        assert_eq!(r.course_code, "B1200010");
        assert_eq!(r.course_name, "数学分析Ⅰ");
        assert_eq!(r.nature, "必修");
        assert_eq!(r.credit, 5.0);
        assert_eq!(r.score, "87");
        assert_eq!(r.grade_point, "3.7");
        assert_eq!(r.score_type, "正常考试");
        assert_eq!(r.is_degree_course, "是");
        assert_eq!(r.college, "数学与计算机科学学院");
        assert_eq!(r.course_mark, "主修");
        assert_eq!(r.category, "学科基础课");
        assert_eq!(r.class_name, "数学与应用数学2502");
        assert_eq!(r.teacher, "李老师");
        assert_eq!(r.exam_mode, "考试");
        assert_eq!(r.credit_grade_point, "18.5");
        assert_eq!(r.raw_score, Some(87.0));
    }

    /// `bfzcj` 缺失（考查课）时不应让整条记录失败
    #[test]
    fn tolerates_missing_raw_score() {
        let body = r#"{"items":[{"kcmc":"形势与政策","cj":"优秀","xf":"1"}]}"#;
        let records = parse_grades(body).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].raw_score, None);
        assert_eq!(records[0].score, "优秀");
    }

    /// 非数值成绩不应导致解析失败
    #[test]
    fn tolerates_non_numeric_score() {
        let body = r#"{"items":[{"kcmc":"体育","cj":"合格","xf":"未知","jd":""}]}"#;
        let records = parse_grades(body).unwrap();
        assert_eq!(records[0].credit, 0.0);
        assert_eq!(records[0].score, "合格");
    }

    /// 通过判定：有原始分时按 60 分线
    #[test]
    fn pass_judgement_uses_raw_score() {
        let mk = |raw: Option<f32>, text: &str| gnnuhub_core::model::GradeRecord {
            score: text.to_string(),
            raw_score: raw,
            ..Default::default()
        };

        // 补考后 `cj` 可能已折算成 60 以上，但 `bfzcj` 仍是原始分
        assert!(mk(Some(87.0), "87").passed());
        assert!(mk(Some(60.0), "60").passed(), "60 分应算通过");
        assert!(!mk(Some(59.0), "59").passed());
        // 高挂重修：cj 显示及格但原始分不及格——必须判未通过
        assert!(!mk(Some(45.0), "60").passed(), "原始分不及格就不算通过");
    }

    /// 通过判定：无原始分时走文本白名单，且否定词优先
    #[test]
    fn pass_judgement_falls_back_to_text() {
        let mk = |text: &str| gnnuhub_core::model::GradeRecord {
            score: text.to_string(),
            ..Default::default()
        };

        for ok in ["优秀", "良好", "中等", "合格", "及格", "通过"] {
            assert!(mk(ok).passed(), "{ok} 应判为通过");
        }
        for bad in ["不合格", "不通过", "未通过", "缺考", "作弊", "0", ""] {
            assert!(!mk(bad).passed(), "{bad:?} 应判为未通过");
        }
    }

    /// 一条响应里可以混有通过与未通过的记录，逐条判定互不影响
    #[test]
    fn mixed_records_judge_independently() {
        let body = r#"{
            "items": [
              {"kcmc":"A","xf":"5","cj":"87","bfzcj":87},
              {"kcmc":"B","xf":"3","cj":"55","bfzcj":55},
              {"kcmc":"C","xf":"2","cj":"合格"}
            ]
        }"#;
        let records = parse_grades(body).unwrap();
        assert_eq!(records.len(), 3);

        let passed: Vec<&str> = records
            .iter()
            .filter(|r| r.passed())
            .map(|r| r.course_name.as_str())
            .collect();
        assert_eq!(
            passed,
            vec!["A", "C"],
            "A 按原始分通过，C 按文本通过，B 未通过"
        );

        // 通过记录的学分合计
        let earned: f32 = records
            .iter()
            .filter(|r| r.passed())
            .map(|r| r.credit)
            .sum();
        assert_eq!(earned, 7.0);
    }
}

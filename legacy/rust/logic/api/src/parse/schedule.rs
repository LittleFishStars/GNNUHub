//! 课表、节次时间与当前教学周解析

use gnnuhub_core::model::{
    ClassSchedule, ClassTime, CourseEntry, PeriodRange, PeriodTime, StudentInfo, parse_credit,
};
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::selector;

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
///
/// # `null` 响应是合法的
///
/// 2026-09-18 实测：服务端在「无课表数据」时会返回字面量 `null`
/// （前端 `cxKbContent` 对 `data==null` 有显式处理，浏览器同样显示
/// 空课表）。本函数把 `null` 解析为**空课表**而不是报错。
/// （历史注记：2026-09-18 下午起该接口曾对所有学期恒定返回
/// `null`，当时 A-J 八组对照实验误判为学校端服务故障，真因是
/// `Session::send` 写死 GET 方法——课表接口只认 POST。见
/// `probe_kb_null.rs` 的实验记录与 `session.rs` 的方法说明。）
pub fn parse_class_schedule(body: &str) -> Result<(ClassSchedule, StudentInfo)> {
    // 字面量 null → 空课表（serde 对 Option 是唯一能直接吃 null 的口）
    let raw: Option<RawScheduleResponse> = serde_json::from_str(body).map_err(Error::Json)?;
    let Some(raw) = raw else {
        tracing::warn!("课表接口返回 null（无课表数据或学校端服务异常），按空课表处理");
        return Ok((ClassSchedule::default(), StudentInfo::default()));
    };

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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 字面量 `null` 是服务端「无课表数据」的合法响应（2026-09-18
    /// 实测，浏览器端同样显示空课表），应解析为空课表而不是报错
    #[test]
    fn parses_null_schedule_as_empty() {
        let (schedule, info) = parse_class_schedule("null").unwrap();
        assert_eq!(schedule.course_count(), 0);
        assert!(info.name.is_none());
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
}

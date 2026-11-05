//! 考试安排解析

use gnnuhub_core::model::{ExamRecord, parse_credit};
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::JqGridEnvelope;

/// 考试查询接口里的一条记录
///
/// 字段名取自前端 `cxXsksxxIndex.js` 学生分支的 `getGridColModel()`
/// （学校码 `10418` 走通用分支）。全部可选：不同考试批次返回的字段
/// 集合并不一致，缺字段应留空而不是让整条记录失败。
#[derive(Debug, Default, Deserialize)]
struct RawExam {
    /// 课程代码
    #[serde(default, rename = "kch")]
    course_code: String,
    /// 课程名称
    #[serde(default, rename = "kcmc")]
    course_name: String,
    /// 考试批次名称
    #[serde(default, rename = "ksmc")]
    exam_name: String,
    /// 考试时间（原文，如 `"2026-07-07(14:30-16:30)"`）
    #[serde(default, rename = "kssj")]
    exam_time: String,
    /// 考试地点
    #[serde(default, rename = "cdmc")]
    location: String,
    /// 考试地点校区
    #[serde(default, rename = "cdxqmc")]
    campus: String,
    /// 考试方式
    #[serde(default, rename = "ksfs")]
    exam_mode: String,
    /// 考核方式
    #[serde(default, rename = "khfs")]
    assessment: String,
    /// 开课学院
    #[serde(default, rename = "kkxy")]
    college: String,
    /// 任课教师（`工号/姓名`）
    #[serde(default, rename = "jsxx")]
    teacher: String,
    /// 是否补考
    #[serde(default, rename = "cxbj")]
    make_up: String,
    /// 教学班
    #[serde(default, rename = "jxbmc")]
    class_name: String,
    /// 学分
    #[serde(default, rename = "xf")]
    credit: String,
    /// 学年
    #[serde(default, rename = "xnmc")]
    academic_year: String,
    /// 学期
    #[serde(default, rename = "xqmmc")]
    semester: String,
    /// 上课时间原文
    #[serde(default, rename = "sksj")]
    class_time: String,
    /// 考试时长（分钟，字符串数值）
    #[serde(default, rename = "sjsj")]
    duration: String,
}

impl From<RawExam> for ExamRecord {
    fn from(raw: RawExam) -> Self {
        Self {
            course_code: raw.course_code,
            course_name: raw.course_name,
            exam_name: raw.exam_name,
            exam_time: raw.exam_time,
            location: raw.location,
            campus: raw.campus,
            exam_mode: raw.exam_mode,
            assessment: raw.assessment,
            college: raw.college,
            teacher: raw.teacher,
            make_up: raw.make_up,
            class_name: raw.class_name,
            credit: parse_credit(&raw.credit),
            academic_year: raw.academic_year,
            semester: raw.semester,
            class_time: raw.class_time,
            duration_minutes: raw.duration.trim().parse().ok(),
        }
    }
}

/// 解析考试安排接口响应
///
/// 输入是 jqGrid 分页信封的原文（数据在 `items` 里）。实测服务端
/// 不按 `xnm`/`xqm` 过滤、一次返回学生全部考试记录，因此**不需要**
/// 分页遍历（`showCount` 给 100 足够容纳正常学生的考试数）。
pub fn parse_exams(body: &str) -> Result<Vec<ExamRecord>> {
    let raw: JqGridEnvelope<RawExam> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(ExamRecord::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 考试安排解析（脱敏夹具，键集对齐 2026-09 实测响应）
    #[test]
    fn parses_exam_records() {
        let body = r#"{
            "currentPage":1,"currentResult":0,"entityOrField":false,
            "items":[
              {"xh_id":"250700001","ksfs":"笔试","bj":"某专业2502班",
               "cdbh":"2309","ksmc":"2025-2026-2学期期末考试（统一）",
               "kssj":"2026-07-07(14:30-16:30)","kch":"1381035",
               "kkxy":"马克思主义学院","cxbj":"否","xqm":"12",
               "khfs":"考试","cdmc":"2-309","cdxqmc":"某校区",
               "sksj":"星期一第7-8节{1-3周,5-17周}","sjsj":"120",
               "kcmc":"某课程","pycc":"本科","njmc":"2025",
               "jgmc":"某学院","jxbmc":"某课程-0049",
               "jxbzc":"某专业2501;某专业2502","xf":"2.5",
               "xnmc":"2025-2026","xh":"250700001",
               "jsxx":"1300028/某老师","zwh":"15"},
              {"kcmc":"某课程Ⅱ","xf":"6.5","sjsj":"未知",
               "kssj":"2026-07-09(09:00-11:00)","cxbj":"是"}
            ],
            "totalCount":2,"totalPage":0,"totalResult":0
        }"#;
        let exams = parse_exams(body).unwrap();
        assert_eq!(exams.len(), 2);

        let first = &exams[0];
        assert_eq!(first.course_name, "某课程");
        assert_eq!(first.exam_name, "2025-2026-2学期期末考试（统一）");
        assert_eq!(first.exam_time, "2026-07-07(14:30-16:30)");
        assert_eq!(first.location, "2-309");
        assert_eq!(first.campus, "某校区");
        assert_eq!(first.exam_mode, "笔试");
        assert_eq!(first.assessment, "考试");
        assert_eq!(first.college, "马克思主义学院");
        assert_eq!(first.teacher, "1300028/某老师");
        assert_eq!(first.class_name, "某课程-0049");
        assert_eq!(first.credit, 2.5);
        assert_eq!(first.academic_year, "2025-2026");
        assert_eq!(first.class_time, "星期一第7-8节{1-3周,5-17周}");
        assert_eq!(first.duration_minutes, Some(120));
        assert!(!first.is_make_up());

        // 缺字段记录应留空而不是失败；非法时长退化为 None
        let second = &exams[1];
        assert_eq!(second.course_name, "某课程Ⅱ");
        assert_eq!(second.credit, 6.5);
        assert!(second.location.is_empty());
        assert_eq!(second.duration_minutes, None, "非法时长应退化为 None");
        assert!(second.is_make_up(), "补考标记应可判定");
    }

    /// 空考试信封应解析成空列表
    #[test]
    fn parses_empty_exam_response() {
        let body = r#"{"currentPage":1,"items":[],"totalCount":0}"#;
        assert!(parse_exams(body).unwrap().is_empty());
    }
}

//! 成绩查询解析

use gnnuhub_core::model::{GradeRecord, parse_credit};
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::JqGridEnvelope;

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
/// 输入是 jqGrid 分页信封的原文（数据在 `items` 里，信封骨架见
/// [`super::JqGridEnvelope`]）。本函数**不做任何跨记录统计**，
/// 只逐条转换。
pub fn parse_grades(body: &str) -> Result<Vec<GradeRecord>> {
    let raw: JqGridEnvelope<RawGrade> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(GradeRecord::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mk = |raw: Option<f32>, text: &str| GradeRecord {
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
        let mk = |text: &str| GradeRecord {
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

//! 重修课程库解析

use gnnuhub_core::model::{RetakeCourse, parse_credit};
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::JqGridEnvelope;

/// 重修课程查询接口里的一条记录
///
/// 字段名取自前端 `cxCxkccxIndex.js` 的 `colModel`。除课程三要素
/// 外全部可选，缺字段留空而不是让整条记录失败。
#[derive(Debug, Default, Deserialize)]
struct RawRetakeCourse {
    /// 课程代码
    #[serde(default, rename = "kch")]
    course_code: String,
    /// 课程名称
    #[serde(default, rename = "kcmc")]
    course_name: String,
    /// 学分
    #[serde(default, rename = "xf")]
    credit: String,
    /// 开课学院
    #[serde(default, rename = "kkbmmc")]
    college: String,
    /// 课程类别
    #[serde(default, rename = "kclbmc")]
    category: String,
    /// 重修状态
    #[serde(default, rename = "cxzt")]
    retake_status: String,
    /// 重修学年
    #[serde(default, rename = "cxxnmc")]
    academic_year: String,
    /// 重修学期
    #[serde(default, rename = "cxxqmc")]
    semester: String,
}

impl From<RawRetakeCourse> for RetakeCourse {
    fn from(raw: RawRetakeCourse) -> Self {
        Self {
            course_code: raw.course_code,
            course_name: raw.course_name,
            credit: parse_credit(&raw.credit),
            college: raw.college,
            category: raw.category,
            retake_status: raw.retake_status,
            academic_year: raw.academic_year,
            semester: raw.semester,
        }
    }
}

/// 解析重修课程查询接口响应
///
/// 输入是 jqGrid 分页信封的原文（数据在 `items` 里）。实测服务端
/// 不按 `cxxnm`/`cxxqm` 过滤、返回全部重修课程记录（4312 条课程库）。
pub fn parse_retake_courses(body: &str) -> Result<Vec<RetakeCourse>> {
    let raw: JqGridEnvelope<RawRetakeCourse> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(RetakeCourse::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重修课程解析（脱敏夹具，键集对齐实测响应）
    #[test]
    fn parses_retake_courses() {
        let body = r#"{
            "currentPage":1,"items":[
              {"cxxnmc":"2024-2025","cxxqmc":"2","cxzt":"空",
               "kch":"0182014","kcmc":"法学概论","kkbmmc":"政治与法律学院",
               "xf":"2.0"},
              {"kcmc":"高等数学","xf":"abc"}
            ],
            "totalResult":"4312"
        }"#;
        let courses = parse_retake_courses(body).unwrap();
        assert_eq!(courses.len(), 2);
        let first = &courses[0];
        assert_eq!(first.course_code, "0182014");
        assert_eq!(first.course_name, "法学概论");
        assert_eq!(first.credit, 2.0);
        assert_eq!(first.college, "政治与法律学院");
        assert_eq!(first.retake_status, "空");
        assert_eq!(first.academic_year, "2024-2025");
        assert_eq!(first.semester, "2");
        // 非法学分退化为 0
        assert_eq!(courses[1].credit, 0.0);
    }
}

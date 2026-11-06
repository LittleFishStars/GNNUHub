//! 学业情况（GPA）解析

use gnnuhub_core::model::GpaSummary;
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::JqGridEnvelope;

/// GPA 统计的一条记录
#[derive(Debug, Default, Deserialize)]
struct RawGpa {
    /// 课程性质代码
    #[serde(default, rename = "kcxzdm")]
    nature_code: String,
    /// 课程性质名称
    #[serde(default, rename = "kcxzmc")]
    nature_name: String,
    /// 平均绩点（字符串数值）
    #[serde(default, rename = "gpa")]
    gpa: String,
    /// 获得学分（字符串数值）
    #[serde(default, rename = "hdxf")]
    earned_credit: String,
}

impl From<RawGpa> for GpaSummary {
    fn from(raw: RawGpa) -> Self {
        Self {
            nature_code: raw.nature_code,
            nature_name: raw.nature_name,
            gpa: raw.gpa.trim().parse().unwrap_or(0.0),
            earned_credit: raw.earned_credit.trim().parse().unwrap_or(0.0),
        }
    }
}

/// 解析学业情况（GPA）查询接口响应
///
/// 实测按课程性质逐组返回（必修/选修等），`xh_id` 留空时服务端
/// 使用会话对应的学生。
pub fn parse_gpa_summary(body: &str) -> Result<Vec<GpaSummary>> {
    let raw: JqGridEnvelope<RawGpa> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(GpaSummary::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 学业情况（GPA）解析（脱敏夹具，键集对齐实测响应）
    #[test]
    fn parses_gpa_summary() {
        let body = r#"{"currentPage":1,"items":[
            {"gpa":"3.00","hdxf":"2","kcxzdm":"02","kcxzmc":"选修课"},
            {"gpa":"2.86","hdxf":"49.5","kcxzdm":"01","kcxzmc":"必修课"}
        ]}"#;
        let items = parse_gpa_summary(body).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].nature_name, "选修课");
        assert_eq!(items[0].gpa, 3.0);
        assert_eq!(items[0].earned_credit, 2.0);
        assert_eq!(items[1].gpa, 2.86);
        assert_eq!(items[1].earned_credit, 49.5);

        // 加权总 GPA = (3.00*2 + 2.86*49.5) / 51.5
        let overall = GpaSummary::overall_gpa(&items);
        assert!((overall - 2.8656).abs() < 0.001, "overall = {overall}");
        assert_eq!(GpaSummary::total_credit(&items), 51.5);
    }
}

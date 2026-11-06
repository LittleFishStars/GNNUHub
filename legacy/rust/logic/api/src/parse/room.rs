//! 空闲教室解析

use gnnuhub_core::model::FreeRoom;
use gnnuhub_core::{Error, Result};
use serde::Deserialize;

use super::JqGridEnvelope;

/// 空闲教室查询的一条记录
#[derive(Debug, Default, Deserialize)]
struct RawFreeRoom {
    /// 场地名称
    #[serde(default, rename = "cdmc")]
    name: String,
    /// 场地编号
    #[serde(default, rename = "cdbh")]
    code: String,
    /// 类别
    #[serde(default, rename = "cdlbmc")]
    category: String,
    /// 归属（学院/校区原文）
    #[serde(default, rename = "xqmc")]
    campus: String,
    /// 教学楼名
    #[serde(default, rename = "jxlmc")]
    building: String,
    /// 座位数
    #[serde(default, rename = "zws")]
    seats: String,
}

impl From<RawFreeRoom> for FreeRoom {
    fn from(raw: RawFreeRoom) -> Self {
        Self {
            name: raw.name,
            code: raw.code,
            category: raw.category,
            campus: raw.campus,
            building: raw.building,
            seats: raw.seats.trim().parse().unwrap_or(0),
        }
    }
}

/// 解析空闲教室查询接口响应
///
/// 形状（实测 1208 条全库）：POST
/// `/cdjy/cdjy_cxKxcdlb.html?gnmkdm=N2155&doType=query`，body 带
/// `xnm`/`xqm`/`syr`（当前学号）/**时间段**（`qssj`/`jssj` 日期、
/// `qssd`/`jssd` `HH:mm:00` 时段——缺时间字段会触发服务端异常页）
/// 与 `zd_fzdm=N211205-kxcdlb`。
pub fn parse_free_rooms(body: &str) -> Result<Vec<FreeRoom>> {
    let raw: JqGridEnvelope<RawFreeRoom> = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(raw.items.into_iter().map(FreeRoom::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空闲教室解析（脱敏夹具，键集对齐实测响应）
    #[test]
    fn parses_free_rooms() {
        let body = r#"{"currentPage":1,"items":[
            {"cdmc":"DNYJ-101","cdbh":"DNYJ101","cdlbmc":"实验室",
             "xqmc":"应急安全学院","jxlmc":"某实验楼","zws":"30","sfkjy":"1"},
            {"cdmc":"运-3WL","zws":"abc"}
        ]}"#;
        let rooms = parse_free_rooms(body).unwrap();
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0].name, "DNYJ-101");
        assert_eq!(rooms[0].category, "实验室");
        assert_eq!(rooms[0].seats, 30);
        // 非法座位数退化 0
        assert_eq!(rooms[1].seats, 0);
    }
}

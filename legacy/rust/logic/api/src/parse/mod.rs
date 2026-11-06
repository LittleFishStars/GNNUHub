//! 教务系统页面与接口数据的解析
//!
//! 教务系统返回两类数据：
//!
//! - **HTML 页面**：个人信息、教学周等需要从 DOM 中抽取
//! - **JSON 接口**：课表、节次时间等
//!
//! 对应 Python 版 `api.py` 中 `_get_basic_info` / `_get_student_info` /
//! `parse_class_schedule` / `get_timetable` / `this_week` 的解析逻辑。
//!
//! # 模块划分
//!
//! 按业务域一个接口一个文件，便于接口变更时定点维护：
//!
//! - [`student`]：学籍与个人信息（首页 / 学籍页 / 学籍 JSON 接口）
//! - [`schedule`]：课表、节次时间、当前教学周
//! - [`grade`]：成绩查询
//! - [`exam`]：考试安排
//! - [`retake`]：重修课程库
//! - [`room`]：空闲教室
//! - [`gpa`]：学业情况（GPA）
//! - [`portal`]：首页门户组件（通知、系统提醒、校历）
//!
//! 所有公开函数都在本模块根再导出，调用方统一走 `parse::函数名`。

mod exam;
mod gpa;
mod grade;
mod portal;
mod retake;
mod room;
mod schedule;
mod student;

pub use exam::parse_exams;
pub use gpa::parse_gpa_summary;
pub use grade::parse_grades;
pub use portal::{parse_calendar, parse_notice_detail, parse_notices, parse_system_messages};
pub use retake::parse_retake_courses;
pub use room::parse_free_rooms;
pub use schedule::{parse_class_schedule, parse_this_week, parse_timetable};
pub use student::{parse_basic_info, parse_student_info, parse_student_profile_json};

use gnnuhub_core::Result;
use serde::Deserialize;

/// jqGrid 分页信封的通用形状
///
/// 正方系统各列表接口（成绩 / 考试 / 重修 / 空闲教室 / GPA）共用同一
/// 分页骨架，数据一律放在 `items` 里；信封上的其余键（`currentPage` /
/// `limit` / `totalResult` 等）由 serde 忽略未知字段的默认行为处理。
///
/// 完整骨架（成绩接口实测原文）：
///
/// ```json
/// {"currentPage":1,"currentResult":0,"entityOrField":false,"items":[],
///  "limit":15,"offset":0,"pageNo":0,"pageSize":15,"showCount":10,
///  "sortName":"xnmmc asc,xqmmc asc,kch asc","sortOrder":" ","sorts":[],
///  "totalCount":0,"totalPage":0,"totalResult":0}
/// ```
///
/// `items` 带 `#[serde(default)]`：空信封与缺失字段都解析成空列表，
/// 与各接口「缺字段留空而不是失败」的容错约定一致。
#[derive(Debug, Deserialize)]
struct JqGridEnvelope<T> {
    #[serde(default)]
    items: Vec<T>,
}

/// 构造一个 CSS 选择器
fn selector(css: &str) -> Result<scraper::Selector> {
    scraper::Selector::parse(css)
        .map_err(|e| gnnuhub_core::Error::PageParse(format!("非法的选择器 {css}: {e:?}")))
}

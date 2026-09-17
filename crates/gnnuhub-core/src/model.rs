//! 教务系统业务数据模型
//!
//! 这里的结构体对应 Python 版 `api.py` 中解析出的数据，
//! 但做了更严格的类型约束（例如学期用枚举而非裸 int）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 学年学期
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Term {
    /// 第一学期（秋季）
    First,
    /// 第二学期（春季）
    Second,
}

impl Term {
    /// 转换为教务系统内部使用的 `xqm` 参数值
    ///
    /// 教务系统用 `3` 表示第一学期，`12` 表示第二学期。
    pub const fn as_xqm(self) -> u8 {
        match self {
            Term::First => 3,
            Term::Second => 12,
        }
    }

    /// 从 1 / 2 构造
    pub fn from_number(n: u32) -> Option<Self> {
        match n {
            1 => Some(Term::First),
            2 => Some(Term::Second),
            _ => None,
        }
    }
}

/// 学年 + 学期的组合，用作课表缓存的键
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AcademicTerm {
    /// 学年，例如 2025 表示 2025-2026 学年
    pub year: u16,
    /// 学期
    pub term: Term,
}

impl AcademicTerm {
    /// 构造一个新的学年学期
    pub const fn new(year: u16, term: Term) -> Self {
        Self { year, term }
    }

    /// 构造第一学期
    pub const fn first(year: u16) -> Self {
        Self::new(year, Term::First)
    }

    /// 构造第二学期
    pub const fn second(year: u16) -> Self {
        Self::new(year, Term::Second)
    }

    /// 教务系统请求参数里的 `xnm` 字段
    pub fn as_xnm(self) -> String {
        self.year.to_string()
    }

    /// 教务系统请求参数里的 `xqm` 字段
    pub fn as_xqm(self) -> u8 {
        self.term.as_xqm()
    }
}

/// 上课节次范围，例如第 1-2 节
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodRange {
    /// 起始节次
    pub start: u8,
    /// 结束节次
    pub end: u8,
}

impl PeriodRange {
    /// 从 `"1-2"` 这样的字符串解析
    pub fn parse(s: &str) -> Option<Self> {
        let (start, end) = s.split_once('-')?;
        Some(Self {
            start: start.trim().parse().ok()?,
            end: end.trim().parse().ok()?,
        })
    }
}

/// 上课时间信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassTime {
    /// 上课周次描述，例如 `"1-16周"`，保留教务系统原始文本
    pub weeks: String,
    /// 星期几，例如 `"星期一"`
    pub weekday: String,
    /// 节次范围
    pub periods: PeriodRange,
}

/// 一门课程的一次具体排课
///
/// 同一门课可能一周上多次，或不同周次在不同教室，
/// 因此课程以「排课条目」为单位存储。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseEntry {
    /// 上课地点
    pub position: String,
    /// 授课教师（可能多人）
    pub teachers: Vec<String>,
    /// 上课时间
    pub time: ClassTime,
    /// 教学班组成，教务系统用 `;` 分隔
    pub classes: Vec<String>,
    /// 教学楼
    pub building: String,
    /// 课程性质，例如「必修」
    pub nature: String,
    /// 课程类别
    pub category: String,
    /// 考核方式
    pub exam_mode: String,
    /// 上课校区
    pub campus: String,
    /// 学分
    pub credit: f32,
}

/// 一个学期的完整课表
///
/// 以课程名为键，值为该课程的全部排课条目。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassSchedule {
    /// 该课表所属的学年学期
    pub academic_term: Option<AcademicTerm>,
    /// 课程名 -> 排课条目列表
    pub courses: BTreeMap<String, Vec<CourseEntry>>,
}

impl ClassSchedule {
    /// 查询指定课程的全部排课
    pub fn course(&self, name: &str) -> &[CourseEntry] {
        self.courses.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// 课程总数
    pub fn course_count(&self) -> usize {
        self.courses.len()
    }
}

/// 学生基本信息
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudentInfo {
    /// 学号
    pub student_id: String,
    /// 姓名
    pub name: Option<String>,
    /// 身份（本科生 / 研究生等）
    pub identity: Option<String>,
    /// 学院
    pub college: Option<String>,
    /// 班级
    pub class_name: Option<String>,
    /// 头像地址
    pub avatar: Option<String>,
    /// 辅导员
    pub instructor: Option<String>,
    /// 专业
    pub major: Option<String>,
    /// 性别
    pub gender: Option<String>,
    /// 专业信息中的其他字段（原 Python 版为 document 字典）
    pub document: Option<Document>,
    /// 出生日期
    pub birthday: Option<String>,
    /// 民族
    pub ethnicity: Option<String>,
    /// 政治面貌
    pub political_status: Option<String>,
    /// 联系地址
    pub address: Option<String>,
    /// 入学年份
    pub enrollment_year: Option<String>,
}

/// 证件信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// 证件类型
    pub kind: String,
    /// 证件号码
    pub number: String,
}

/// 一天中的节次时间（第一节几点开始、第几节几点结束）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodTime {
    /// 开始时间，例如 `"08:00"`
    pub start: String,
    /// 结束时间，例如 `"08:45"`
    pub end: String,
}

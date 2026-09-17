//! 教务系统业务数据模型
//!
//! 这里的结构体对应 Python 版 `api.py` 中解析出的数据，
//! 但做了更严格的类型约束（例如学期用枚举而非裸 int）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 学年学期
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// 解析失败时的占位值
    ///
    /// 节次字段偶尔会返回非预期内容（例如 `"?"`）。课表解析以「尽量保留
    /// 课程记录」为优先，缺节次远好于整条记录丢失，因此退化到第 0 节。
    pub const UNKNOWN: Self = Self { start: 0, end: 0 };
}

impl Default for PeriodRange {
    fn default() -> Self {
        Self::UNKNOWN
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

impl CourseEntry {
    /// 把用分隔符串起来的多个值拆成列表
    ///
    /// 教务系统把教师（`,` 分隔）与教学班（`;` 分隔）塞在同一个字段里，
    /// 空字段与空片段都要丢掉，避免产出 `[""]` 这种噪声。
    pub fn split_list(raw: &str, separator: char) -> Vec<String> {
        raw.split(separator)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// 学分的宽松解析
///
/// 接口以字符串返回学分，偶有 `""` 或非数值内容。学分缺失不影响课表
/// 的可用性，因此解析失败退化为 0.0 而不是让整个响应失败。
pub fn parse_credit(raw: &str) -> f32 {
    raw.trim().parse().unwrap_or(0.0)
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
///
/// 全部字段都是可选的：不同页面（首页 / 学籍页 / 课表接口）各自只提供
/// 其中一部分，需要靠 [`StudentInfo::merge_from`] 逐次补齐。
///
/// 正因如此，`Default` 得到的是一份「什么都不知道」的信息——
/// 学号也用 `Option` 而非空字符串表示「暂无」，这样
/// [`StudentInfo::merge_from`] 才能安全地跳过它。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudentInfo {
    /// 学号
    pub student_id: Option<String>,
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

impl StudentInfo {
    /// 用新取得的信息补齐自己
    ///
    /// 只覆盖对方**确实提供**的字段（`Some`），因此后拉取的页面不会把
    /// 已有数据清空。首页、学籍页、课表接口各自提供的字段互有重叠，
    /// 依次调用本方法即可拼出完整资料。
    pub fn merge_from(&mut self, incoming: Self) {
        macro_rules! fill {
            ($($field:ident),* $(,)?) => {
                $(
                    if incoming.$field.is_some() {
                        self.$field = incoming.$field;
                    }
                )*
            };
        }

        fill!(
            student_id,
            name,
            identity,
            college,
            class_name,
            avatar,
            instructor,
            major,
            gender,
            document,
            birthday,
            ethnicity,
            political_status,
            address,
            enrollment_year,
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `xqm` 取值是教务系统的内部约定，不是 1/2
    #[test]
    fn term_maps_to_xqm() {
        assert_eq!(Term::First.as_xqm(), 3);
        assert_eq!(Term::Second.as_xqm(), 12);
        assert_eq!(Term::from_number(1), Some(Term::First));
        assert_eq!(Term::from_number(2), Some(Term::Second));
        assert_eq!(Term::from_number(3), None);
    }

    /// 节次范围解析
    #[test]
    fn parses_period_range() {
        assert_eq!(
            PeriodRange::parse("1-2"),
            Some(PeriodRange { start: 1, end: 2 })
        );
        // 容错：两端可能带空格
        assert_eq!(
            PeriodRange::parse(" 3 - 4 "),
            Some(PeriodRange { start: 3, end: 4 })
        );
        assert_eq!(PeriodRange::parse("5"), None);
        assert_eq!(PeriodRange::parse("a-b"), None);
    }

    /// 合并时只补齐对方提供的字段
    #[test]
    fn merge_fills_only_provided_fields() {
        let mut base = StudentInfo {
            student_id: Some("250710078".into()),
            name: Some("张三".into()),
            ..Default::default()
        };
        let incoming = StudentInfo {
            college: Some("数学与计算机科学学院".into()),
            ..Default::default()
        };

        base.merge_from(incoming);

        assert_eq!(base.name.as_deref(), Some("张三"), "原有值应保留");
        assert_eq!(
            base.college.as_deref(),
            Some("数学与计算机科学学院"),
            "新字段应被补上"
        );
        assert_eq!(base.student_id.as_deref(), Some("250710078"));
    }

    /// 对方提供了新值时应覆盖
    #[test]
    fn merge_overwrites_provided_fields() {
        let mut base = StudentInfo {
            name: Some("旧名字".into()),
            ..Default::default()
        };
        base.merge_from(StudentInfo {
            name: Some("新名字".into()),
            ..Default::default()
        });
        assert_eq!(base.name.as_deref(), Some("新名字"));
    }

    /// 空合并不得清空任何已有值
    #[test]
    fn merge_with_empty_keeps_everything() {
        let mut base = StudentInfo {
            student_id: Some("250710078".into()),
            name: Some("张三".into()),
            gender: Some("男".into()),
            document: Some(Document {
                kind: "居民身份证".into(),
                number: "360101200001011234".into(),
            }),
            ..Default::default()
        };
        let snapshot = base.clone();

        base.merge_from(StudentInfo::default());

        assert_eq!(base, snapshot, "空信息来源不应改变任何字段");
    }

    /// 分隔符列表应去空白、丢空片段
    #[test]
    fn splits_separated_lists() {
        assert_eq!(
            CourseEntry::split_list("王老师, 刘老师", ','),
            vec!["王老师", "刘老师"]
        );
        assert_eq!(
            CourseEntry::split_list("计算机2201班;计算机2202班", ';'),
            vec!["计算机2201班", "计算机2202班"]
        );
        // 空字段与多余分隔符都不应产出空字符串
        assert!(CourseEntry::split_list("", ',').is_empty());
        assert_eq!(CourseEntry::split_list("a,,b,", ','), vec!["a", "b"]);
        assert_eq!(CourseEntry::split_list("  ", ',').len(), 0);
    }

    /// 学分解析失败应退化为 0 而非报错
    #[test]
    fn parses_credit_leniently() {
        assert_eq!(parse_credit("4.0"), 4.0);
        assert_eq!(parse_credit(" 1.5 "), 1.5);
        assert_eq!(parse_credit(""), 0.0);
        assert_eq!(parse_credit("未知"), 0.0);
    }

    /// 节次解析失败的占位值就是默认值
    #[test]
    fn unknown_period_range_is_default() {
        assert_eq!(PeriodRange::default(), PeriodRange::UNKNOWN);
        assert_eq!(PeriodRange::default().start, 0);
    }
}

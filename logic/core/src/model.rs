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

/// 周次区间的奇偶修饰
///
/// 教务系统用 `(单)` / `(双)` 表示区间内只上单（奇数）周或双（偶数）周。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeekParity {
    /// 不区分奇偶，每周都上
    All,
    /// 单周（奇数周）
    Odd,
    /// 双周（偶数周）
    Even,
}

/// 一段连续的周次区间（含奇偶修饰）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WeekRange {
    start: u8,
    end: u8,
    parity: WeekParity,
}

impl WeekRange {
    /// 判断某一周是否落在本区间内
    fn matches(self, week: u8) -> bool {
        (self.start..=self.end).contains(&week)
            && match self.parity {
                WeekParity::All => true,
                WeekParity::Odd => week % 2 == 1,
                WeekParity::Even => week % 2 == 0,
            }
    }
}

/// 解析周次描述文本
///
/// 教务系统 `zcd` 字段的已知格式（均来自真实响应）：
///
/// - `"1-18周"` — 连续区间
/// - `"7-13周(单)"` — 区间内只上奇数周
/// - 多段以逗号分隔：`"1-8周(双),10-16周"`（全角逗号同样兼容）
///
/// 返回 `None` 表示文本为空或不符合已知格式；调用方应保守处理
/// （当作「覆盖任意周」），宁可多显示一门课也不要漏掉。
fn parse_week_ranges(text: &str) -> Option<Vec<WeekRange>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut ranges = Vec::new();
    for segment in text.split([',', '，']) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // 奇偶修饰藏在括号里：`(单)` / `（双）`
        let (body, parity) = match segment.find(['(', '（']) {
            Some(idx) => {
                let tail = &segment[idx..];
                let parity = if tail.contains('单') {
                    WeekParity::Odd
                } else if tail.contains('双') {
                    WeekParity::Even
                } else {
                    // 未知修饰词，整体视为不可解析
                    return None;
                };
                (&segment[..idx], parity)
            }
            None => (segment, WeekParity::All),
        };

        // 去掉「周」「第」等装饰字符后解析数字区间
        let body = body.replace(['周', '第'], "");
        let (start, end) = match body.trim().split_once('-') {
            Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
            None => {
                // 「第17周」这类单周次写法
                let v = body.trim().parse().ok()?;
                (v, v)
            }
        };
        ranges.push(WeekRange { start, end, parity });
    }

    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

impl ClassTime {
    /// 判断该排课是否覆盖给定的教学周
    ///
    /// 依据 [`Self::weeks`] 的原文解析；文本不可解析时**保守返回
    /// `true`**——单周过滤服务于展示场景，多显示一门课的代价远小于
    /// 漏掉一门课。
    pub fn covers_week(&self, week: u8) -> bool {
        parse_week_ranges(&self.weeks)
            .map(|ranges| ranges.iter().any(|r| r.matches(week)))
            .unwrap_or(true)
    }
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

    /// 提取指定教学周的课表视图
    ///
    /// 以整学期课表为基础做客户端过滤：只保留当周有课的排课条目，
    /// 当周没有任何排课的课程不会出现在结果里。配合整学期课表接口
    /// 使用时，单周视图 0 次额外请求即可获得，也不依赖任何按周查询
    /// 的远程接口。
    pub fn week_view(&self, week: u8) -> ClassSchedule {
        let mut view = ClassSchedule {
            academic_term: self.academic_term,
            courses: BTreeMap::new(),
        };
        for (name, entries) in &self.courses {
            let kept: Vec<CourseEntry> = entries
                .iter()
                .filter(|e| e.time.covers_week(week))
                .cloned()
                .collect();
            if !kept.is_empty() {
                view.courses.insert(name.clone(), kept);
            }
        }
        view
    }
}

/// 一条成绩记录
///
/// 字段与成绩查询页前端的 `colModel` 一一对应（`cxDgXscj.js`
/// 的 `getGridColModel()`，即学校 `10418` 走的学生分支）。
///
/// # 成绩字段为什么有四个
///
/// 教务系统对「同一门课」会并列给出四组数值，含义完全不同，
/// 混用会算错绩点：
///
/// | 字段 | 含义 |
/// |---|---|
/// | [`Self::score`] (`cj`) | 用于**展示**的成绩，可能已按补考/重修折算 |
/// | [`Self::raw_score`] (`bfzcj`) | **百分制原始分**，前端拿它判 `< 60` 标红 |
/// | [`Self::grade_point`] (`jd`) | 课程绩点 |
/// | [`Self::credit_grade_point`] (`xfjd`) | 学分绩点 = 学分 × 绩点 |
///
/// 字符串字段一律保留原始文本：教务系统会用 `"优秀"`、`"合格"`、
/// `"通过"` 等非数值形式表示考查课成绩，转成数字会丢信息。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GradeRecord {
    /// 学年，例如 `"2025-2026"`
    pub academic_year: String,
    /// 学期，例如 `"1"`
    pub semester: String,
    /// 课程代码
    pub course_code: String,
    /// 课程名称
    pub course_name: String,
    /// 课程性质，例如「必修」
    pub nature: String,
    /// 学分
    pub credit: f32,
    /// 成绩（展示用文本）
    pub score: String,
    /// 成绩备注
    pub score_note: String,
    /// 绩点
    pub grade_point: String,
    /// 成绩性质，例如「正常考试」「补考」
    pub score_type: String,
    /// 是否成绩作废
    pub score_voided: String,
    /// 是否学位课程
    pub is_degree_course: String,
    /// 开课学院
    pub college: String,
    /// 课程标记（主修 / 辅修 …）
    pub course_mark: String,
    /// 课程类别
    pub category: String,
    /// 课程归属
    pub attribution: String,
    /// 教学班
    pub class_name: String,
    /// 任课教师
    pub teacher: String,
    /// 考核方式
    pub exam_mode: String,
    /// 学生标记
    pub student_mark: String,
    /// 学分绩点
    pub credit_grade_point: String,
    /// 百分制原始分（仅 `bfzcj` 存在时才有值）
    ///
    /// 前端用 `bfzcj < 60` 判定「不及格标红，及格标蓝」，
    /// 而 `cj` 在补考/重修后会变成折算值，不能用于该判定。
    pub raw_score: Option<f32>,
}

impl GradeRecord {
    /// 成绩是否已通过
    ///
    /// 优先看原始分（≥60 视为通过）；没有原始分时退化为文本判断。
    /// 文本判断采用**白名单**：教务系统对考查课常用「优秀/良好/合格/
    /// 中等/及格/通过」表示通过，其余（含「不合格」「缺考」「作弊」）
    /// 一律视为未通过——保守方向更安全，避免把未通过算成已通过。
    pub fn passed(&self) -> bool {
        if let Some(raw) = self.raw_score {
            return raw >= 60.0;
        }
        const PASSING: &[&str] = &[
            "优秀", "良好", "中等", "合格", "及格", "通过", "优", "良", "中",
        ];
        let text = self.score.trim();
        // 先排除明确的否定词，避免「不合格」被「合格」匹配上
        if text.contains("不合格") || text.contains("不通过") || text.contains("未通过") {
            return false;
        }
        PASSING.iter().any(|p| text.contains(p))
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

    /// 构造仅指定周次文本的 ClassTime，方便逐条验证解析规则
    fn time_with_weeks(weeks: &str) -> ClassTime {
        ClassTime {
            weeks: weeks.to_string(),
            weekday: String::new(),
            periods: PeriodRange::default(),
        }
    }

    /// 周次文本解析：连续区间
    #[test]
    fn covers_week_range() {
        let t = time_with_weeks("1-18周");
        assert!(t.covers_week(1));
        assert!(t.covers_week(9));
        assert!(t.covers_week(18));
        assert!(!t.covers_week(19));
        assert!(!t.covers_week(0));
    }

    /// 周次文本解析：单双周修饰
    #[test]
    fn covers_week_odd_even() {
        let odd = time_with_weeks("7-13周(单)");
        assert!(odd.covers_week(7));
        assert!(odd.covers_week(13));
        assert!(!odd.covers_week(8), "双周不应命中(单)区间");

        let even = time_with_weeks("1-16周(双)");
        assert!(even.covers_week(2));
        assert!(even.covers_week(16));
        assert!(!even.covers_week(1), "奇数周不应命中(双)区间");
    }

    /// 周次文本解析：多段与全角逗号、单周次写法
    #[test]
    fn covers_week_multi_segment() {
        let multi = time_with_weeks("1-8周,10-16周");
        assert!(multi.covers_week(5));
        assert!(!multi.covers_week(9), "空档周不应命中");
        assert!(multi.covers_week(10));

        let fullwidth = time_with_weeks("1-8周，10-16周");
        assert!(fullwidth.covers_week(5));
        assert!(!fullwidth.covers_week(9));

        let single = time_with_weeks("第17周");
        assert!(single.covers_week(17));
        assert!(!single.covers_week(16));
    }

    /// 周次文本不可解析时保守当作覆盖
    #[test]
    fn covers_week_defaults_to_true_when_unparsable() {
        assert!(time_with_weeks("").covers_week(3), "空文本应保守命中");
        assert!(
            time_with_weeks("第?周").covers_week(3),
            "畸形文本应保守命中"
        );
        assert!(
            time_with_weeks("1-8周(上机)").covers_week(3),
            "未知括号修饰词应保守命中"
        );
    }

    /// week_view 只保留当周有课的条目，当周无课的课程整体消失
    #[test]
    fn week_view_filters_by_week() {
        let mk = |name: &str, weeks: &str| {
            (
                name.to_string(),
                vec![CourseEntry {
                    position: String::new(),
                    teachers: vec![],
                    time: ClassTime {
                        weeks: weeks.to_string(),
                        weekday: "星期一".to_string(),
                        periods: PeriodRange { start: 1, end: 2 },
                    },
                    classes: vec![],
                    building: String::new(),
                    nature: String::new(),
                    category: String::new(),
                    exam_mode: String::new(),
                    campus: String::new(),
                    credit: 0.0,
                }],
            )
        };

        let schedule = ClassSchedule {
            academic_term: Some(AcademicTerm::first(2025)),
            courses: BTreeMap::from([
                mk("数学分析", "1-18周"),
                mk("大学英语", "7-13周(单)"),
                mk("体育", "3-4周"),
            ]),
        };

        // 第 7 周：数学分析命中、大学英语(单周)命中、体育已结束
        let view = schedule.week_view(7);
        assert_eq!(view.course_count(), 2);
        assert_eq!(view.course("数学分析").len(), 1);
        assert_eq!(view.course("大学英语").len(), 1);
        assert!(view.course("体育").is_empty());

        // 第 4 周：数学分析命中、大学英语(单周)不命中、体育进行中
        let view = schedule.week_view(4);
        assert_eq!(view.course_count(), 2);
        assert!(view.course("大学英语").is_empty());
        assert_eq!(view.course("体育").len(), 1);

        // 视图继承学期信息
        assert_eq!(view.academic_term, Some(AcademicTerm::first(2025)));
    }
}

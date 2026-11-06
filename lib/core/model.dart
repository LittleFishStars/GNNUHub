/// 教务系统业务数据模型 —— 对应 Rust `logic/core/src/model.rs`
///
/// 类型与字段与 Rust 版一一对应；教务系统各接口返回的字段名
/// 保持原样在解析层处理。
library;

// ---------------------------------------------------------------------------
// 学期
// ---------------------------------------------------------------------------

/// 学期（第一学期=秋季 / 第二学期=春季）
enum Term {
  first,
  second;

  /// 教务系统内部 `xqm` 参数值：第一学期=3、第二学期=12
  int get asXqm => this == Term.first ? 3 : 12;

  /// 展示名
  String get label => this == Term.first ? '第一学期' : '第二学期';

  /// 从 1/2 构造，非法返回 null
  static Term? fromNumber(int n) => switch (n) {
        1 => Term.first,
        2 => Term.second,
        _ => null,
      };
}

/// 学年 + 学期组合（课表等缓存键）
class AcademicTerm {
  const AcademicTerm({required this.year, required this.term});

  /// 学年，例如 2025 表示 2025-2026 学年
  final int year;
  final Term term;

  static AcademicTerm first(int year) =>
      AcademicTerm(year: year, term: Term.first);
  static AcademicTerm second(int year) =>
      AcademicTerm(year: year, term: Term.second);

  /// 教务系统请求参数 `xnm`
  String get asXnm => year.toString();

  /// 教务系统请求参数 `xqm`
  int get asXqm => term.asXqm;

  @override
  bool operator ==(Object other) =>
      other is AcademicTerm && other.year == year && other.term == term;

  @override
  int get hashCode => Object.hash(year, term);

  @override
  String toString() => '$year-${year + 1} ${term.label}';
}

// ---------------------------------------------------------------------------
// 课表
// ---------------------------------------------------------------------------

/// 节次范围，例如第 1-2 节
class PeriodRange {
  const PeriodRange({required this.start, required this.end});

  final int start;
  final int end;

  /// 解析失败的占位值（第 0 节）——缺节次远好于整条记录丢失
  static const unknown = PeriodRange(start: 0, end: 0);

  /// 从 `"1-2"` 解析；失败返回 null
  static PeriodRange? tryParse(String s) {
    final parts = s.split('-');
    if (parts.length != 2) return null;
    final a = int.tryParse(parts[0].trim());
    final b = int.tryParse(parts[1].trim());
    if (a == null || b == null) return null;
    return PeriodRange(start: a, end: b);
  }

  @override
  bool operator ==(Object other) =>
      other is PeriodRange && other.start == start && other.end == end;

  @override
  int get hashCode => Object.hash(start, end);
}

/// 周次区间奇偶修饰
enum WeekParity { all, odd, even }

/// 一段连续周次区间（含奇偶修饰）
class WeekRange {
  const WeekRange({
    required this.start,
    required this.end,
    this.parity = WeekParity.all,
  });

  final int start;
  final int end;
  final WeekParity parity;

  bool matches(int week) {
    if (week < start || week > end) return false;
    return switch (parity) {
      WeekParity.all => true,
      WeekParity.odd => week.isOdd,
      WeekParity.even => week.isEven,
    };
  }
}

/// 解析周次描述文本（`zcd` 字段）
///
/// 已知格式：`"1-18周"`、`"7-13周(单)"`、多段逗号分隔（全角兼容）。
/// 返回 null 表示不可解析，调用方应保守当作「覆盖任意周」。
List<WeekRange>? parseWeekRanges(String text) {
  text = text.trim();
  if (text.isEmpty) return null;

  final ranges = <WeekRange>[];
  for (var segment in text.split(RegExp(r'[,，]'))) {
    segment = segment.trim();
    if (segment.isEmpty) continue;

    WeekParity parity = WeekParity.all;
    var body = segment;
    final bracket = segment.indexOf(RegExp(r'[（(]'));
    if (bracket >= 0) {
      final tail = segment.substring(bracket);
      if (tail.contains('单')) {
        parity = WeekParity.odd;
      } else if (tail.contains('双')) {
        parity = WeekParity.even;
      } else {
        // 未知修饰词（如「(上机)」），整体视为不可解析
        return null;
      }
      body = segment.substring(0, bracket);
    }

    body = body.replaceAll(RegExp(r'[周第]'), '').trim();
    final parts = body.split('-');
    int? start;
    int? end;
    if (parts.length == 2) {
      start = int.tryParse(parts[0].trim());
      end = int.tryParse(parts[1].trim());
    } else {
      // 「第17周」这类单周次写法
      start = int.tryParse(body);
      end = start;
    }
    if (start == null || end == null) return null;
    ranges.add(WeekRange(start: start, end: end, parity: parity));
  }
  return ranges.isEmpty ? null : ranges;
}

/// 上课时间信息
class ClassTime {
  const ClassTime({
    required this.weeks,
    required this.weekday,
    required this.periods,
  });

  /// 上课周次描述原文，例如 `"1-16周"`
  final String weeks;

  /// 星期几，例如 `"星期一"`
  final String weekday;

  /// 节次范围
  final PeriodRange periods;

  /// 该排课是否覆盖给定教学周；文本不可解析时保守返回 true
  bool coversWeek(int week) =>
      parseWeekRanges(weeks)?.any((r) => r.matches(week)) ?? true;
}

/// 一门课程的一次具体排课
class CourseEntry {
  const CourseEntry({
    required this.position,
    required this.teachers,
    required this.time,
    required this.classes,
    required this.building,
    required this.nature,
    required this.category,
    required this.examMode,
    required this.campus,
    required this.credit,
  });

  final String position;
  final List<String> teachers;
  final ClassTime time;
  final List<String> classes;
  final String building;
  final String nature;
  final String category;
  final String examMode;
  final String campus;
  final double credit;

  /// 把分隔符串接的多个值拆成列表（去空白、丢空片段）
  static List<String> splitList(String raw, String separator) => raw
      .split(separator)
      .map((s) => s.trim())
      .where((s) => s.isNotEmpty)
      .toList();
}

/// 学分的宽松解析；失败退化为 0
double parseCredit(String raw) =>
    double.tryParse(raw.trim()) ?? 0.0;

/// 一个学期的完整课表（课程名 → 排课条目）
class ClassSchedule {
  ClassSchedule({this.academicTerm, Map<String, List<CourseEntry>>? courses})
      : courses = courses ?? <String, List<CourseEntry>>{};

  final AcademicTerm? academicTerm;
  final Map<String, List<CourseEntry>> courses;

  /// 课程总数
  int get courseCount => courses.length;

  /// 提取指定教学周的课表视图（客户端过滤，0 次额外请求）
  ClassSchedule weekView(int week) {
    final view = ClassSchedule(academicTerm: academicTerm);
    courses.forEach((name, entries) {
      final kept = entries.where((e) => e.time.coversWeek(week)).toList();
      if (kept.isNotEmpty) view.courses[name] = kept;
    });
    return view;
  }
}

// ---------------------------------------------------------------------------
// 成绩
// ---------------------------------------------------------------------------

/// 一条成绩记录（字段对应 cxDgXscj.js colModel）
class GradeRecord {
  const GradeRecord({
    required this.academicYear,
    required this.semester,
    required this.courseCode,
    required this.courseName,
    required this.nature,
    required this.credit,
    required this.score,
    required this.scoreNote,
    required this.gradePoint,
    required this.scoreType,
    required this.scoreVoided,
    required this.isDegreeCourse,
    required this.college,
    required this.courseMark,
    required this.category,
    required this.attribution,
    required this.className,
    required this.teacher,
    required this.examMode,
    required this.studentMark,
    required this.creditGradePoint,
    this.rawScore,
  });

  final String academicYear;
  final String semester;
  final String courseCode;
  final String courseName;
  final String nature;
  final double credit;

  /// 成绩（展示用文本，可能为「优秀」等非数值形式）
  final String score;
  final String scoreNote;
  final String gradePoint;
  final String scoreType;
  final String scoreVoided;
  final String isDegreeCourse;
  final String college;
  final String courseMark;
  final String category;
  final String attribution;
  final String className;
  final String teacher;
  final String examMode;
  final String studentMark;
  final String creditGradePoint;

  /// 百分制原始分（bfzcj，仅存在时才有值；前端用它判 <60 标红）
  final double? rawScore;

  /// 成绩是否已通过（白名单文本判断，保守方向）
  bool get passed {
    final raw = rawScore;
    if (raw != null) return raw >= 60.0;
    const passing = ['优秀', '良好', '中等', '合格', '及格', '通过', '优', '良', '中'];
    final text = score.trim();
    if (text.contains('不合格') || text.contains('不通过') || text.contains('未通过')) {
      return false;
    }
    return passing.any(text.contains);
  }
}

// ---------------------------------------------------------------------------
// 学籍信息
// ---------------------------------------------------------------------------

/// 证件信息
class DocumentInfo {
  const DocumentInfo({required this.kind, required this.number});
  final String kind;
  final String number;

  @override
  bool operator ==(Object other) =>
      other is DocumentInfo &&
      other.kind == kind &&
      other.number == number;

  @override
  int get hashCode => Object.hash(kind, number);
}

/// 学生基本信息（全部字段可选，靠 [mergeFrom] 逐次补齐）
class StudentInfo {
  StudentInfo({
    this.studentId,
    this.name,
    this.identity,
    this.college,
    this.className,
    this.avatar,
    this.instructor,
    this.major,
    this.gender,
    this.document,
    this.birthday,
    this.ethnicity,
    this.politicalStatus,
    this.address,
    this.enrollmentYear,
  });

  String? studentId;
  String? name;
  String? identity;
  String? college;
  String? className;
  String? avatar;
  String? instructor;
  String? major;
  String? gender;
  DocumentInfo? document;
  String? birthday;
  String? ethnicity;
  String? politicalStatus;
  String? address;
  String? enrollmentYear;

  /// 用新取得的信息补齐自己（只覆盖对方确实提供的字段）
  void mergeFrom(StudentInfo incoming) {
    void fill(Object? Function() get, void Function(Object?) set) {
      final v = get();
      if (v != null) set(v);
    }

    fill(() => incoming.studentId, (v) => studentId = v as String?);
    fill(() => incoming.name, (v) => name = v as String?);
    fill(() => incoming.identity, (v) => identity = v as String?);
    fill(() => incoming.college, (v) => college = v as String?);
    fill(() => incoming.className, (v) => className = v as String?);
    fill(() => incoming.avatar, (v) => avatar = v as String?);
    fill(() => incoming.instructor, (v) => instructor = v as String?);
    fill(() => incoming.major, (v) => major = v as String?);
    fill(() => incoming.gender, (v) => gender = v as String?);
    fill(() => incoming.document, (v) => document = v as DocumentInfo?);
    fill(() => incoming.birthday, (v) => birthday = v as String?);
    fill(() => incoming.ethnicity, (v) => ethnicity = v as String?);
    fill(() => incoming.politicalStatus,
        (v) => politicalStatus = v as String?);
    fill(() => incoming.address, (v) => address = v as String?);
    fill(() => incoming.enrollmentYear, (v) => enrollmentYear = v as String?);
  }
}

// ---------------------------------------------------------------------------
// 考试
// ---------------------------------------------------------------------------

/// 一条考试安排记录（字段对应 cxXsksxxIndex.js colModel）
class ExamRecord {
  const ExamRecord({
    required this.courseCode,
    required this.courseName,
    required this.examName,
    required this.examTime,
    required this.location,
    required this.campus,
    required this.examMode,
    required this.assessment,
    required this.college,
    required this.teacher,
    required this.makeUp,
    required this.className,
    required this.credit,
    required this.academicYear,
    required this.semester,
    required this.classTime,
    this.durationMinutes,
  });

  final String courseCode;
  final String courseName;

  /// 考试批次名称
  final String examName;

  /// 考试时间原文，例如 `"2026-07-07(14:30-16:30)"`
  final String examTime;
  final String location;
  final String campus;
  final String examMode;
  final String assessment;
  final String college;
  final String teacher;

  /// 是否补考（原文「是/否」）
  final String makeUp;
  final String className;
  final double credit;
  final String academicYear;
  final String semester;

  /// 该课程的上课时间（sksj 原文）
  final String classTime;

  /// 考试时长（分钟；缺失或非法为 null）
  final int? durationMinutes;

  bool get isMakeUp => makeUp.contains('是');
}

// ---------------------------------------------------------------------------
// 校历
// ---------------------------------------------------------------------------

/// 学年学期校历信息（据此可推算当前教学周）
class AcademicCalendar {
  const AcademicCalendar({
    required this.year,
    required this.term,
    required this.startDate,
    required this.endDate,
  });

  final int year;
  final int term;
  final String startDate;
  final String endDate;

  /// 根据校历与 [today] 推算当前教学周；学期未开始或日期解析失败返回 null
  int? currentWeek(DateTime today) {
    final start = DateTime.tryParse(startDate);
    if (start == null) return null;
    final startDay = DateTime(start.year, start.month, start.day);
    final todayDay = DateTime(today.year, today.month, today.day);
    final days = todayDay.difference(startDay).inDays;
    if (days < 0) return null;
    return days ~/ 7 + 1;
  }
}

// ---------------------------------------------------------------------------
// GPA / 通知 / 教室 / 重修 / 节次
// ---------------------------------------------------------------------------

/// 一条按课程性质汇总的学业情况
class GpaSummary {
  const GpaSummary({
    required this.natureCode,
    required this.natureName,
    required this.gpa,
    required this.earnedCredit,
  });

  final String natureCode;
  final String natureName;
  final double gpa;
  final double earnedCredit;

  /// 总加权平均绩点：Σ(GPA×学分) ÷ Σ学分
  static double overallGpa(List<GpaSummary> items) {
    var total = 0.0;
    var credits = 0.0;
    for (final i in items) {
      total += i.gpa * i.earnedCredit;
      credits += i.earnedCredit;
    }
    return credits > 0 ? total / credits : 0.0;
  }

  /// 已获得学分合计
  static double totalCredit(List<GpaSummary> items) =>
      items.fold(0.0, (s, i) => s + i.earnedCredit);
}

/// 一条通知公告
class Notice {
  const Notice({
    required this.title,
    required this.date,
    required this.detailPath,
    required this.pinned,
    required this.category,
  });

  final String title;
  final String date;
  final String detailPath;
  final bool pinned;
  final String category;
}

/// 通知详情
class NoticeDetail {
  const NoticeDetail({
    required this.title,
    required this.publisher,
    required this.publishTime,
    required this.views,
    required this.content,
  });

  final String title;
  final String publisher;
  final String publishTime;
  final String views;
  final String content;
}

/// 一条空闲教室（场地）记录
class FreeRoom {
  const FreeRoom({
    required this.name,
    required this.code,
    required this.category,
    required this.campus,
    required this.building,
    required this.seats,
  });

  final String name;
  final String code;
  final String category;
  final String campus;
  final String building;
  final int seats;
}

/// 一条重修课程记录
class RetakeCourse {
  const RetakeCourse({
    required this.courseCode,
    required this.courseName,
    required this.credit,
    required this.college,
    required this.category,
    required this.retakeStatus,
    required this.academicYear,
    required this.semester,
  });

  final String courseCode;
  final String courseName;
  final double credit;
  final String college;
  final String category;

  /// 重修状态（原文，「空」表示未开班）
  final String retakeStatus;
  final String academicYear;
  final String semester;
}

/// 一天中的节次时间
class PeriodTime {
  const PeriodTime({required this.start, required this.end});
  final String start;
  final String end;
}

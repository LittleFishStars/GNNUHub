/// 课表、节次时间、当前教学周解析 —— 对应 Rust `parse/schedule.rs`
library;

import 'dart:convert';

import 'package:html/parser.dart';

import '../../core/error.dart';
import '../../core/model.dart';
import 'common.dart';

/// 课表接口响应 →（课表, 学生简讯）
///
/// ⚠️ 无数据时服务端返回**字面量 `null`** → 按空课表处理，不报错。
/// `kbList` 优先，空则回落 `kbListnew`。
/// ⚠️ `ZYMC`（专业名）映射到 **college**（原实现如此，照抄勿「修正」）。
(ClassSchedule, StudentInfo) parseClassSchedule(String body) {
  final schedule = ClassSchedule();
  final info = StudentInfo();

  dynamic root;
  try {
    root = jsonDecode(body);
  } on FormatException {
    root = null; // 与字面量 null 同路处理
  }
  if (root == null || root is! Map) {
    return (schedule, info);
  }
  final map = root.cast<String, dynamic>();

  // 学生简讯（键名全大写）
  final xsxx = map['xsxx'];
  if (xsxx is Map) {
    final brief = xsxx.cast<String, dynamic>();
    void fill(String key, void Function(String) set) {
      final v = brief[key];
      if (v is String && v.isNotEmpty) set(v);
    }

    fill('XM', (v) => info.name = v);
    fill('ZYMC', (v) => info.college = v); // 原实现映射到 college
    fill('BJMC', (v) => info.className = v);
    fill('JSXM', (v) => info.instructor = v);
  }

  final kbList = (map['kbList'] as List?) ?? const [];
  final kbListNew = (map['kbListnew'] as List?) ?? const [];
  final courses = (kbList.isEmpty ? kbListNew : kbList)
      .whereType<Map>()
      .map((c) => c.cast<String, dynamic>());

  for (final course in courses) {
    final name = (course['kcmc'] as String?) ?? '';
    if (name.isEmpty) continue; // 缺课程名跳过该条（比原实现更安全）

    final entry = CourseEntry(
      position: (course['cdmc'] as String?) ?? '',
      teachers: CourseEntry.splitList((course['xm'] as String?) ?? '', ','),
      time: ClassTime(
        weeks: (course['zcd'] as String?) ?? '',
        weekday: (course['xqjmc'] as String?) ?? '',
        periods: PeriodRange.tryParse((course['jcs'] as String?) ?? '') ??
            PeriodRange.unknown,
      ),
      classes:
          CourseEntry.splitList((course['jxbzc'] as String?) ?? '', ';'),
      building: (course['lh'] as String?) ?? '',
      nature: (course['kcxz'] as String?) ?? '',
      category: (course['kclb'] as String?) ?? '',
      examMode: (course['khfsmc'] as String?) ?? '',
      campus: (course['xqmc'] as String?) ?? '',
      credit: parseNum(course['xf'] as String?),
    );
    schedule.courses.putIfAbsent(name, () => []).add(entry);
  }

  return (schedule, info);
}

/// 节次时间表（裸 JSON 数组 `[{"qssj":"08:00","jssj":"08:45"},...]`）
List<PeriodTime> parseTimetable(String body) {
  dynamic root;
  try {
    root = jsonDecode(body);
  } on FormatException catch (e) {
    throw JsonError(e.message);
  }
  if (root is! List) return const [];
  return [
    for (final item in root)
      if (item is Map)
        PeriodTime(
          start: (item['qssj'] as String?) ?? '',
          end: (item['jssj'] as String?) ?? '',
        ),
  ];
}

/// 教学周页 HTML → 当前教学周（`#zs option[selected]`）
///
/// 找不到报错（不是返回 0）；优先改用校历 [AcademicCalendar.currentWeek]。
int parseThisWeek(String html) {
  final doc = parse(html);
  final option = doc.querySelector('#zs option[selected]');
  if (option == null) {
    throw const PageParseError('未找到已选中的教学周选项');
  }
  final text = allText(option);
  final before = text.split('(').first.trim();
  final week = int.tryParse(before);
  if (week == null) {
    throw PageParseError('无法解析教学周: "$text"');
  }
  return week;
}

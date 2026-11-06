/// 成绩、考试、重修、教室、GPA 的 jqGrid 列表解析
/// —— 对应 Rust `parse/{grade,exam,retake,room,gpa}.rs`
///
/// 五个 parse_* 函数体完全同形：读信封 → 逐条 map，不做跨记录统计。
library;

import '../../core/model.dart';
import 'common.dart';

/// 成绩记录（字段名取自 cxDgXscj.js colModel）
List<GradeRecord> parseGrades(String body) => [
      for (final item in jqItems(body))
        GradeRecord(
          academicYear: (item['xnmmc'] as String?) ?? '',
          semester: (item['xqmmc'] as String?) ?? '',
          courseCode: (item['kch'] as String?) ?? '',
          courseName: (item['kcmc'] as String?) ?? '',
          nature: (item['kcxzmc'] as String?) ?? '',
          credit: parseNum(item['xf'] as String?),
          score: (item['cj'] as String?) ?? '',
          scoreNote: (item['cjbz'] as String?) ?? '',
          gradePoint: (item['jd'] as String?) ?? '',
          scoreType: (item['ksxz'] as String?) ?? '',
          scoreVoided: (item['cjsfzf'] as String?) ?? '',
          isDegreeCourse: (item['sfxwkc'] as String?) ?? '',
          college: (item['kkbmmc'] as String?) ?? '',
          courseMark: (item['kcbj'] as String?) ?? '',
          category: (item['kclbmc'] as String?) ?? '',
          attribution: (item['kcgsmc'] as String?) ?? '',
          className: (item['jxbmc'] as String?) ?? '',
          teacher: (item['jsxm'] as String?) ?? '',
          examMode: (item['khfsmc'] as String?) ?? '',
          studentMark: (item['xsbjmc'] as String?) ?? '',
          creditGradePoint: (item['xfjd'] as String?) ?? '',
          // bfzcj 是唯一原生数值字段且可为 null
          rawScore: (item['bfzcj'] as num?)?.toDouble(),
        ),
    ];

/// 考试安排（cxXsksxxIndex.js 学生分支）
List<ExamRecord> parseExams(String body) => [
      for (final item in jqItems(body))
        ExamRecord(
          courseCode: (item['kch'] as String?) ?? '',
          courseName: (item['kcmc'] as String?) ?? '',
          examName: (item['ksmc'] as String?) ?? '',
          examTime: (item['kssj'] as String?) ?? '',
          location: (item['cdmc'] as String?) ?? '',
          campus: (item['cdxqmc'] as String?) ?? '',
          examMode: (item['ksfs'] as String?) ?? '',
          assessment: (item['khfs'] as String?) ?? '',
          college: (item['kkxy'] as String?) ?? '',
          teacher: (item['jsxx'] as String?) ?? '',
          makeUp: (item['cxbj'] as String?) ?? '',
          className: (item['jxbmc'] as String?) ?? '',
          credit: parseNum(item['xf'] as String?),
          academicYear: (item['xnmc'] as String?) ?? '', // 成绩接口是 xnmmc
          semester: (item['xqmmc'] as String?) ?? '',
          classTime: (item['sksj'] as String?) ?? '',
          durationMinutes:
              int.tryParse(((item['sjsj'] as String?) ?? '').trim()),
        ),
    ];

/// 重修课程库（cxCxkccxIndex.js）
List<RetakeCourse> parseRetakeCourses(String body) => [
      for (final item in jqItems(body))
        RetakeCourse(
          courseCode: (item['kch'] as String?) ?? '',
          courseName: (item['kcmc'] as String?) ?? '',
          credit: parseNum(item['xf'] as String?),
          college: (item['kkbmmc'] as String?) ?? '',
          category: (item['kclbmc'] as String?) ?? '',
          retakeStatus: (item['cxzt'] as String?) ?? '',
          academicYear: (item['cxxnmc'] as String?) ?? '',
          semester: (item['cxxqmc'] as String?) ?? '',
        ),
    ];

/// 空闲教室（cdjy_cxKxcdlb.html）
List<FreeRoom> parseFreeRooms(String body) => [
      for (final item in jqItems(body))
        FreeRoom(
          name: (item['cdmc'] as String?) ?? '',
          code: (item['cdbh'] as String?) ?? '',
          category: (item['cdlbmc'] as String?) ?? '',
          campus: (item['xqmc'] as String?) ?? '',
          building: (item['jxlmc'] as String?) ?? '',
          seats: parseIntSafe(item['zws'] as String?),
        ),
    ];

/// 学业情况（按课程性质分组的 GPA）
List<GpaSummary> parseGpaSummary(String body) => [
      for (final item in jqItems(body))
        GpaSummary(
          natureCode: (item['kcxzdm'] as String?) ?? '',
          natureName: (item['kcxzmc'] as String?) ?? '',
          gpa: parseNum(item['gpa'] as String?),
          earnedCredit: parseNum(item['hdxf'] as String?),
        ),
    ];

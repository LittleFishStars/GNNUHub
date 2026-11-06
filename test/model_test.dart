/// 模型层单测 —— 周次解析、合并、学分宽松解析等
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:gnnuhub/core/model.dart';

ClassTime timeWithWeeks(String weeks) => ClassTime(
      weeks: weeks,
      weekday: '',
      periods: PeriodRange.unknown,
    );

void main() {
  group('Term', () {
    test('xqm 取值是教务系统内部约定', () {
      expect(Term.first.asXqm, 3);
      expect(Term.second.asXqm, 12);
      expect(Term.fromNumber(1), Term.first);
      expect(Term.fromNumber(2), Term.second);
      expect(Term.fromNumber(3), isNull);
    });
  });

  group('PeriodRange', () {
    test('解析容错', () {
      expect(PeriodRange.tryParse('1-2'), const PeriodRange(start: 1, end: 2));
      expect(
          PeriodRange.tryParse(' 3 - 4 '), const PeriodRange(start: 3, end: 4));
      expect(PeriodRange.tryParse('5'), isNull);
      expect(PeriodRange.tryParse('a-b'), isNull);
    });
  });

  group('parseWeekRanges / coversWeek', () {
    test('连续区间', () {
      final t = timeWithWeeks('1-18周');
      expect(t.coversWeek(1), isTrue);
      expect(t.coversWeek(9), isTrue);
      expect(t.coversWeek(18), isTrue);
      expect(t.coversWeek(19), isFalse);
      expect(t.coversWeek(0), isFalse);
    });

    test('单双周修饰', () {
      final odd = timeWithWeeks('7-13周(单)');
      expect(odd.coversWeek(7), isTrue);
      expect(odd.coversWeek(13), isTrue);
      expect(odd.coversWeek(8), isFalse, reason: '双周不应命中(单)区间');

      final even = timeWithWeeks('1-16周(双)');
      expect(even.coversWeek(2), isTrue);
      expect(even.coversWeek(16), isTrue);
      expect(even.coversWeek(1), isFalse, reason: '奇数周不应命中(双)区间');
    });

    test('多段与全角逗号、单周次写法', () {
      final multi = timeWithWeeks('1-8周,10-16周');
      expect(multi.coversWeek(5), isTrue);
      expect(multi.coversWeek(9), isFalse, reason: '空档周不应命中');
      expect(multi.coversWeek(10), isTrue);

      final fullwidth = timeWithWeeks('1-8周，10-16周');
      expect(fullwidth.coversWeek(5), isTrue);
      expect(fullwidth.coversWeek(9), isFalse);

      final single = timeWithWeeks('第17周');
      expect(single.coversWeek(17), isTrue);
      expect(single.coversWeek(16), isFalse);
    });

    test('不可解析时保守命中', () {
      expect(timeWithWeeks('').coversWeek(3), isTrue);
      expect(timeWithWeeks('第?周').coversWeek(3), isTrue);
      expect(timeWithWeeks('1-8周(上机)').coversWeek(3), isTrue);
    });
  });

  group('StudentInfo.mergeFrom', () {
    test('只补齐对方提供的字段', () {
      final base = StudentInfo()
        ..studentId = '250710078'
        ..name = '张三';
      final incoming = StudentInfo()..college = '数学与计算机科学学院';

      base.mergeFrom(incoming);

      expect(base.name, '张三', reason: '原有值应保留');
      expect(base.college, '数学与计算机科学学院', reason: '新字段应被补上');
      expect(base.studentId, '250710078');
    });

    test('空合并不清空任何已有值', () {
      final base = StudentInfo()
        ..studentId = '250710078'
        ..name = '张三'
        ..gender = '男'
        ..document = const DocumentInfo(
            kind: '居民身份证', number: '360101200001011234');
      final snapshot = StudentInfo();
      snapshot.mergeFrom(base);

      base.mergeFrom(StudentInfo());
      expect(base.studentId, snapshot.studentId);
      expect(base.name, snapshot.name);
      expect(base.gender, snapshot.gender);
      expect(base.document, snapshot.document);
    });
  });

  group('CourseEntry.splitList', () {
    test('去空白、丢空片段', () {
      expect(
        CourseEntry.splitList('王老师, 刘老师', ','),
        ['王老师', '刘老师'],
      );
      expect(
        CourseEntry.splitList('计算机2201班;计算机2202班', ';'),
        ['计算机2201班', '计算机2202班'],
      );
      expect(CourseEntry.splitList('', ','), isEmpty);
      expect(CourseEntry.splitList('a,,b,', ','), ['a', 'b']);
      expect(CourseEntry.splitList('  ', ','), isEmpty);
    });
  });

  group('parseCredit', () {
    test('宽松解析', () {
      expect(parseCredit('4.0'), 4.0);
      expect(parseCredit(' 1.5 '), 1.5);
      expect(parseCredit(''), 0.0);
      expect(parseCredit('未知'), 0.0);
    });
  });

  group('ClassSchedule.weekView', () {
    CourseEntry mk(String weeks) => CourseEntry(
          position: '',
          teachers: const [],
          time: ClassTime(
              weeks: weeks,
              weekday: '星期一',
              periods: const PeriodRange(start: 1, end: 2)),
          classes: const [],
          building: '',
          nature: '',
          category: '',
          examMode: '',
          campus: '',
          credit: 0,
        );

    test('按周过滤', () {
      final schedule = ClassSchedule(
        academicTerm: AcademicTerm.first(2025),
        courses: {
          '数学分析': [mk('1-18周')],
          '大学英语': [mk('7-13周(单)')],
          '体育': [mk('3-4周')],
        },
      );

      // 第 7 周：数学分析命中、大学英语(单周)命中、体育已结束
      final view7 = schedule.weekView(7);
      expect(view7.courseCount, 2);
      expect(view7.courses['数学分析']!.length, 1);
      expect(view7.courses['大学英语']!.length, 1);
      expect(view7.courses.containsKey('体育'), isFalse);

      // 第 4 周：数学分析命中、大学英语(单周)不命中、体育进行中
      final view4 = schedule.weekView(4);
      expect(view4.courseCount, 2);
      expect(view4.courses.containsKey('大学英语'), isFalse);
      expect(view4.courses['体育']!.length, 1);

      expect(view7.academicTerm, AcademicTerm.first(2025));
    });
  });

  group('GpaSummary', () {
    test('学分加权总 GPA', () {
      final items = [
        const GpaSummary(
            natureCode: '01', natureName: '必修课', gpa: 3.0, earnedCredit: 2),
        const GpaSummary(
            natureCode: '02', natureName: '选修课', gpa: 2.86, earnedCredit: 49.5),
      ];
      expect(GpaSummary.overallGpa(items), closeTo(2.8656, 0.001));
      expect(GpaSummary.totalCredit(items), closeTo(51.5, 0.0001));
      expect(GpaSummary.overallGpa(const []), 0);
    });
  });

  group('AcademicCalendar.currentWeek', () {
    test('起始日当天=第 1 周', () {
      final cal = AcademicCalendar(
          year: 2026, term: 1, startDate: '2026-08-31', endDate: '2027-01-17');
      expect(cal.currentWeek(DateTime(2026, 8, 31)), 1);
      expect(cal.currentWeek(DateTime(2026, 9, 8)), 2);
      expect(cal.currentWeek(DateTime(2026, 8, 30)), isNull);
    });
  });
}

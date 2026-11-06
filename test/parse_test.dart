/// 解析层单测 —— 通知、系统提醒、校历、课表、Cookie 等
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:gnnuhub/api/cookies.dart';
import 'package:gnnuhub/api/parse/lists.dart';
import 'package:gnnuhub/api/parse/portal.dart';
import 'package:gnnuhub/api/parse/schedule.dart';
import 'package:gnnuhub/core/error.dart';
import 'package:gnnuhub/core/model.dart';

void main() {
  group('parseSetCookie', () {
    test('基本解析', () {
      final (name, value) =
          parseSetCookie('JSESSIONID=ABC123; Path=/; HttpOnly')!;
      expect(name, 'JSESSIONID');
      expect(value, 'ABC123');
    });

    test('DELETED 的 Cookie 应被丢弃', () {
      expect(parseSetCookie('JSESSIONID=DELETED; Path=/sso'), isNull);
    });

    test('deleteMe 必须丢弃（大小写不敏感，但包含词不误伤）', () {
      expect(parseSetCookie('rememberMe=deleteMe; Path=/; Max-Age=0'), isNull);
      expect(parseSetCookie('rememberMe=DELETEME; Path=/'), isNull);
      expect(parseSetCookie('rememberMe=deleteMeLater; Path=/'), isNotNull);
    });

    test('缺少等号的 Cookie 应被忽略', () {
      expect(parseSetCookie('justtext'), isNull);
    });
  });

  group('isSsoScoped', () {
    test('/sso 作用域应被识别', () {
      expect(isSsoScoped('JSESSIONID=ABC; Path=/sso; HttpOnly'), isTrue);
      expect(isSsoScoped('JSESSIONID=ABC; path=/sso/lyiotlogin'), isTrue);
      expect(isSsoScoped('JSESSIONID=ABC; Path=/sso5'), isTrue);
    });

    test('根作用域不应误判', () {
      expect(isSsoScoped('JSESSIONID=ABC; Path=/; HttpOnly'), isFalse);
      expect(isSsoScoped('JSESSIONID=ABC'), isFalse);
      expect(isSsoScoped('SF_cookie_17=10086038; Path=/'), isFalse);
    });
  });

  group('parseNotices', () {
    const html = '''
<div class="list-group">
<a title="【学生】教务系统使用说明" href="/xtgl/xwck_ckXw.html?xwbh=AAA111&doType=save" class="list-group-item">
  <span class="title xwxx"><font color="red">【置顶】</font><font color="black">【通知】</font>【学生】教务系统使用说明</span>
  <span class="time">2020-12-31</span>
</a>
<a title="" href="/xtgl/xwck_ckXw.html?xwbh=B222" class="list-group-item">
  <span class="title xwxx">空标题条目</span>
</a>
</div>''';

    test('标题取 a 的 title 属性，置顶与栏目提取正确', () {
      final notices = parseNotices(html);
      expect(notices.length, 1, reason: '空 title 条目应丢弃');
      final n = notices.first;
      expect(n.title, '【学生】教务系统使用说明');
      expect(n.date, '2020-12-31');
      expect(n.pinned, isTrue);
      expect(n.category, '【通知】');
      expect(n.detailPath, '/xtgl/xwck_ckXw.html?xwbh=AAA111&doType=save');
    });
  });

  group('parseSystemMessages', () {
    test('JSON 字符串按 | 分隔、去尾部空段', () {
      final messages = parseSystemMessages('"你可能有应选未选的主修课程，请核实！|"');
      expect(messages, ['你可能有应选未选的主修课程，请核实！']);
    });

    test('空字符串 → 空列表', () {
      expect(parseSystemMessages('""'), isEmpty);
    });
  });

  group('parseNoticeDetail', () {
    test('发布信息全角冒号、正文清洗与图片标注', () {
      const html = '''
<html><body>
<h3 class="text-center">关于期末考试安排的通知</h3>
<h5 class="news_title1 text-center">
  <span>发布人：教务处</span>
  <span>发布时间：2026-06-01 09:00:00</span>
  <span>浏览人数：123</span>
</h5>
<div class="news_con">
  <p>各位同学：</p>
  <p>期末考试   安排已发布，</p>
  <img src="/img/exam.png">
  <p>请按时参加。</p>
</div>
</body></html>''';
      final detail = parseNoticeDetail(html);
      expect(detail.title, '关于期末考试安排的通知');
      expect(detail.publisher, '教务处');
      expect(detail.publishTime, '2026-06-01 09:00:00');
      expect(detail.views, '123');
      expect(detail.content, contains('图片: /img/exam.png'));
      expect(detail.content.split('\n').length, 3, reason: '图片标注与相邻文本同段');
    });

    test('缺少 news_con 硬报错', () {
      expect(
        () => parseNoticeDetail('<h3 class="text-center">x</h3>'),
        throwsA(isA<PageParseError>()),
      );
    });
  });

  group('parseCalendar', () {
    test('标准模式提取', () {
      const html = '<div>2026-2027学年1学期(2026-08-31至2027-01-17)</div>';
      final cal = parseCalendar(html);
      expect(cal.year, 2026);
      expect(cal.term, 1);
      expect(cal.startDate, '2026-08-31');
      expect(cal.endDate, '2027-01-17');
    });

    test('定位失败报错', () {
      expect(
        () => parseCalendar('<div>没有学期信息</div>'),
        throwsA(isA<PageParseError>()),
      );
    });
  });

  group('parseClassSchedule', () {
    test('字面量 null → 空课表', () {
      final (schedule, info) = parseClassSchedule('null');
      expect(schedule.courseCount, 0);
      expect(info.name, isNull);
    });

    test('kbList 解析 + 学生简讯', () {
      const body = '''
{"xsxx":{"XM":"张三","ZYMC":"数学学院","BJMC":"数学2201","JSXM":"李老师"},
 "kbList":[{"kcmc":"体育","jcs":"?","xf":"1","cdmc":"操场","xm":"王老师",
   "zcd":"1-16周","xqjmc":"星期一","jxbzc":"体育1班;体育2班","lh":"体育馆",
   "kcxz":"必修","kclb":"公共课","khfsmc":"考查","xqmc":"章贡校区"}]}''';
      final (schedule, info) = parseClassSchedule(body);
      expect(info.name, '张三');
      expect(info.college, '数学学院', reason: 'ZYMC 原实现映射到 college');
      expect(info.className, '数学2201');
      expect(info.instructor, '李老师');
      expect(schedule.courseCount, 1);
      final entry = schedule.courses['体育']!.first;
      expect(entry.time.periods, PeriodRange.unknown, reason: '"?" 退化为 0 节');
      expect(entry.credit, 1.0);
      expect(entry.teachers, ['王老师']);
      expect(entry.classes, ['体育1班', '体育2班']);
    });

    test('kbList 为空回落 kbListnew', () {
      const body =
          '{"kbList":[],"kbListnew":[{"kcmc":"高数","jcs":"1-2","xf":"4"}]}';
      final (schedule, _) = parseClassSchedule(body);
      expect(schedule.courseCount, 1);
    });
  });

  group('parseTimetable', () {
    test('裸 JSON 数组', () {
      final times = parseTimetable('[{"qssj":"08:00","jssj":"08:45"}]');
      expect(times.length, 1);
      expect(times.first.start, '08:00');
      expect(times.first.end, '08:45');
    });
  });

  group('parseGrades', () {
    test('bfzcj 缺失 → null；passed 白名单', () {
      const body = '''
{"items":[{"xnmmc":"2025-2026","xqmmc":"1","kch":"B1200010","kcmc":"数学分析Ⅰ",
  "kcxzmc":"必修","xf":"4.0","cj":"优秀","jd":"3.9","xfjd":"15.6"},
 {"xnmmc":"2025-2026","xqmmc":"1","kch":"B1200020","kcmc":"大学英语",
  "kcxzmc":"必修","xf":"3.0","cj":"60","bfzcj":45,"jd":"1.0","xfjd":"3.0"}]}''';
      final grades = parseGrades(body);
      expect(grades.length, 2);
      expect(grades[0].rawScore, isNull);
      expect(grades[0].passed, isTrue, reason: '"优秀" 在白名单');
      // 高挂重修：cj 显示及格但原始分不及格 —— 必须判未通过
      expect(grades[1].passed, isFalse);
    });

    test('"不合格" 不被 "合格" 误匹配', () {
      const body =
          '{"items":[{"kcmc":"体育","cj":"不合格","xf":"1"}]}';
      expect(parseGrades(body).first.passed, isFalse);
    });
  });

  group('parseExams', () {
    test('字段映射与时长解析', () {
      const body = '''
{"items":[{"kch":"B1200010","kcmc":"数学分析Ⅰ","ksmc":"2025-2026-2学期期末考试",
 "kssj":"2026-07-07(14:30-16:30)","cdmc":"2-309","cdxqmc":"章贡校区",
 "ksfs":"笔试","khfs":"考试","kkxy":"数学学院","jsxx":"1300028/某老师",
 "cxbj":"否","jxbmc":"数学2201","xf":"4.0","xnmc":"2025-2026","xqmmc":"2",
 "sksj":"星期一第7-8节{1-3周,5-17周}","sjsj":"120"}]}''';
      final exams = parseExams(body);
      expect(exams.length, 1);
      final exam = exams.first;
      expect(exam.examTime, '2026-07-07(14:30-16:30)');
      expect(exam.teacher, '1300028/某老师');
      expect(exam.durationMinutes, 120);
      expect(exam.isMakeUp, isFalse);
      expect(exam.academicYear, '2025-2026');
    });

    test('sjsj 非法 → durationMinutes 为 null', () {
      const body =
          '{"items":[{"kcmc":"体育","sjsj":"未知","xf":"1","xnmc":"2025-2026","xqmmc":"2"}]}';
      expect(parseExams(body).first.durationMinutes, isNull);
    });
  });

  group('parseGpaSummary', () {
    test('宽松解析', () {
      const body =
          '{"items":[{"kcxzdm":"01","kcxzmc":"必修课","gpa":"3.00","hdxf":"2"},'
          '{"kcxzdm":"02","kcxzmc":"选修课","gpa":"2.86","hdxf":"49.5"}]}';
      final gpa = parseGpaSummary(body);
      expect(gpa.length, 2);
      expect(gpa[0].gpa, 3.0);
      expect(gpa[1].earnedCredit, 49.5);
    });
  });
}

/// 已认证会话与业务接口 —— 对应 Rust `logic/api/src/session.rs`
///
/// 持有登录后的 Cookie，并在此基础上提供课表、学籍等查询。
/// 所有接口命中缓存时不产生网络请求（缓存模式与原实现一致，
/// 包括 timetable 全局单份、free_rooms 缓存键不含 lh/cdlb_id 等行为）。
library;

import '../core/constants.dart';
import '../core/model.dart';
import 'client.dart';
import 'cookies.dart';
import 'parse/lists.dart';
import 'parse/portal.dart';
import 'parse/schedule.dart';
import 'parse/student.dart';

/// 已认证的教务系统会话
class HubSession {
  HubSession(this.client, Map<String, String> cookies, this.studentId)
      : cookies = Map<String, String>.unmodifiable(cookies);

  final HubClient client;

  /// 登录后获得的 Cookie（构造后只读，业务请求不更新）
  final Map<String, String> cookies;

  /// 登录学号
  final String studentId;

  // -------------------------------------------------------------------------
  // 缓存状态
  // -------------------------------------------------------------------------
  final StudentInfo _info = StudentInfo();
  final Map<AcademicTerm, ClassSchedule> _schedules = {};
  final Map<AcademicTerm, List<ExamRecord>> _exams = {};
  final Map<AcademicTerm, List<RetakeCourse>> _retakes = {};
  List<GpaSummary>? _gpa;
  AcademicCalendar? _calendar;
  final Map<String, List<FreeRoom>> _freeRooms = {};
  List<PeriodTime>? _timetable; // 全局单份，不分学期
  int? _thisWeek;

  String get _cookieHeader => buildCookieHeader(cookies);

  // -------------------------------------------------------------------------
  // 基础请求
  // -------------------------------------------------------------------------

  /// 把路径绝对化（http(s) 原样；否则拼教务域名，**不做编码**）
  String _absolute(String path) => path.startsWith('http://') ||
          path.startsWith('https://')
      ? path
      : '$jwglBaseUrl$path';

  /// GET（query + Cookie）
  Future<String> _get(String path, {Map<String, String> query = const {}}) {
    var url = _absolute(path);
    if (query.isNotEmpty) url = '$url?${encodeForm(query)}';
    return client.get(url, headers: {'Cookie': _cookieHeader});
  }

  /// POST（query + urlencoded form + Cookie）
  Future<String> _post(
    String path, {
    Map<String, String> query = const {},
    required Map<String, String> form,
  }) {
    return client.postForm(
      _absolute(path),
      query: query,
      form: form,
      headers: {'Cookie': _cookieHeader},
    );
  }

  /// 通用缓存包装
  Future<T> _cached<T>(T? Function() read, Future<T> Function() fetch,
      void Function(T) write) async {
    final existing = read();
    if (existing != null) return existing;
    final value = await fetch();
    write(value);
    return value;
  }

  // -------------------------------------------------------------------------
  // 学籍信息
  // -------------------------------------------------------------------------

  /// 首页个人信息页（无缓存）
  Future<StudentInfo> fetchBasicInfo() async {
    final html = await _get('/xtgl/index_cxYhxxIndex.html', query: {
      'xt': 'jw',
      'localeKey': 'zh_CN',
    });
    final parsed = parseBasicInfo(html);
    _info.mergeFrom(parsed);
    parsed.studentId = studentId; // 首页不带学号，补齐
    return parsed;
  }

  /// 学籍详情（JSON 接口优先，失败回退 HTML 页面）
  Future<StudentInfo> fetchStudentInfo() async {
    try {
      final body = await _post(
        '/xsxxxggl/xsxxwh_cxCkDgxsxx.html',
        query: {'gnmkdm': 'N100801'},
        form: {'xh_id': studentId, 'fromXh_id': ''},
      );
      final parsed = parseStudentProfileJson(body);
      _info.mergeFrom(parsed);
      return parsed;
    } catch (_) {
      final html = await _get('/xsxxxggl/xsgrxxwh_cxXsgrxx.html', query: {
        'gnmkdm': 'N100801',
        'layout': 'default',
      });
      final parsed = parseStudentInfo(html);
      _info.mergeFrom(parsed);
      return parsed;
    }
  }

  /// 合并后的完整学籍信息
  ///
  /// 先首页 → 再详情（失败静默兜底为空信息）→ 强制学号为登录学号。
  Future<StudentInfo> studentInfo() async {
    if (_info.name != null && _info.major != null) {
      return _cloneInfo(_info);
    }
    final basic = await fetchBasicInfo();
    StudentInfo detail;
    try {
      detail = await fetchStudentInfo();
    } catch (_) {
      detail = StudentInfo();
    }
    _info.mergeFrom(basic);
    _info.mergeFrom(detail);
    _info.studentId = studentId;
    return _cloneInfo(_info);
  }

  static StudentInfo _cloneInfo(StudentInfo src) {
    final copy = StudentInfo();
    copy.mergeFrom(src);
    return copy;
  }

  // -------------------------------------------------------------------------
  // 课表
  // -------------------------------------------------------------------------

  /// 完整学期课表（只认 POST）
  Future<ClassSchedule> _fetchFullSchedule(AcademicTerm term) async {
    final body = await _post(
      '/kbcx/xskbcx_cxXsgrkb.html',
      query: {'gnmkdm': 'N2151'},
      form: {
        'xnm': term.asXnm,
        'xqm': term.asXqm.toString(),
        'kzlx': 'ck',
        'xsdm': '',
        'kclbdm': '',
      },
    );
    final (schedule, info) = parseClassSchedule(body);
    final wrapped = ClassSchedule(
        academicTerm: term, courses: schedule.courses);
    _schedules[term] = wrapped;
    _info.mergeFrom(info);
    return wrapped;
  }

  /// 获取课表；[week] 非空时返回该周视图（纯本地过滤，0 次额外请求）
  Future<ClassSchedule> classSchedule(AcademicTerm term, {int? week}) async {
    var full = _schedules[term];
    full ??= await _fetchFullSchedule(term);
    return week == null ? full : full.weekView(week);
  }

  /// 指定课程的全部排课
  Future<List<CourseEntry>> course(AcademicTerm term, String name) async {
    final schedule = await classSchedule(term);
    return schedule.courses[name] ?? const [];
  }

  /// 当前教学周（从教学周页 `#zs 下拉框`）
  Future<int> thisWeek() => _cached<int>(
        () => _thisWeek,
        () async {
          final html = await _get(
              '/kbcx/xskbcxZccx_cxXskbcxIndex.html',
              query: {'gnmkdm': 'N2154', 'layout': 'default'});
          return parseThisWeek(html);
        },
        (v) => _thisWeek = v,
      );

  /// 当前教学周的课表
  Future<ClassSchedule> currentWeekSchedule(AcademicTerm term) async {
    final week = await thisWeek();
    return classSchedule(term, week: week);
  }

  /// 节次时间表（缓存为全局单份，换学期不重新请求 —— 与原实现一致）
  Future<List<PeriodTime>> timetable(AcademicTerm term) =>
      _cached<List<PeriodTime>>(
        () => _timetable,
        () async {
          final body = await _post(
            '/kbcx/xskbcx_cxRjc.html',
            query: {'gnmkdm': 'N2151'},
            form: {
              'xnm': term.asXnm,
              'xqm': term.asXqm.toString(),
              'xqh_id': '1',
            },
          );
          return parseTimetable(body);
        },
        (v) => _timetable = v,
      );

  // -------------------------------------------------------------------------
  // 考试 / 重修 / GPA
  // -------------------------------------------------------------------------

  /// 考试安排（服务端不按学期过滤，term 仅作缓存分桶键）
  Future<List<ExamRecord>> examSchedule(AcademicTerm term) =>
      _cached<List<ExamRecord>>(
        () => _exams[term],
        () async {
          final body = await _post(
            '/kwgl/kscx_cxXsksxxIndex.html',
            query: {'gnmkdm': 'N358105', 'doType': 'query'},
            form: {
              'xnm': term.asXnm,
              'xqm': term.asXqm.toString(),
              'ksmcdmb_id': '',
              'kch': '',
              'kc': '',
              'ksrq': '',
              'kkbm_id': '',
              'zd_fzdm': 'N358105-xs',
              'queryModel.showCount': '100',
            },
          );
          return parseExams(body);
        },
        (v) => _exams[term] = v,
      );

  /// 重修课程库（服务端不按学期过滤，返回全部）
  Future<List<RetakeCourse>> retakeCourses(AcademicTerm term) =>
      _cached<List<RetakeCourse>>(
        () => _retakes[term],
        () async {
          final body = await _post(
            '/cxkccx/cxkccx_cxCxkccxIndex.html',
            query: {'gnmkdm': 'N255720', 'doType': 'query'},
            form: {
              'cxxnm': term.asXnm,
              'cxxqm': term.asXqm.toString(),
              'kkbm_id': '',
              'kclbdm': '',
              'bxqsfkk_cx': '',
              'kkxb_id': '',
              'kch': '',
            },
          );
          return parseRetakeCourses(body);
        },
        (v) => _retakes[term] = v,
      );

  /// 学业情况（按课程性质分组的 GPA；全量加权 GPA 用
  /// [GpaSummary.overallGpa] 计算）
  Future<List<GpaSummary>> gpaSummary() => _cached<List<GpaSummary>>(
        () => _gpa,
        () async {
          final body = await _post(
            '/xsxy/xsxyqk_cxKczxAllIndex.html',
            query: {'gnmkdm': 'N105515', 'doType': 'query'},
            form: {'xh_id': ''},
          );
          return parseGpaSummary(body);
        },
        (v) => _gpa = v,
      );

  // -------------------------------------------------------------------------
  // 空闲教室
  // -------------------------------------------------------------------------

  /// 空闲教室（场地）列表
  ///
  /// 缓存键**不含 lh 与 cdlb_id**（与原实现一致）。
  /// qssj/jssj 为 `YYYY-MM-DD`，qssd/jssd 为 `HH:mm:00`，
  /// 时间字段缺失会触发服务端异常页。
  Future<List<FreeRoom>> freeRooms({
    required AcademicTerm term,
    required String qssj,
    required String jssj,
    required String qssd,
    required String jssd,
    required String lh,
    required String cdlbId,
  }) async {
    final termKey = '${term.year}-${term.term == Term.first ? 'First' : 'Second'}';
    final key = '$termKey/$qssj/$jssj/$qssd/$jssd';
    return _cached<List<FreeRoom>>(
      () => _freeRooms[key],
      () async {
        final body = await _post(
          '/cdjy/cdjy_cxKxcdlb.html',
          query: {'gnmkdm': 'N2155', 'doType': 'query'},
          form: {
            'xnm': term.asXnm,
            'xqm': term.asXqm.toString(),
            'cdyysqkz': '0',
            'glrycdyysfxysp': '1',
            'xqFlag': '0',
            'syr': studentId,
            'syrxm': '',
            'yuyjsctbj': '1',
            'dm': '',
            'xqh_id': '',
            'lh': lh,
            'cdlb_id': cdlbId,
            'cdejlb_id': '',
            'jyfs': '1',
            'qssj': qssj,
            'jssj': jssj,
            'qssd': qssd,
            'jssd': jssd,
            'zd_fzdm': 'N211205-kxcdlb',
            'queryModel.showCount': '100',
            'queryModel.currentPage': '1',
          },
        );
        return parseFreeRooms(body);
      },
      (v) => _freeRooms[key] = v,
    );
  }

  // -------------------------------------------------------------------------
  // 门户：通知、系统提醒、校历
  // -------------------------------------------------------------------------

  /// 首页通知公告列表（无缓存）
  Future<List<Notice>> notices() async {
    final html = await _get('/xtgl/index_cxNews.html',
        query: {'localeKey': 'zh_CN'});
    return parseNotices(html);
  }

  /// 系统提醒消息（无缓存）
  Future<List<String>> systemMessages() async {
    final body = await _get('/xtgl/index_cxXttsxx.html', query: {'bj': '2'});
    return parseSystemMessages(body);
  }

  /// 通知详情正文（无缓存，path 原样拼接）
  Future<NoticeDetail> noticeContent(String detailPath) async {
    final html = await _get(detailPath);
    return parseNoticeDetail(html);
  }

  /// 校历（全局单份缓存）
  Future<AcademicCalendar> academicCalendar() =>
      _cached<AcademicCalendar>(
        () => _calendar,
        () async {
          final html = await _get('/xtgl/index_cxAreaFive.html',
              query: {'localeKey': 'zh_CN'});
          return parseCalendar(html);
        },
        (v) => _calendar = v,
      );

  /// 原始 GET（无 parse、无缓存；供离线分析页面/JS 用）
  Future<String> fetchRaw(String path) => _get(path);
}

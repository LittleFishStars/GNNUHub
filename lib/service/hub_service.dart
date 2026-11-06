/// UI 无关的应用服务层 —— 对应 Rust `logic/service/src/lib.rs`
///
/// 各 UI 实现只应依赖本层（加上 [core/model] 的领域模型），
/// 不要绕过它直接调用 api 层——否则会话状态、登录流程与错误分类
/// 就会在每个 UI 里各自为政。
///
/// `retrySafe(HubError)` 提供统一的「重试是否有害」判据，
/// 特别是**账号锁定时重试会加重锁定**这一硬约束。
library;

import 'package:flutter/foundation.dart';

import '../api/client.dart';
import '../api/login.dart';
import '../api/session.dart';
import '../api/ticket.dart';
import '../core/constants.dart';
import '../core/error.dart';
import '../core/model.dart';
import '../ocr/bitmap_ocr.dart';
import '../ocr/library.dart';

/// 应用服务：持有登录会话并提供用例级操作
///
/// 内部只有锁化的可空会话；直接放进自己的状态管理即可。
class HubService extends ChangeNotifier {
  HubService({ClientConfig? config, BitmapLibrary? library})
      : _config = config ?? const ClientConfig(),
        _libraryFuture = library == null
            ? BitmapLibrary.fromAsset()
            : Future.value(library);

  final ClientConfig _config;
  final Future<BitmapLibrary> _libraryFuture;

  HubClient? _client;
  HubSession? _session;
  BitmapOcr? _ocr;

  /// 当前登录的学号（未登录返回 null）
  String? get loggedInAs => _session?.studentId;

  bool get isLoggedIn => _session != null;

  /// 用学号 + 密码登录，成功后返回学号
  ///
  /// 验证码由内嵌位图字库自动识别，识别错误自动重试（上限
  /// [ClientConfig.maxLoginRetries]）。密码只在本调用内使用，
  /// 不落盘、不进日志。
  Future<String> login(String studentId, String password) async {
    final trimmed = studentId.trim();
    final id = int.tryParse(trimmed);
    if (id == null) {
      throw const InvalidStudentIdError();
    }

    final library = await _libraryFuture;
    final ocr = _ocr ??= BitmapOcr(library);
    final client = _client ??= HubClient(_config);

    final service = '$jwglBaseUrl/';
    final outcome =
        await loginWithRetry(client, trimmed, password, service, ocr);

    switch (outcome) {
      case LoginSuccess(:final tgt, :final ticket):
        final cookies =
            await exchangeTicketForSession(client, ticket, tgt, 5);
        _session = HubSession(client, cookies, trimmed);
        notifyListeners();
        return trimmed;
      case BadCredentials():
        throw const InvalidCredentialsError();
      case CaptchaIncorrect():
        throw const InvalidCaptchaError();
      case AccountLocked(:final detail):
        throw TicketMissingError('账号已锁定: $detail');
    }
  }

  /// 退出登录（丢弃会话；幂等）
  void logout() {
    _session = null;
    notifyListeners();
  }

  /// 取出当前会话（未登录抛 [NotLoggedInError]）
  HubSession _requireSession() {
    final session = _session;
    if (session == null) throw const NotLoggedInError();
    return session;
  }

  /// 获取合并后的完整学籍信息（内部有缓存与回退策略）
  Future<StudentInfo> studentInfo() => _requireSession().studentInfo();

  /// 获取课表；[week] 非空返回该周视图
  Future<ClassSchedule> classSchedule(AcademicTerm term, {int? week}) =>
      _requireSession().classSchedule(term, week: week);

  /// 获取当前教学周
  Future<int> thisWeek() => _requireSession().thisWeek();

  /// 拉取空闲教室列表
  Future<List<FreeRoom>> freeRooms({
    required AcademicTerm term,
    required String qssj,
    required String jssj,
    required String qssd,
    required String jssd,
    required String lh,
    required String cdlbId,
  }) =>
      _requireSession().freeRooms(
        term: term,
        qssj: qssj,
        jssj: jssj,
        qssd: qssd,
        jssd: jssd,
        lh: lh,
        cdlbId: cdlbId,
      );

  /// 拉取节次时间表
  Future<List<PeriodTime>> timetable(AcademicTerm term) =>
      _requireSession().timetable(term);

  /// 拉取通知详情正文
  Future<NoticeDetail> noticeContent(String detailPath) =>
      _requireSession().noticeContent(detailPath);

  /// 拉取校历
  Future<AcademicCalendar> academicCalendar() =>
      _requireSession().academicCalendar();

  /// 拉取学业情况（按课程性质分组的 GPA）
  Future<List<GpaSummary>> gpaSummary() => _requireSession().gpaSummary();

  /// 拉取首页通知公告列表
  Future<List<Notice>> notices() => _requireSession().notices();

  /// 拉取系统提醒消息
  Future<List<String>> systemMessages() => _requireSession().systemMessages();

  /// 拉取重修课程库
  Future<List<RetakeCourse>> retakeCourses(AcademicTerm term) =>
      _requireSession().retakeCourses(term);

  /// 获取考试安排
  Future<List<ExamRecord>> examSchedule(AcademicTerm term) =>
      _requireSession().examSchedule(term);
}

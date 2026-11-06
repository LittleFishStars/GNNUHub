/// 统一身份认证登录流程 —— 对应 Rust `logic/api/src/login/mod.rs` + `sso.rs`
///
/// 完整链路（抓包实测共 3 跳）：
/// ```
/// ① GET /sso/lyiotlogin?ticket=ST-xxx  （裸请求，不带 Cookie）
///      → 302 /sso/lyiotlogin             Set-Cookie: JSESSIONID=<A> /sso
/// ② GET /sso/lyiotlogin                （带 <A>）
///      → 302 /ticketlogin?uid=...&verify=...
/// ③ GET /ticketlogin?...                （带 <A>）
///      → 302 /xtgl/login_slogin.html    Set-Cookie: JSESSIONID=<B> Path=/
/// ```
/// ③ 之后必须停止：请求 login_slogin 会被服务端直接关闭 TLS 连接。
library;

import 'package:dio/dio.dart';

import '../core/constants.dart';
import '../core/error.dart';
import '../crypto/rsa.dart';
import '../ocr/bitmap_ocr.dart';
import 'captcha.dart';
import 'client.dart';
import 'cookies.dart';
import 'ticket.dart';

/// 票据接口路径
const String _ticketsPath = '/lyuapServer/v1/tickets';

/// 教务系统 SSO 登录入口
const String _ssoLoginPath = '/sso/lyiotlogin';

/// 教务系统登录页路径（**不要请求它**）
const String _loginPagePath = '/xtgl/login_slogin.html';

/// 单跳最多尝试次数（仅对传输层错误重试）
const int _maxHopAttempts = 3;

/// 重试前等待（明显大于请求间隔，避免撞在同一个风控窗口）
const Duration _retryDelay = Duration(milliseconds: 1200);

/// 向票据接口提交认证请求
Future<LoginOutcome> tryLogin(
  HubClient client,
  String studentId,
  String encryptedPassword,
  String captchaCode,
  String captchaUid,
  String service,
) async {
  final url = '$casBaseUrl$_ticketsPath';
  final form = <String, String>{
    'username': studentId,
    'password': encryptedPassword,
    'service': service,
    'code': captchaCode,
    'id': captchaUid, // 字段名是 id，不是 uid
    'loginType': '',
    'otpcode': '',
  };
  final body = await client.postForm(
    url,
    form: form,
    headers: casHeaders(),
  );
  return parseTicketResponse(body);
}

/// 申请一张新验证码
Future<Captcha> fetchCaptcha(HubClient client) async {
  final url = Captcha.requestUrl(Captcha.randomRequestId());
  final body = await client.get(url, headers: casHeaders());
  return Captcha.parse(body);
}

/// 执行完整的带重试登录流程
///
/// 与 Python 版的差异：重试次数由配置决定，不再无限递归。
/// OCR 未命中字库是本地问题（字库缺条目），**直接上抛**不消耗重试；
/// 真正需要重试的是服务端判定「验证码错误」。
Future<LoginOutcome> loginWithRetry(
  HubClient client,
  String studentId,
  String password,
  String service,
  OcrEngine ocr,
) async {
  final maxAttempts = client.config.maxLoginRetries;
  final encryptedPassword = encodePassword(password); // 循环外只加密一次

  for (var attempt = 1; attempt <= maxAttempts; attempt++) {
    final captcha = await fetchCaptcha(client);
    final code = ocr.recognize(captcha.image);
    final outcome = await tryLogin(
      client,
      studentId,
      encryptedPassword,
      code,
      captcha.uid,
      service,
    );
    switch (outcome) {
      case LoginSuccess():
        return outcome;
      case CaptchaIncorrect():
        continue;
      case BadCredentials():
      case AccountLocked():
        // 密码错误 / 账号锁定都立即停止，重试无意义或有害
        return outcome;
    }
  }
  throw LoginRetriesExhaustedError(maxAttempts);
}

/// 用 ticket 换取教务系统会话 Cookie
///
/// 第 1 跳必须携带 ticket、不带任何 Cookie；
/// 最终必须拿到**根作用域** `JSESSIONID`（`/sso` 那份是中转产物）。
Future<Map<String, String>> exchangeTicketForSession(
  HubClient client,
  String ticket,
  String tgt,
  int maxHops,
) async {
  final cookies = <String, String>{};
  String? ssoJsessionId;

  var url = '$jwglBaseUrl$_ssoLoginPath?ticket=$ticket'; // ticket 未编码
  var hops = 0;
  final hopLimit = maxHops < 5 ? 5 : maxHops; // 实测 3 跳即可拿到有效会话

  while (true) {
    hops++;
    if (hops > hopLimit) {
      throw LoginRetriesExhaustedError(hopLimit);
    }

    // 第 1 跳裸请求；之后带上链路累积的 Cookie
    final carry = hops == 1
        ? <String, String>{}
        : Map<String, String>.from(cookies);

    final response = await _sendHopWithRetry(client, url, carry);
    final status = response.statusCode ?? 0;

    // 收集所有 Set-Cookie；`/sso` 作用域的值记录下来供链路结束后剔除
    final setCookies = <String>[];
    response.headers.map.forEach((name, values) {
      if (name.toLowerCase() == 'set-cookie') setCookies.addAll(values);
    });
    for (final text in setCookies) {
      final parsed = parseSetCookie(text);
      if (parsed == null) continue;
      if (isSsoScoped(text)) ssoJsessionId = parsed.$2;
      cookies[parsed.$1] = parsed.$2;
    }

    // 非重定向说明链路已结束
    if (status < 300 || status > 399) break;

    final location = response.headers.value('location');
    if (location == null) break;

    final next = location.startsWith('http')
        ? location
        : '$jwglBaseUrl$location';

    // ⚠️ 关键：拿到根作用域 JSESSIONID 后即停止
    if (next.contains(_loginPagePath)) break;

    final uri = Uri.tryParse(next);
    if (uri == null) {
      throw SsoRedirectError('非法的跳转地址 $next');
    }
    if (uri.host == casHost) {
      throw const SsoRedirectError('ticket 未被教务系统接受，跳回了认证平台');
    }
    if (uri.host != jwglHost) {
      throw SsoRedirectError('跳转目标域不在教务系统内: $next');
    }
    url = next;
  }

  // 会话是否成立，取决于是否拿到了根作用域的 JSESSIONID。
  // Rust 实现在「最终值仍是 /sso 那份」时只 warn、不撤销已装好的会话，
  // 这里保持一致（以代码为准，注释里的「据实报错」并未实现）。
  if (ssoJsessionId != null && cookies['JSESSIONID'] == ssoJsessionId) {
    // 只拿到 /sso 作用域的值，链路没走完；保留现状交给后续请求验证
  }
  if (!cookies.containsKey('JSESSIONID')) {
    throw const UnauthenticatedError();
  }
  return cookies;
}

/// 发起一次 SSO 跳转请求，对**传输层瞬时故障**做有限重试
///
/// HTTP 状态码层面的失败不重试（服务端对异常请求会主动断开连接，
/// 盲目重试只会加深风控）。
Future<Response<String>> _sendHopWithRetry(
  HubClient client,
  String url,
  Map<String, String> cookies,
) async {
  final header = buildCookieHeader(cookies);
  Object? lastError;
  for (var attempt = 1; attempt <= _maxHopAttempts; attempt++) {
    try {
      // rawGet 内部已走节流锁（synchronized Lock 不可重入，勿再套一层）
      return await client.rawGet(url, header);
    } catch (e) {
      lastError = e;
      if (attempt < _maxHopAttempts) {
        await Future<void>.delayed(_retryDelay);
      }
    }
  }
  throw NetworkError('SSO 跳转请求失败: $lastError');
}


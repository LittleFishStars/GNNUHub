/// HTTP 客户端构造与配置 —— 对应 Rust `logic/api/src/client.rs`
///
/// 关键约束：
/// - 手动管理重定向（SSO 每跳都要检查目标域）：`followRedirects=false`
/// - 不启用 Cookie 自动管理：全部 Cookie 显式注入 `Cookie` 头，
///   避免同名 JSESSIONID 叠加导致服务端断开连接
/// - 业务请求只带 Cookie 头；CAS 请求用 [casHeaders]（含 Host 覆盖）
library;

import 'package:dio/dio.dart';
import 'package:synchronized/synchronized.dart';

import '../core/constants.dart';
import '../core/error.dart';
import '../crypto/rsa.dart';

/// 默认请求超时（秒）
const int defaultTimeoutSecs = 30;

/// 相邻请求最小间隔（WAF 礼仪，勿关闭）
const Duration defaultRequestInterval = Duration(milliseconds: 600);

/// 客户端配置
class ClientConfig {
  const ClientConfig({
    this.timeout = const Duration(seconds: defaultTimeoutSecs),
    this.maxLoginRetries = 5,
    this.requestInterval = defaultRequestInterval,
  });

  final Duration timeout;

  /// 登录流程最多重试次数（含首次尝试之后的重试；下限 1）
  final int maxLoginRetries;

  /// 相邻两次请求之间的最小间隔（Duration.zero 可关闭）
  final Duration requestInterval;

  ClientConfig withMaxLoginRetries(int retries) => ClientConfig(
        timeout: timeout,
        maxLoginRetries: retries < 1 ? 1 : retries,
        requestInterval: requestInterval,
      );
}

/// CAS 请求头（含 Host 覆盖，只能用于 cas.gnnu.edu.cn）
///
/// 复用到教务等其他域名会造成 Host 与 SNI 不匹配，服务端直接关闭连接。
Map<String, String> casHeaders({String host = casHost}) => {
      'X-Requested-With': 'XMLHttpRequest',
      'loginUserToken': loginUserToken(),
      'loginToken': 'loginToken',
      'Content-Type': 'application/x-www-form-urlencoded;charset=utf-8',
      'Host': host,
      'Connection': 'keep-alive',
    };

/// 把表单编码为 application/x-www-form-urlencoded（空格→`+`，
/// 与 reqwest `.form()` 对齐；dio 的 FormData 编码细节不同，手工拼最稳）
String encodeForm(Map<String, String> form) {
  String enc(String s) =>
      Uri.encodeQueryComponent(s).replaceAll('%20', '+');
  return form.entries
      .map((e) => '${enc(e.key)}=${enc(e.value)}')
      .join('&');
}

/// API 客户端：持有复用的 HTTP 连接池与请求节流状态
class HubClient {
  HubClient(this.config) : _dio = _buildDio(config);

  final ClientConfig config;
  final Dio _dio;

  /// 上次发起请求的时刻；锁保护「读取→等待→写入」临界区整体串行
  final _throttleLock = Lock();
  DateTime? _lastRequest;

  static Dio _buildDio(ClientConfig config) {
    final dio = Dio(BaseOptions(
      connectTimeout: config.timeout,
      receiveTimeout: config.timeout,
      sendTimeout: config.timeout,
      responseType: ResponseType.plain,
      // 手动管理重定向：SSO 跳转的每一跳都需要检查目标域
      followRedirects: false,
      maxRedirects: 0,
      // 所有状态码都接受，由调用方按业务判定
      validateStatus: (_) => true,
      headers: {
        'User-Agent': 'gnnuhub/0.2.0',
        'Accept-Encoding': 'gzip, deflate',
      },
    ));
    return dio;
  }

  /// 节流执行：持锁补足差额（非固定 sleep），失败不更新时间戳
  Future<Response<String>> throttled(
    Future<Response<String>> Function() build,
  ) async {
    return _throttleLock.synchronized(() async {
      final interval = config.requestInterval;
      if (interval > Duration.zero && _lastRequest != null) {
        final elapsed = DateTime.now().difference(_lastRequest!);
        if (elapsed < interval) {
          final wait = interval - elapsed;
          await Future<void>.delayed(wait);
        }
      }
      final response = await build();
      // 只有请求真正发出后才更新时刻，避免失败重试被误判为「已节流」
      _lastRequest = DateTime.now();
      return response;
    });
  }

  /// 发裸 GET（不检查状态码，重定向也作为正常响应返回；可带 Cookie）
  Future<Response<String>> rawGet(String url, String cookieHeader) {
    return throttled(() => _dio.get<String>(
          url,
          options: Options(
            headers: {
              if (cookieHeader.isNotEmpty) 'Cookie': cookieHeader,
            },
            responseType: ResponseType.plain,
          ),
        ));
  }

  /// 发送 GET，返回文本；非 2xx 抛 [UnexpectedStatusError]
  Future<String> get(String url, {Map<String, String>? headers}) async {
    final response = await throttled(() => _dio.get<String>(
          url,
          options: Options(
            headers: headers,
            responseType: ResponseType.plain,
          ),
        ));
    return _check(response, url);
  }

  /// 发送 POST（query + urlencoded form），返回文本
  Future<String> postForm(
    String url, {
    Map<String, String> query = const {},
    Map<String, String>? form,
    Map<String, String>? headers,
  }) async {
    final queryPart =
        query.isEmpty ? '' : '?${encodeForm(query)}';
    final fullUrl = '$url$queryPart';
    final body = form == null ? null : encodeForm(form);
    final response = await throttled(() => _dio.post<String>(
          fullUrl,
          data: body,
          options: Options(
            headers: {
              if (body != null)
                'Content-Type': 'application/x-www-form-urlencoded',
              ...?headers,
            },
            responseType: ResponseType.plain,
          ),
        ));
    return _check(response, fullUrl);
  }

  String _check(Response<String> response, String url) {
    final status = response.statusCode ?? 0;
    if (status < 200 || status > 299) {
      throw UnexpectedStatusError(status: status, url: url);
    }
    return response.data ?? '';
  }
}

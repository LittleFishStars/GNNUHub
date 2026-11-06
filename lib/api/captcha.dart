/// 验证码获取 —— 对应 Rust `logic/api/src/captcha.rs`
///
/// 接口返回结构（实测）：
/// ```json
/// {
///   "kaptchaType": "2",
///   "uid": "34ae36e5ca2045a485d8ef781c19556f",
///   "content": "data:image/png;base64,iVBORw0KGgo...",
///   "timeout": 300
/// }
/// ```
///
/// **注意**：请求时传入的 `id` 与响应中的 `uid` 不同，提交登录时必须
/// 使用响应里的 `uid`。
library;

import 'dart:convert';
import 'dart:math';

import '../core/constants.dart';
import '../core/error.dart';

/// 验证码接口路径
const String kaptchaPath = '/lyuapServer/kaptcha';

/// 一次验证码申请的完整信息
class Captcha {
  const Captcha({
    required this.uid,
    required this.image,
    required this.timeoutSecs,
  });

  /// 服务端返回的验证码标识
  final String uid;

  /// 图片的 data URL
  final String image;

  /// 有效期（秒）
  final int timeoutSecs;

  /// 生成一个随机请求 id（32 位十六进制，等价 Python `token_hex(16)`）
  static String randomRequestId() {
    final rng = Random.secure();
    final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
    return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  }

  /// 构造验证码接口的完整 URL
  static String requestUrl(String requestId) =>
      '$casBaseUrl$kaptchaPath?id=$requestId';

  /// 从接口响应构造；空 uid 或空图片视为响应异常
  factory Captcha.fromResponse(Map<String, dynamic> json) {
    final uid = (json['uid'] as String?) ?? '';
    final content = (json['content'] as String?) ?? '';
    final timeout = (json['timeout'] as num?)?.toInt() ?? 0;
    if (uid.isEmpty || content.isEmpty) {
      throw const CaptchaResponseError(
          '接口未返回 uid 或图片内容，可能是风控拦截或接口变更');
    }
    return Captcha(uid: uid, image: content, timeoutSecs: timeout);
  }

  /// 从原始文本解析（响应必须是 JSON 对象）
  factory Captcha.parse(String body) {
    dynamic json;
    try {
      json = jsonDecode(body);
    } on FormatException catch (e) {
      throw JsonError('验证码响应不是合法 JSON: $e');
    }
    if (json is! Map<String, dynamic>) {
      throw const CaptchaResponseError('验证码响应结构非预期');
    }
    return Captcha.fromResponse(json);
  }
}

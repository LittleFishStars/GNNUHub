/// 票据接口响应解析 —— 对应 Rust `logic/api/src/login/ticket.rs`
///
/// ⚠️ 核心坑：失败响应中 `meta.success` 可能是 `true`；
/// `statusCode` 在部分响应里是整数而非字符串。判定登录结果
/// **必须依据 `data.code`，不能依赖 meta.success**。
library;

import 'dart:convert';

import '../../core/error.dart';

/// 一次登录尝试的结果
sealed class LoginOutcome {
  const LoginOutcome();
}

/// 认证成功，携带 TGT 与 ticket
class LoginSuccess extends LoginOutcome {
  const LoginSuccess({required this.tgt, required this.ticket});
  final String tgt;
  final String ticket;
}

/// 验证码错误，可以换一张重试
class CaptchaIncorrect extends LoginOutcome {
  const CaptchaIncorrect();
}

/// 学号或密码错误，重试无意义
class BadCredentials extends LoginOutcome {
  const BadCredentials(this.detail);
  final String detail;
}

/// **账号已被锁定**，任何重试都会延长锁定时间
///
/// 实测服务端返回 `{"data":{"code":"USERLOCK"}}`。触发条件是短时间内
/// 连续多次账密错误。必须作为独立分支显式暴露，让上层立刻停手。
class AccountLocked extends LoginOutcome {
  const AccountLocked(this.detail);
  final String detail;
}

/// 解析票据接口响应
///
/// 分支顺序：成功优先 → data.code → meta 判定 → 兜底原文。
LoginOutcome parseTicketResponse(String body) {
  // 分支 0：成功优先（`tgt` 与 `ticket` 同时存在且均为字符串）
  try {
    final raw = jsonDecode(body);
    if (raw is Map) {
      final tgt = raw['tgt'];
      final ticket = raw['ticket'];
      if (tgt is String && ticket is String) {
        return LoginSuccess(tgt: tgt, ticket: ticket);
      }
    }
  } on FormatException {
    // 落到分支 1 统一报 JsonError
  }

  // 分支 1：结构解析（非 JSON 文本在此直接报错）
  final Map<String, dynamic> root;
  try {
    final decoded = jsonDecode(body);
    if (decoded is! Map) throw const JsonError('票据响应不是 JSON 对象');
    root = decoded.cast<String, dynamic>();
  } on FormatException catch (e) {
    throw JsonError(e.message);
  }

  final data = root['data'];
  final meta = root['meta'];

  // 分支 2：data 存在（进入即必 return，绝不 fall through 到 meta）
  if (data is Map) {
    final code = ((data['code'] as String?) ?? '').trim();
    final extra = data['data'];
    if (code.isEmpty || code.toUpperCase() == 'SUCCESS') {
      throw const TicketMissingError('响应声明成功但未携带 ticket');
    }
    if (code.toUpperCase() == 'CODEFALSE') {
      return const CaptchaIncorrect();
    }
    if (code.toUpperCase() == 'PASSERROR') {
      final detail = extra is String ? extra.trim() : '';
      return BadCredentials(detail.isNotEmpty
          ? '密码错误（$detail = 剩余次数,已用次数）'
          : '密码错误');
    }
    if (code.toUpperCase() == 'USERLOCK') {
      return const AccountLocked(
          '账号已被锁定（USERLOCK），请等待解锁后再试，期间不要重复提交');
    }
    throw TicketMissingError('未识别的业务码: $code');
  }

  // 分支 3：无 data、meta 存在
  if (meta is Map) {
    final statusCodeValue = meta['statusCode'];
    final statusCode = statusCodeValue is String
        ? statusCodeValue
        : (statusCodeValue == null ? '' : statusCodeValue.toString());
    final metaMessage = (meta['message'] as String?) ?? '';
    final message = metaMessage.isEmpty ? statusCode : metaMessage;
    final lowerMessage = message.toLowerCase();
    if (statusCode.toUpperCase() == 'CODEFALSE' ||
        message.contains('验证码') ||
        lowerMessage.contains('code')) {
      return const CaptchaIncorrect();
    }
    if (statusCode.toUpperCase() == 'USERNAMEORPASSWORDERROR' ||
        message.contains('密码') ||
        message.contains('用户名')) {
      return BadCredentials(message);
    }
    throw TicketMissingError(message);
  }

  // 分支 4：data/meta 都没有
  throw TicketMissingError(_truncate(body, 200));
}

/// 按字符（runes）截断，避免孤立代理对
String _truncate(String s, int max) {
  final runes = s.runes.toList();
  if (runes.length <= max) return s;
  return String.fromCharCodes(runes.take(max));
}

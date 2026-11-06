/// 项目统一错误类型 —— 对应 Rust `logic/core/src/error.rs`
///
/// Display 文案是中文、面向最终用户的，UI 可直接展示；
/// 程序化分类用 [retrySafe] 判断「重试是否有害」。
sealed class HubError implements Exception {
  const HubError();

  /// 面向用户的中文文案
  String get message;

  @override
  String toString() => message;
}

/// 网络请求失败（DNS、连接、超时等）
class NetworkError extends HubError {
  const NetworkError(this.detail);
  final String detail;

  @override
  String get message => '网络请求失败: $detail';
}

/// 服务端返回了非预期状态码
class UnexpectedStatusError extends HubError {
  const UnexpectedStatusError({required this.status, required this.url});
  final int status;
  final String url;

  @override
  String get message => '服务端返回异常状态码 $status: $url';
}

/// 响应体不是合法 JSON
class JsonError extends HubError {
  const JsonError(this.detail);
  final String detail;

  @override
  String get message => '响应解析失败: $detail';
}

/// 学号或密码错误
class InvalidCredentialsError extends HubError {
  const InvalidCredentialsError();

  @override
  String get message => '学号或密码错误';
}

/// 验证码识别错误或已过期
class InvalidCaptchaError extends HubError {
  const InvalidCaptchaError();

  @override
  String get message => '验证码错误或已过期';
}

/// 登录票据获取失败（含账号锁定等服务端文案）
class TicketMissingError extends HubError {
  const TicketMissingError(this.detail);
  final String detail;

  @override
  String get message => '登录票据获取失败: $detail';
}

/// SSO 跳转未能落在教务系统域内
class SsoRedirectError extends HubError {
  const SsoRedirectError(this.detail);
  final String detail;

  @override
  String get message => 'SSO 跳转异常，未落在教务系统域内: $detail';
}

/// 登录流程重试次数耗尽
class LoginRetriesExhaustedError extends HubError {
  const LoginRetriesExhaustedError(this.attempts);
  final int attempts;

  @override
  String get message => '登录重试 $attempts 次后仍然失败';
}

/// 验证码接口返回异常
class CaptchaResponseError extends HubError {
  const CaptchaResponseError(this.detail);
  final String detail;

  @override
  String get message => '验证码接口响应异常: $detail';
}

/// 会话未认证或已过期
class UnauthenticatedError extends HubError {
  const UnauthenticatedError();

  @override
  String get message => '会话未认证或已过期，请重新登录';
}

/// 页面结构与预期不符（HTML 解析失败）
class PageParseError extends HubError {
  const PageParseError(this.detail);
  final String detail;

  @override
  String get message => '页面解析失败，结构可能已变更: $detail';
}

/// 缺少必需字段
class MissingFieldError extends HubError {
  const MissingFieldError(this.field);
  final String field;

  @override
  String get message => '缺少必需字段: $field';
}

/// 图像处理失败
class ImageError extends HubError {
  const ImageError(this.detail);
  final String detail;

  @override
  String get message => '图像处理失败: $detail';
}

/// 配置问题
class ConfigError extends HubError {
  const ConfigError(this.detail);
  final String detail;

  @override
  String get message => '配置错误: $detail';
}

/// 其他未归类错误
class OtherError extends HubError {
  const OtherError(this.detail);
  final String detail;

  @override
  String get message => detail;
}

/// 服务层补充错误：未登录就调用受保护操作
class NotLoggedInError extends HubError {
  const NotLoggedInError();

  @override
  String get message => '尚未登录，请先登录';
}

/// 服务层补充错误：学号格式不合法
class InvalidStudentIdError extends HubError {
  const InvalidStudentIdError();

  @override
  String get message => '学号必须是纯数字';
}

/// 重试同一操作是否安全
///
/// 凭据错误、账号锁定、未登录、学号非法时重试无意义或有害
/// （账号锁定重试会加重锁定）。网络抖动、验证码识别错误等可安全重试。
bool retrySafe(HubError error) => switch (error) {
      NotLoggedInError() ||
      InvalidStudentIdError() ||
      InvalidCredentialsError() ||
      TicketMissingError() => false,
      _ => true,
    };

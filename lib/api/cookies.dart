/// Cookie 解析与拼接 —— 对应 Rust `logic/api/src/login/cookies.rs`
///
/// 所有 Cookie 都是**显式管理**的（禁用 HTTP 库的 cookie jar），
/// 这里是唯一的拼装/拆解入口。
library;

/// 把 Cookie 映射拼成请求头（`k=v` 用 `"; "` 连接）
String buildCookieHeader(Map<String, String> cookies) =>
    cookies.entries.map((e) => '${e.key}=${e.value}').join('; ');

/// 从 Set-Cookie 头提取名称与值（只取第一段 `name=value`）
///
/// 返回 null 表示该 Cookie 不应被携带：
/// - 名称为空
/// - 值为 `DELETED`（教务系统的失效标记）
/// - 值为 `deleteMe`（实测携带 `rememberMe=deleteMe` 会导致服务端
///   直接关闭连接，必须剔除；大小写不敏感，但仅限完全相等）
(String, String)? parseSetCookie(String header) {
  final first = header.split(';').first;
  final eq = first.indexOf('=');
  if (eq < 0) return null;
  final name = first.substring(0, eq).trim();
  final value = first.substring(eq + 1).trim();
  if (name.isEmpty) return null;
  final lower = value.toLowerCase();
  if (lower == 'deleted' || lower == 'deleteme') return null;
  return (name, value);
}

/// 判断 Set-Cookie 是否只对 `/sso` 路径生效
///
/// 只解析 `Path` 属性；缺少该属性时按浏览器默认行为视为当前目录
/// （对登录流程而言即根作用域）。
bool isSsoScoped(String setCookie) {
  for (final attr in setCookie.split(';').skip(1)) {
    final eq = attr.indexOf('=');
    if (eq < 0) continue;
    if (attr.substring(0, eq).trim().toLowerCase() == 'path') {
      final value = attr.substring(eq + 1).trim();
      if (value.startsWith('/sso')) return true;
    }
  }
  return false;
}

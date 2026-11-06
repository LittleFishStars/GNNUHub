/// 统一身份认证平台的加密实现 —— 对应 Rust `logic/crypto/src/lib.rs`
///
/// 复刻前端 JS 的 RSA 加密过程：明文按 ASCII 分块，每 2 字节合并为
/// 一个 base-65536 数位，对块做 `value^65537 mod N` 模幂，结果转
/// 16 进制、块间空格分隔。
library;

import 'dart:convert';

/// RSA 公钥指数
const int _exponent = 65537;

/// 每个明文块的字节数
const int chunkSize = 126;

/// 模数（base 65536 数位表示，低位在前，由原 JS 提取）
const List<int> _modulusDigits = [
  59313, 4375, 54507, 5267, 8345, 43610, 49971, 28563, //
  34983, 36521, 17297, 62027, 42744, 32131, 40043, 48417,
  5636, 46659, 52373, 20768, 28635, 46498, 55076, 13948,
  44453, 44804, 40613, 1466, 26896, 54350, 28506, 28712,
  44726, 4974, 46852, 32655, 60720, 2973, 7722, 43040,
  10398, 28111, 52739, 6542, 43865, 20892, 59308, 8898,
  58877, 36302, 41921, 27719, 59291, 10923, 8559, 53747,
  10707, 59976, 48415, 32958, 37390, 57449, 45414, 46574,
];

/// 把 base-65536 数位数组还原为大整数（低位在前）
BigInt _modulus() {
  var value = BigInt.zero;
  for (final digit in _modulusDigits.reversed) {
    value = (value << 16) + BigInt.from(digit);
  }
  return value;
}

final BigInt _mod = _modulus();

/// 对单个明文块加密，返回 16 进制小写串
///
/// 每 2 字节合并为一个 16 位「数位」：低字节在前
/// （对应原 JS `c.digits[r] = a[l] + (a[l+1] << 8)`）。
String _encryptBlock(List<int> block) {
  var value = BigInt.zero;
  // chunks(2).rev()：从最后一个完整块起向前累加
  final chunks = <List<int>>[];
  for (var i = 0; i < block.length; i += 2) {
    chunks.add(block.sublist(i, i + 2 > block.length ? block.length : i + 2));
  }
  for (final chunk in chunks.reversed) {
    value = value << 16;
    final low = chunk[0];
    final high = chunk.length > 1 ? chunk[1] : 0;
    value += BigInt.from(low + (high << 8));
  }
  final encrypted = value.modPow(BigInt.from(_exponent), _mod);
  return encrypted.toRadixString(16);
}

/// 对任意 ASCII 字符串执行加密，返回以空格分隔的 16 进制密文
String encrypt(String plaintext) {
  if (plaintext.isEmpty) return '';

  // 对应 Rust `plaintext.bytes()`：取 UTF-8 字节
  final bytes = List<int>.from(utf8.encode(plaintext)); // 定长 Uint8List → 可变列表（需补零）
  // 补零到 chunkSize 的整数倍
  while (bytes.length % chunkSize != 0) {
    bytes.add(0);
  }

  final parts = <String>[];
  for (var i = 0; i < bytes.length; i += chunkSize) {
    parts.add(_encryptBlock(bytes.sublist(i, i + chunkSize)));
  }
  return parts.join(' ');
}

/// 加密登录密码
String encodePassword(String password) => encrypt(password);

/// 生成并加密 `loginUserToken`：`lyasp` 前缀 + 当前毫秒时间戳
String loginUserToken() {
  final millis = DateTime.now().millisecondsSinceEpoch;
  return encrypt('lyasp$millis');
}

/// 学籍与个人信息解析 —— 对应 Rust `logic/api/src/parse/student.rs`
///
/// 三个入口：
/// - [parseBasicInfo]：首页个人信息页 HTML（片段级容错，永不失败）
/// - [parseStudentInfo]：学籍信息页 HTML（值在 `#col_xxx p`）
/// - [parseStudentProfileJson]：学籍 JSON 接口（键名 = HTML id 去 `col_`）
library;

import 'dart:convert';

import 'package:html/parser.dart';

import '../../core/error.dart';
import '../../core/constants.dart';
import '../../core/model.dart';
import 'common.dart';

/// 首页个人信息页 HTML → StudentInfo
///
/// 每字段独立容错，节点缺失留 null，整体永不失败。
StudentInfo parseBasicInfo(String html) {
  final doc = parse(html);
  final info = StudentInfo();

  final h4 = doc.querySelector('h4');
  if (h4 != null) {
    final text = allText(h4);
    final parts = text
        .split(RegExp('[\u{00A0} ]'))
        .where((s) => s.trim().isNotEmpty)
        .toList();
    if (parts.isNotEmpty) info.name = parts[0].trim();
    if (parts.length > 1) info.identity = parts[1].trim();
  }

  final p = doc.querySelector('p');
  if (p != null) {
    final parts = allText(p)
        .split(RegExp(r'\s+'))
        .where((s) => s.isNotEmpty)
        .toList();
    if (parts.isNotEmpty) info.college = parts[0];
    if (parts.length > 1) info.className = parts[1];
  }

  final img = doc.querySelector('img');
  final src = img?.attributes['src'];
  if (src != null) {
    info.avatar = '$jwglBaseUrl$src';
  }

  return info;
}

/// 学籍信息页 HTML → StudentInfo（值在 `<div id="col_xxx"><p>值</p></div>`）
StudentInfo parseStudentInfo(String html) {
  final doc = parse(html);
  final info = StudentInfo();

  String? textOf(String id) {
    final node = doc.querySelector('#$id p');
    if (node == null) return null;
    final text = allText(node).trim();
    return text.isEmpty ? null : text;
  }

  info.name = textOf('col_xm');
  info.gender = textOf('col_xbm');
  info.birthday = textOf('col_csrq');
  info.ethnicity = textOf('col_mzm');
  info.politicalStatus = textOf('col_zzmmm');
  info.college = textOf('col_jg_id');
  info.address = textOf('col_txdz');

  final major = textOf('col_zyh_id');
  if (major != null) {
    // 按字符去末尾 6 位代码后缀（仅超长才截，避免多字节切断）
    final chars = major.characters.toList();
    info.major =
        chars.length > 6 ? chars.sublist(0, chars.length - 6).join() : major;
  }

  info.className = textOf('col_bh_id');

  final enrollment = textOf('col_zsnddm');
  if (enrollment != null) {
    info.enrollmentYear = enrollment.characters.take(4).join();
  }

  final kind = textOf('col_zjlxm');
  final number = textOf('col_zjhm');
  if (kind != null || number != null) {
    info.document = DocumentInfo(kind: kind ?? '', number: number ?? '');
  }

  return info;
}

/// 学籍 JSON 接口（`xsxxwh_cxCkDgxsxx.html`，模块码 N100801）→ StudentInfo
///
/// 键名规律 = HTML 字段 id 去掉 `col_` 前缀；64 个顶层键按表映射、
/// 忽略未知键。`zyh_id` 是干净名称，**绝不能截尾**。
StudentInfo parseStudentProfileJson(String body) {
  dynamic root;
  try {
    root = jsonDecode(body);
  } on FormatException catch (e) {
    throw JsonError(e.message);
  }
  if (root is! Map) {
    throw const PageParseError('学籍 JSON 响应结构非预期');
  }
  String? raw(String key) => nonEmpty(root[key] as String?);

  final info = StudentInfo();
  info.studentId = raw('xh');
  info.name = raw('xm');
  info.gender = raw('xbm');
  info.college = raw('jg_id');
  info.major = raw('zyh_id'); // JSON 侧是干净名称，无需剥后缀
  info.className = raw('bh_id');
  info.enrollmentYear = raw('zsnddm');
  info.birthday = raw('csrq');
  info.ethnicity = raw('mzm');
  info.politicalStatus = raw('zzmmm');
  info.address = raw('txdz');
  info.instructor = raw('fdyjgh'); // 辅导员，HTML 页没有
  info.identity = raw('pyccdm'); // 培养层次（本科/研究生）

  final kind = raw('zjlxm');
  final number = raw('zjhm');
  // 「证件号缺失时不应造出半份证件」：两者皆空 → document 为 null
  if (kind != null || number != null) {
    info.document = DocumentInfo(kind: kind ?? '', number: number ?? '');
  }
  return info;
}

extension _Characters on String {
  Iterable<String> get characters => runes.map(String.fromCharCode);
}

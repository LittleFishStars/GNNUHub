/// 首页门户组件解析：通知公告、系统提醒、校历、通知正文
/// —— 对应 Rust `logic/api/src/parse/portal.rs`
library;

import 'dart:convert';

import 'package:html/dom.dart' as dom;
import 'package:html/parser.dart';

import '../../core/error.dart';
import '../../core/model.dart';
import 'common.dart';

/// 首页通知栏 HTML **片段** → 通知列表
///
/// ⚠️ 必须用 `parseFragment`（本目录其他 HTML 函数都用 document）。
/// 置顶用文本 contains(`【置顶】`) 判定，不是 font 颜色；
/// 标题取 `<a>` 的 `title` 属性，trim 后为空则整条丢弃。
List<Notice> parseNotices(String html) {
  final fragment = parseFragment(html);
  final notices = <Notice>[];
  for (final a in fragment.querySelectorAll('a.list-group-item')) {
    final title = (a.attributes['title'] ?? '').trim();
    if (title.isEmpty) continue;

    final timeNode = a.querySelector('span.time');
    final date = timeNode == null ? '' : allText(timeNode).trim();

    final spanTitle = a.querySelector('span.title');
    final spanText = spanTitle == null ? '' : allText(spanTitle);
    final pinned = spanText.contains('【置顶】');

    // 只取第一个【…】作栏目标记；【置顶】不当作 category
    var category = '';
    var clean = spanText.replaceAll('【置顶】', '');
    final start = clean.indexOf('【');
    if (start >= 0) {
      final rel = clean.substring(start).indexOf('】');
      if (rel >= 0) {
        final end = start + rel + '】'.length;
        final tag = clean.substring(start, end);
        if (tag != '【置顶】') category = tag;
        clean = clean.replaceAll(tag, '');
      }
    }

    var href = a.attributes['href'] ?? '';
    if (!href.startsWith('/')) href = '/$href';

    notices.add(Notice(
      title: title,
      date: date,
      detailPath: href,
      pinned: pinned,
      category: category,
    ));
  }
  return notices;
}

/// 系统提醒：响应是**一个 JSON 字符串**，多条以 `|` 分隔，
/// 可能带尾部空段 → 先解码成 String 再 split+trim+去空
List<String> parseSystemMessages(String body) {
  dynamic decoded;
  try {
    decoded = jsonDecode(body);
  } on FormatException catch (e) {
    throw JsonError(e.message);
  }
  if (decoded is! String) return const [];
  return decoded
      .split('|')
      .map((s) => s.trim())
      .where((s) => s.isNotEmpty)
      .toList();
}

/// 通知正文页 → NoticeDetail
///
/// 发布信息用**全角冒号**；`div.news_con` 缺失是硬错误；
/// 图片以 `图片: <路径>` 内联标注。
NoticeDetail parseNoticeDetail(String html) {
  final doc = parse(html);
  String textOf(String selector) {
    final node = doc.querySelector(selector);
    return node == null ? '' : allText(node).trim();
  }

  final title = textOf('h3.text-center');
  var publisher = '';
  var publishTime = '';
  var views = '';
  for (final span in doc.querySelectorAll('h5.news_title1 span')) {
    final text = allText(span).trim();
    if (text.startsWith('发布人：')) {
      publisher = text.substring('发布人：'.length).trim();
    } else if (text.startsWith('发布时间：')) {
      publishTime = text.substring('发布时间：'.length).trim();
    } else if (text.startsWith('浏览人数：')) {
      views = text.substring('浏览人数：'.length).trim();
    }
  }

  final con = doc.querySelector('div.news_con');
  if (con == null) {
    throw const PageParseError('通知正文页缺少 news_con 容器');
  }

  final buffer = StringBuffer();
  void walk(dom.Node node) {
    if (node is dom.Text) {
      final text = node.data.trim();
      if (text.isNotEmpty) {
        buffer.write(text);
        buffer.write(' ');
      }
    } else if (node is dom.Element) {
      final name = node.localName?.toLowerCase() ?? '';
      if (name == 'img') {
        final src = node.attributes['src'];
        if (src != null) buffer.write('图片: $src ');
      }
      if (name == 'p' || name == 'br' || name == 'div' || name == 'tr') {
        buffer.write('\n');
      }
      for (final child in node.nodes) {
        walk(child);
      }
    }
  }

  walk(con);
  // 每行压空白、丢空行
  final content = buffer
      .toString()
      .split('\n')
      .map((line) => line.split(RegExp(r'\s+')).where((s) => s.isNotEmpty).join(' '))
      .where((line) => line.isNotEmpty)
      .join('\n');

  return NoticeDetail(
    title: title,
    publisher: publisher,
    publishTime: publishTime,
    views: views,
    content: content,
  );
}

/// 校历组件 HTML → AcademicCalendar
///
/// 目标模式：`2026-2027学年1学期(2026-08-31至2027-01-17)`。
/// 刻意不用正则，用定位字符串方式提取（与原实现一致）。
AcademicCalendar parseCalendar(String html) {
  try {
    final yearPos = html.indexOf('学年');
    if (yearPos < 0) throw const _ParseFail();
    final before = html.substring(0, yearPos);
    // 从末尾倒推连续的 ASCII 数字与 '-'
    var digitsStart = before.length;
    while (digitsStart > 0) {
      final c = before.codeUnitAt(digitsStart - 1);
      final isDigit = c >= 0x30 && c <= 0x39;
      if (!isDigit && c != 0x2D) break;
      digitsStart--;
    }
    final year = int.tryParse(before.substring(digitsStart).split('-').first);
    if (year == null) throw const _ParseFail();

    final parenRel = html.substring(yearPos).indexOf('(');
    if (parenRel < 0) throw const _ParseFail();
    final paren = yearPos + parenRel;
    final closeRel = html.substring(paren).indexOf(')');
    if (closeRel < 0) throw const _ParseFail();
    final inner = html.substring(paren + 1, paren + closeRel);
    final sep = inner.indexOf('至'); // 全角「至」
    if (sep < 0) throw const _ParseFail();
    final startDate = inner.substring(0, sep).trim();
    final endDate = inner.substring(sep + '至'.length).trim();

    final seg = html.substring(yearPos + '学年'.length, paren);
    final termText = seg.endsWith('学期')
        ? seg.substring(0, seg.length - '学期'.length).trim()
        : seg.trim();
    final term = int.tryParse(termText);
    if (term == null) throw const _ParseFail();

    return AcademicCalendar(
      year: year,
      term: term,
      startDate: startDate,
      endDate: endDate,
    );
  } on _ParseFail {
    throw const PageParseError('校历页中未找到学期信息');
  }
}

class _ParseFail implements Exception {
  const _ParseFail();
}

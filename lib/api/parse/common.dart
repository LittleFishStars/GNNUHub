/// 解析层共用工具 —— 对应 Rust `logic/api/src/parse/mod.rs`
library;

import 'dart:convert';
import 'package:html/dom.dart' as dom;
import '../../core/error.dart';

/// jqGrid 分页信封：数据一律在 `items`；空信封/缺字段 → 空列表
List<Map<String, dynamic>> jqItems(String body) {
  dynamic root;
  try {
    root = jsonDecode(body);
  } on FormatException catch (e) {
    throw JsonError(e.message);
  }
  if (root is! Map) return const [];
  final items = root['items'];
  if (items is! List) return const [];
  return [
    for (final item in items)
      if (item is Map) item.cast<String, dynamic>(),
  ];
}

/// 宽松数值解析：失败退化为 0（「缺字段留空而不是失败」）
double parseNum(String? raw) =>
    double.tryParse(raw?.trim() ?? '') ?? 0.0;

/// 宽松整数解析：失败退化为 0
int parseIntSafe(String? raw) => int.tryParse(raw?.trim() ?? '') ?? 0;

/// trim 后为空 → null（「空白一律视为未提供」）
String? nonEmpty(String? raw) {
  final s = raw?.trim() ?? '';
  return s.isEmpty ? null : s;
}

/// 节点文本合并（丢弃标签、保留全部文本）
String allText(dynamic node) {
  if (node == null) return '';
  final buffer = StringBuffer();
  void walk(dynamic n) {
    if (n is dom.Text) {
      buffer.write(n.data);
    } else if (n is dom.Element) {
      for (final child in n.nodes) {
        walk(child);
      }
    }
  }

  walk(node);
  return buffer.toString();
}

/// 字形与字库 —— 对应 Rust `logic/ocr/src/bitmap.rs` 的字库部分
library;

import 'dart:convert';
import 'package:flutter/services.dart' show rootBundle;

import '../core/error.dart';

/// 一个字形（包围盒裁剪后的 0/1 位串）
class Glyph {
  const Glyph({required this.w, required this.h, required this.bits});
  final int w;
  final int h;
  final String bits;
}

/// 字库中的一条目
class GlyphEntry {
  GlyphEntry({
    required this.w,
    required this.h,
    required List<String> rows,
    required this.label,
  }) : bits = rows.join();

  final int w;
  final int h;
  final String bits;
  final String label;

  factory GlyphEntry.fromJson(Map<String, dynamic> json, int index) {
    final w = (json['w'] as num?)?.toInt();
    final h = (json['h'] as num?)?.toInt();
    final label = json['label'] as String?;
    final rows = json['rows'];
    if (w == null) throw ConfigError('字库第 $index 条缺少 w');
    if (h == null) throw ConfigError('字库第 $index 条缺少 h');
    if (label == null) throw ConfigError('字库第 $index 条缺少 label');
    if (rows is! List) throw ConfigError('字库第 $index 条缺少 rows');
    if (rows.length != h) {
      throw ConfigError('字库第 $index 条 rows 行数 ${rows.length} != h=$h');
    }
    for (var i = 0; i < rows.length; i++) {
      final row = rows[i];
      if (row is! String || row.length != w) {
        throw ConfigError(
            '字库第 $index 条第 $i 行宽度与 w=$w 不符: ${row.runtimeType}');
      }
    }
    return GlyphEntry(w: w, h: h, rows: rows.cast<String>(), label: label[0]);
  }
}

/// 位图字库：精确表 + 按尺寸分组表
class BitmapLibrary {
  BitmapLibrary._(this._exact, this._bySize, this.entries);

  /// 精确表：键 `"$w,$h,$bits"` → 字符
  final Map<String, String> _exact;

  /// 按尺寸分组（保留全部条目，含同 label 重复）
  final Map<String, List<GlyphEntry>> _bySize;

  /// 原始条目
  final List<GlyphEntry> entries;

  int get len => _exact.length;

  /// 从 JSON 文本解析
  factory BitmapLibrary.fromJson(String text) {
    dynamic root;
    try {
      root = jsonDecode(text);
    } on FormatException catch (e) {
      throw ConfigError('字库 JSON 解析失败: $e');
    }
    if (root is! Map<String, dynamic>) {
      throw const ConfigError('字库缺少 entries 数组');
    }
    final rawEntries = root['entries'];
    if (rawEntries is! List) {
      throw const ConfigError('字库缺少 entries 数组');
    }
    final exact = <String, String>{};
    final bySize = <String, List<GlyphEntry>>{};
    final entries = <GlyphEntry>[];
    for (var i = 0; i < rawEntries.length; i++) {
      final entry =
          GlyphEntry.fromJson((rawEntries[i] as Map).cast<String, dynamic>(), i);
      entries.add(entry);
      // JSON 重复键语义：精确表后者覆盖、分组表全保留
      exact['${entry.w},${entry.h},${entry.bits}'] = entry.label;
      bySize.putIfAbsent('${entry.w},${entry.h}', () => []).add(entry);
    }
    return BitmapLibrary._(exact, bySize, entries);
  }

  /// 从 Flutter asset 加载（assets/ocr/bitmap_lib.json）
  static Future<BitmapLibrary> fromAsset() async {
    final text = await rootBundle.loadString('assets/ocr/bitmap_lib.json');
    return BitmapLibrary.fromJson(text);
  }

  /// 精确匹配，未命中返回 null
  String? lookupExact(int w, int h, String bits) =>
      _exact['$w,$h,$bits'];

  /// 模糊匹配：只在同 (w,h) 组内做汉明距离比较，
  /// 严格更小才替换（平局取 JSON 中靠前的条目）
  (String, int)? lookupFuzzy(int w, int h, String bits, int maxDistance) {
    final group = _bySize['$w,$h'];
    if (group == null) return null;
    String? bestLabel;
    var bestDistance = 1 << 62;
    for (final candidate in group) {
      if (candidate.bits.length != bits.length) continue;
      var d = 0;
      for (var i = 0; i < bits.length; i++) {
        if (candidate.bits[i] != bits[i]) d++;
      }
      if (d > maxDistance) continue;
      if (d < bestDistance) {
        bestDistance = d;
        bestLabel = candidate.label;
      }
    }
    return bestLabel == null ? null : (bestLabel, bestDistance);
  }

  /// 精确优先、其次模糊；都落空返回 null
  String? lookup(int w, int h, String bits, int maxDistance) {
    final exact = lookupExact(w, h, bits);
    if (exact != null) return exact;
    return lookupFuzzy(w, h, bits, maxDistance)?.$1;
  }
}

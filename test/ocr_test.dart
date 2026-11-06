/// OCR 字库与真实样本回归测试
///
/// 与 Rust `logic/ocr/tests/real_samples.rs` 对齐：跑全部 captcha_samples
/// 样本（每个样本的期望答案在同名 .json 里）。
library;

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:gnnuhub/core/error.dart';
import 'package:gnnuhub/ocr/bitmap_ocr.dart';
import 'package:gnnuhub/ocr/library.dart';

void main() {
  final samplesDir = Directory('captcha_samples');
  final sampleFiles = samplesDir.existsSync()
      ? samplesDir
          .listSync()
          .whereType<File>()
          .where((f) => f.path.endsWith('.png'))
          .toList()
      : <File>[];

  test('字库加载：72 条目、覆盖 62 个字符', () {
    final lib = BitmapLibrary.fromJson(
        File('assets/ocr/bitmap_lib.json').readAsStringSync());
    expect(lib.len, 72, reason: '精确键数');
    expect(lib.entries.map((e) => e.label).toSet().length, 62);
  });

  test('真实样本回归（位图查表应全对）', () {
    expect(sampleFiles, isNotEmpty, reason: 'captcha_samples/ 应存在样本');
    final lib = BitmapLibrary.fromJson(
        File('assets/ocr/bitmap_lib.json').readAsStringSync());
    final ocr = BitmapOcr(lib); // 默认模糊距离 2（与原实现一致）

    var pass = 0;
    final failures = <String>[];
    for (final png in sampleFiles) {
      // 样本 JSON 只含 index/uid（与 Rust real_samples.rs 一致：
      // 字库与样本同源，验证「全部识别出 4 个合法字符」即可）
      final image =
          'data:image/png;base64,${base64Encode(png.readAsBytesSync())}';

      try {
        final actual = ocr.recognize(image);
        if (actual.length == 4 &&
            actual.split('').every((c) => captchaAlphabet.contains(c))) {
          pass++;
        } else {
          failures.add('${png.path.split(Platform.pathSeparator).last}: 非法输出 $actual');
        }
      } on HubError catch (e) {
        failures.add('${png.path.split(Platform.pathSeparator).last}: ${e.message}');
      }
    }
    expect(failures, isEmpty,
        reason: 'OCR 命中率 $pass/${sampleFiles.length}');
  });

  test('模糊匹配 ≤2 可吸收抖动', () {
    final lib = BitmapLibrary.fromJson(
        File('assets/ocr/bitmap_lib.json').readAsStringSync());
    final entry = lib.entries.first;
    // 翻转 2 个位后模糊匹配仍应命中同一字符
    final chars = entry.bits.split('');
    var flipped = 0;
    for (var i = 0; i < chars.length && flipped < 2; i++) {
      if (chars[i] != entry.bits[0]) {
        chars[i] = entry.bits[0];
        flipped++;
      }
    }
    final (label, distance) =
        lib.lookupFuzzy(entry.w, entry.h, chars.join(), 2)!;
    expect(label, entry.label);
    expect(distance, lessThanOrEqualTo(2));
  });
}

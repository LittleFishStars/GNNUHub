/// 本地轻量持久化：记住学号与上次选用的学年学期
library;

import 'package:shared_preferences/shared_preferences.dart';

class TermStore {
  TermStore._();

  static const _keyStudentId = 'student_id';
  static const _keyYear = 'term_year';
  static const _keyTerm = 'term_index';

  static Future<String?> loadStudentId() async {
    final prefs = await SharedPreferences.getInstance();
    return prefs.getString(_keyStudentId);
  }

  static Future<void> saveStudentId(String id) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_keyStudentId, id);
  }

  static Future<void> saveTerm({required int year, required int termIndex}) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setInt(_keyYear, year);
    await prefs.setInt(_keyTerm, termIndex);
  }

  static Future<({int year, int termIndex})?> loadTerm() async {
    final prefs = await SharedPreferences.getInstance();
    final year = prefs.getInt(_keyYear);
    final termIndex = prefs.getInt(_keyTerm);
    if (year == null || termIndex == null) return null;
    return (year: year, termIndex: termIndex);
  }
}

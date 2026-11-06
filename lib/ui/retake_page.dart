/// 重修课程库页（服务端返回全量 4312 条，本地过滤）
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import 'schedule_page.dart' show courseColor;
import 'widgets/panels.dart';

class RetakePage extends StatefulWidget {
  const RetakePage({super.key});

  @override
  State<RetakePage> createState() => _RetakePageState();
}

class _RetakePageState extends State<RetakePage> {
  List<RetakeCourse>? _all;
  Object? _error;
  bool _busy = false;
  String _query = '';

  Future<void> _load() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final year = DateTime.now().month >= 8
          ? DateTime.now().year
          : DateTime.now().year - 1;
      final courses = await context
          .read<HubService>()
          .retakeCourses(AcademicTerm.first(year));
      if (mounted) setState(() => _all = courses);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final all = _all;
    final filtered = all == null
        ? null
        : (_query.isEmpty
            ? all
            : all
                .where((c) =>
                    c.courseName.contains(_query) ||
                    c.courseCode.contains(_query) ||
                    c.college.contains(_query))
                .toList());

    return Scaffold(
      appBar: AppBar(
        title: const Text('重修课程'),
        actions: [
          IconButton(
            tooltip: '刷新',
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(LucideIcons.refreshCw, size: 18),
            onPressed: _busy ? null : _load,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 4, 14, 8),
            child: TextField(
              onChanged: (v) => setState(() => _query = v.trim()),
              decoration: const InputDecoration(
                prefixIcon: Icon(LucideIcons.search, size: 17),
                hintText: '按课程名 / 课程代码 / 开课学院过滤',
              ),
            ),
          ),
          Expanded(
            child: _error != null
                ? ErrorPanel(message: errorText(_error!), onRetry: _load)
                : _busy && filtered == null
                    ? const LoadingPanel(label: '拉取重修课程库…')
                    : filtered == null
                        ? const EmptyPanel()
                        : filtered.isEmpty
                            ? const EmptyPanel(
                                icon: LucideIcons.bookX, label: '没有匹配的课程')
                            : ListView.builder(
                                padding:
                                    const EdgeInsets.symmetric(horizontal: 14),
                                itemCount: filtered.length,
                                itemExtent: 62,
                                itemBuilder: (context, i) {
                                  final course = filtered[i];
                                  return Card(
                                    margin:
                                        const EdgeInsets.only(bottom: 8),
                                    child: ListTile(
                                      dense: true,
                                      leading: Container(
                                        width: 38,
                                        height: 38,
                                        alignment: Alignment.center,
                                        decoration: ShapeDecoration(
                                          color:
                                              courseColor(course.courseName, context),
                                          shape: RoundedRectangleBorder(
                                            borderRadius:
                                                BorderRadius.circular(10),
                                          ),
                                        ),
                                        child: Text(
                                          course.credit == 0
                                              ? ''
                                              : course.credit
                                                  .toStringAsFixed(
                                                      course.credit
                                                              .truncateToDouble() ==
                                                          course.credit
                                                      ? 0
                                                      : 1),
                                          style: const TextStyle(
                                            fontSize: 12.5,
                                            fontWeight: FontWeight.w700,
                                          ),
                                        ),
                                      ),
                                      title: Text(
                                        course.courseName,
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: const TextStyle(
                                            fontSize: 14,
                                            fontWeight: FontWeight.w600),
                                      ),
                                      subtitle: Text(
                                        [
                                          if (course.courseCode.isNotEmpty)
                                            course.courseCode,
                                          if (course.college.isNotEmpty)
                                            course.college,
                                          '重修批次 ${course.academicYear}-${course.semester}',
                                        ].join(' · '),
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: TextStyle(
                                            fontSize: 11.5,
                                            color: scheme.onSurfaceVariant),
                                      ),
                                      trailing: course.retakeStatus.isEmpty ||
                                              course.retakeStatus == '空'
                                          ? Text(
                                              '未开班',
                                              style: TextStyle(
                                                fontSize: 11.5,
                                                color: scheme
                                                    .onSurfaceVariant,
                                              ),
                                            )
                                          : Container(
                                              padding: const EdgeInsets
                                                  .symmetric(
                                                  horizontal: 8,
                                                  vertical: 3),
                                              decoration: ShapeDecoration(
                                                color:
                                                    scheme.primaryContainer,
                                                shape: RoundedRectangleBorder(
                                                  borderRadius:
                                                      BorderRadius.circular(
                                                          8),
                                                ),
                                              ),
                                              child: Text(
                                                course.retakeStatus,
                                                style: TextStyle(
                                                  fontSize: 11,
                                                  color: scheme
                                                      .onPrimaryContainer,
                                                ),
                                              ),
                                            ),
                                    ),
                                  );
                                },
                              ),
          ),
          if (filtered != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 6, top: 2),
              child: Text(
                '共 ${filtered.length} 条（全库 ${all!.length} 条）',
                style: TextStyle(
                  fontSize: 12,
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

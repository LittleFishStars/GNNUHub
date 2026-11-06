/// 课表页：学期选择 + 周次切换 + 周网格视图
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import '../state/term_store.dart';
import 'widgets/panels.dart';

/// 课程名 → 稳定色相（同课同色）
Color courseColor(String name, BuildContext context) {
  final scheme = Theme.of(context).colorScheme;
  var hash = 0;
  for (final rune in name.runes) {
    hash = (hash * 31 + rune) & 0x7FFFFFFF;
  }
  final hue = (hash % 360).toDouble();
  return HSLColor.fromColor(scheme.primary)
      .withHue(hue)
      .withSaturation(0.38)
      .withLightness(
          Theme.of(context).brightness == Brightness.light ? 0.86 : 0.28)
      .toColor();
}

class SchedulePage extends StatefulWidget {
  const SchedulePage({super.key});

  @override
  State<SchedulePage> createState() => _SchedulePageState();
}

class _SchedulePageState extends State<SchedulePage> {
  int _year = DateTime.now().month >= 8
      ? DateTime.now().year
      : DateTime.now().year - 1;
  Term _term = DateTime.now().month >= 8 || DateTime.now().month <= 1
      ? Term.first
      : Term.second;
  int? _week;
  ClassSchedule? _schedule;
  Object? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    TermStore.loadTerm().then((saved) {
      if (saved != null && mounted) {
        setState(() {
          _year = saved.year;
          _term = saved.termIndex == 0 ? Term.first : Term.second;
        });
      }
      _load();
    });
  }

  Future<void> _load() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final service = context.read<HubService>();
      final schedule = await service.classSchedule(
        AcademicTerm(year: _year, term: _term),
        week: _week,
      );
      if (mounted) setState(() => _schedule = schedule);
      await TermStore.saveTerm(
          year: _year, termIndex: _term == Term.first ? 0 : 1);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _gotoThisWeek() async {
    try {
      final service = context.read<HubService>();
      final week = await service.thisWeek();
      if (!mounted) return;
      setState(() => _week = week);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(errorText(e))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final schedule = _schedule;
    final view = schedule == null
        ? null
        : (_week == null ? schedule : schedule.weekView(_week!));

    return Scaffold(
      appBar: AppBar(
        title: const Text('课表'),
        actions: [
          // 当前周
          IconButton(
            tooltip: '回到当前周',
            icon: const Icon(LucideIcons.crosshair, size: 18),
            onPressed: _busy ? null : _gotoThisWeek,
          ),
          IconButton(
            tooltip: '查询',
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(LucideIcons.search, size: 18),
            onPressed: _busy ? null : _load,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: Column(
        children: [
          // 选择器行（填充色阶，无边框）
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 4, 14, 8),
            child: Row(
              children: [
                _Selector(
                  value: _year,
                  items: [
                    for (var y = DateTime.now().year + 1; y >= 2015; y--) y,
                  ],
                  label: (y) => '$y-${y + 1}',
                  onChanged: (v) => setState(() => _year = v),
                ),
                const SizedBox(width: 8),
                _Selector<Term>(
                  value: _term,
                  items: Term.values,
                  label: (t) => t.label,
                  onChanged: (v) => setState(() => _term = v),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _Selector<int?>(
                    value: _week,
                    items: [null, for (var w = 1; w <= 25; w++) w],
                    label: (w) => w == null ? '整学期' : '第 $w 周',
                    onChanged: (v) => setState(() => _week = v),
                  ),
                ),
              ],
            ),
          ),
          Expanded(
            child: _error != null
                ? ErrorPanel(
                    message: errorText(_error!),
                    onRetry: _load,
                  )
                : _busy && view == null
                    ? const LoadingPanel(label: '查询课表中…')
                    : view == null
                        ? const EmptyPanel()
                        : view.courses.isEmpty
                            ? const EmptyPanel(
                                icon: LucideIcons.calendarX2,
                                label: '该学期没有课程')
                            : LayoutBuilder(
                                builder: (context, constraints) {
                                  return _WeekGrid(
                                    view: view,
                                    week: _week,
                                    maxWidth: constraints.maxWidth,
                                  );
                                },
                              ),
          ),
          if (view != null)
            Padding(
              padding: EdgeInsets.only(bottom: MediaQuery
                      .of(context)
                      .padding
                      .bottom + 8),
              child: Text(
                '${view.courseCount} 门课程 · $_year-${_year + 1} ${_term.label}'
                '${_week == null ? '' : ' · 第 $_week 周'}',
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

/// 填充式下拉选择（无边框）
class _Selector<T> extends StatelessWidget {
  const _Selector({
    required this.value,
    required this.items,
    required this.label,
    required this.onChanged,
  });

  final T value;
  final List<T> items;
  final String Function(T) label;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return PopupMenuButton<T>(
      initialValue: value,
      onSelected: onChanged,
      constraints: const BoxConstraints(maxHeight: 420, maxWidth: 220),
      itemBuilder: (context) => [
        for (final item in items)
          PopupMenuItem<T>(
            value: item,
            height: 42,
            child: Text(label(item), style: const TextStyle(fontSize: 14)),
          ),
      ],
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        decoration: ShapeDecoration(
          color: Theme.of(context).brightness == Brightness.light
              ? const Color(0xFFF1F4F8)
              : scheme.surfaceContainerHigh,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Flexible(
              child: Text(
                label(value),
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 13.5),
              ),
            ),
            const SizedBox(width: 4),
            Icon(LucideIcons.chevronDown,
                size: 15, color: scheme.onSurfaceVariant),
          ],
        ),
      ),
    );
  }
}

/// 周网格：行=节次，列=星期
class _WeekGrid extends StatelessWidget {
  const _WeekGrid({
    required this.view,
    required this.week,
    required this.maxWidth,
  });

  final ClassSchedule view;
  final int? week;
  final double maxWidth;

  static const _weekdayNames = ['一', '二', '三', '四', '五', '六', '日'];

  @override
  Widget build(BuildContext context) {
    final compact = maxWidth < 560;
    // 收集全部排课，按 (weekday, period.start) 索引
    final cellEntries = <int, List<(CourseEntry, String)>>{};
    for (final entry in view.courses.entries) {
      for (final course in entry.value) {
        final weekday =
            _weekdayNames.indexOf(course.time.weekday.replaceAll('星期', ''));
        final slot = course.time.periods.start;
        cellEntries
            .putIfAbsent((weekday + 1) * 100 + slot, () => [])
            .add((course, entry.key));
      }
    }

    final maxPeriod = cellEntries.keys
        .map((k) => k % 100)
        .fold(8, (a, b) => a > b ? a : b);

    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 14),
      child: SingleChildScrollView(
        child: Column(
          children: [
            // 表头
            Row(
              children: [
                _cell('节', compact, header: true),
                for (var d = 0; d < 7; d++)
                  _cell('周${_weekdayNames[d]}', compact, header: true),
              ],
            ),
            for (var p = 1; p <= maxPeriod; p++)
              Row(
                children: [
                  _cell('$p', compact, header: true),
                  for (var d = 1; d <= 7; d++)
                    _cellEntries(cellEntries[d * 100 + p], compact, context),
                ],
              ),
          ],
        ),
      ),
    );
  }

  Widget _cell(String text, bool compact, {bool header = false}) {
    final scheme = header ? null : null;
    return Container(
      width: compact ? 34 : 44,
      height: compact ? 40 : 64,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: scheme,
      ),
      child: Text(
        text,
        style: TextStyle(
          fontSize: compact ? 11 : 12.5,
          fontWeight: header ? FontWeight.w600 : FontWeight.w500,
        ),
      ),
    );
  }

  Widget _cellEntries(
      List<(CourseEntry, String)>? entries, bool compact, BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    if (entries == null || entries.isEmpty) {
      return Container(
        width: compact ? 52 : 120,
        height: compact ? 40 : 64,
        margin: const EdgeInsets.all(1.5),
        decoration: ShapeDecoration(
          color: Theme.of(context).brightness == Brightness.light
              ? const Color(0xFFF3F5F9)
              : scheme.surfaceContainerLowest,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
          ),
        ),
      );
    }
    final children = <Widget>[];
    for (final (course, name) in entries) {
      final color = courseColor(name, context);
      children.add(Container(
        width: compact ? 52 : 120,
        height: compact ? 40 : 64,
        margin: const EdgeInsets.all(1.5),
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 3),
        decoration: ShapeDecoration(
          color: color,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
          ),
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(
              name,
              maxLines: compact ? 1 : 2,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.center,
              style: TextStyle(
                fontSize: compact ? 10.5 : 12,
                fontWeight: FontWeight.w600,
                height: 1.15,
              ),
            ),
            if (!compact) ...[
              const SizedBox(height: 2),
              Text(
                [
                  if (course.position.isNotEmpty) course.position,
                  if (course.teachers.isNotEmpty) course.teachers.first,
                ].join(' · '),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 10,
                  color: Colors.black.withValues(alpha: 0.55),
                ),
              ),
            ],
          ],
        ),
      ));
    }
    return Container(
      width: compact ? 52 : 120,
      height: compact ? 40 : 64,
      margin: const EdgeInsets.all(1.5),
      child: Column(children: children),
    );
  }
}

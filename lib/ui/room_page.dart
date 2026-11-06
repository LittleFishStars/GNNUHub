/// 空闲教室（可借用场地）查询页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import 'widgets/panels.dart';

/// 楼栋候选（接口原文 lh；空串表示全部）
const _buildings = <(String, String)>[
  ('全部', ''),
  ('第五教学楼', '5'),
  ('第四教学楼', '4'),
  ('第三教学楼', '3'),
  ('第二教学楼', '2'),
  ('第一教学楼', '1'),
];

/// 场地类别候选（cdlb_id）
const _categories = <(String, String)>[
  ('全部', ''),
  ('多媒体教室', '1'),
  ('实验室', '2'),
  ('运动场', '3'),
  ('报告厅', '4'),
];

class RoomPage extends StatefulWidget {
  const RoomPage({super.key});

  @override
  State<RoomPage> createState() => _RoomPageState();
}

class _RoomPageState extends State<RoomPage> {
  final _year = DateTime.now().year;
  final Term _term =
      DateTime.now().month >= 8 || DateTime.now().month <= 1
          ? Term.first
          : Term.second;

  DateTime _start = DateTime.now();
  DateTime _end = DateTime.now();
  TimeOfDay _startPeriod = const TimeOfDay(hour: 8, minute: 0);
  TimeOfDay _endPeriod = const TimeOfDay(hour: 21, minute: 50);
  String _building = '';
  String _category = '';

  List<FreeRoom>? _rooms;
  Object? _error;
  bool _busy = false;

  String _d(DateTime d) =>
      '${d.year}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

  String _t(TimeOfDay t) =>
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}:00';

  Future<void> _pickDate({required bool isStart}) async {
    final picked = await showDatePicker(
      context: context,
      initialDate: isStart ? _start : _end,
      firstDate: DateTime.now().subtract(const Duration(days: 60)),
      lastDate: DateTime.now().add(const Duration(days: 120)),
    );
    if (picked != null) {
      setState(() => isStart ? _start = picked : _end = picked);
    }
  }

  Future<void> _pickPeriod({required bool isStart}) async {
    final picked = await showTimePicker(
      context: context,
      initialTime: isStart ? _startPeriod : _endPeriod,
    );
    if (picked != null) {
      setState(() => isStart ? _startPeriod = picked : _endPeriod = picked);
    }
  }

  Future<void> _search() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final rooms = await context.read<HubService>().freeRooms(
            term: AcademicTerm(year: _year, term: _term),
            qssj: _d(_start),
            jssj: _d(_end),
            qssd: _t(_startPeriod),
            jssd: _t(_endPeriod),
            lh: _building,
            cdlbId: _category,
          );
      if (mounted) setState(() => _rooms = rooms);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void initState() {
    super.initState();
    _search();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final rooms = _rooms;
    final wide = MediaQuery.sizeOf(context).width >= 640;

    return Scaffold(
      appBar: AppBar(
        title: const Text('空闲教室'),
        actions: [
          IconButton(
            tooltip: '查询',
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(LucideIcons.search, size: 18),
            onPressed: _busy ? null : _search,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 4, 14, 8),
            child: Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                _ChipButton(
                  icon: LucideIcons.calendarDays,
                  label: '${_d(_start)} ~ ${_d(_end)}',
                  onTap: () => _pickDate(isStart: true),
                ),
                _ChipButton(
                  icon: LucideIcons.clock3,
                  label: '${_t(_startPeriod).substring(0, 5)} ~ '
                      '${_t(_endPeriod).substring(0, 5)}',
                  onTap: () => _pickPeriod(isStart: true),
                ),
                _ChipSelect(
                  value: _building,
                  options: _buildings,
                  icon: LucideIcons.building2,
                  onChanged: (v) => setState(() => _building = v),
                ),
                _ChipSelect(
                  value: _category,
                  options: _categories,
                  icon: LucideIcons.tag,
                  onChanged: (v) => setState(() => _category = v),
                ),
              ],
            ),
          ),
          Expanded(
            child: _error != null
                ? ErrorPanel(message: errorText(_error!), onRetry: _search)
                : _busy && rooms == null
                    ? const LoadingPanel(label: '查询空闲教室…')
                    : rooms == null
                        ? const EmptyPanel()
                        : rooms.isEmpty
                            ? const EmptyPanel(
                                icon: LucideIcons.doorClosed,
                                label: '没有符合条件的空闲场地')
                            : LayoutBuilder(
                                builder: (context, constraints) {
                                  final columns = wide
                                      ? (constraints.maxWidth ~/ 300)
                                          .clamp(2, 4)
                                      : 1;
                                  return GridView.builder(
                                    padding: const EdgeInsets.symmetric(
                                        horizontal: 14),
                                    gridDelegate:
                                        SliverGridDelegateWithFixedCrossAxisCount(
                                      crossAxisCount: columns,
                                      mainAxisSpacing: 10,
                                      crossAxisSpacing: 10,
                                      childAspectRatio: wide ? 2.6 : 3.4,
                                    ),
                                    itemCount: rooms.length,
                                    itemBuilder: (context, i) {
                                      final room = rooms[i];
                                      return Card(
                                        child: Padding(
                                          padding: const EdgeInsets.all(12),
                                          child: Column(
                                            crossAxisAlignment:
                                                CrossAxisAlignment.start,
                                            children: [
                                              Row(
                                                children: [
                                                  Expanded(
                                                    child: Text(
                                                      room.name,
                                                      maxLines: 1,
                                                      overflow:
                                                          TextOverflow.ellipsis,
                                                      style: const TextStyle(
                                                        fontSize: 14.5,
                                                        fontWeight:
                                                            FontWeight.w600,
                                                      ),
                                                    ),
                                                  ),
                                                  if (room.seats > 0)
                                                    Row(
                                                      mainAxisSize:
                                                          MainAxisSize.min,
                                                      children: [
                                                        Icon(
                                                          LucideIcons.users,
                                                          size: 13,
                                                          color: scheme
                                                              .onSurfaceVariant,
                                                        ),
                                                        const SizedBox(
                                                            width: 3),
                                                        Text(
                                                          '${room.seats}',
                                                          style: TextStyle(
                                                            fontSize: 12,
                                                            color: scheme
                                                                .onSurfaceVariant,
                                                          ),
                                                        ),
                                                      ],
                                                    ),
                                                ],
                                              ),
                                              const SizedBox(height: 4),
                                              Text(
                                                [
                                                  if (room.category.isNotEmpty)
                                                    room.category,
                                                  if (room.building
                                                      .isNotEmpty)
                                                    room.building,
                                                  if (room.campus.isNotEmpty)
                                                    room.campus,
                                                ].join(' · '),
                                                maxLines: 1,
                                                overflow: TextOverflow.ellipsis,
                                                style: TextStyle(
                                                  fontSize: 11.5,
                                                  color:
                                                      scheme.onSurfaceVariant,
                                                ),
                                              ),
                                            ],
                                          ),
                                        ),
                                      );
                                    },
                                  );
                                },
                              ),
          ),
          if (rooms != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 6, top: 2),
              child: Text(
                '${_d(_start)} ~ ${_d(_end)}：共 ${rooms.length} 间',
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

/// 填充式条件按钮
class _ChipButton extends StatelessWidget {
  const _ChipButton({
    required this.icon,
    required this.label,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: Theme.of(context).brightness == Brightness.light
          ? const Color(0xFFF1F4F8)
          : scheme.surfaceContainerHigh,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 15, color: scheme.onSurfaceVariant),
              const SizedBox(width: 6),
              Text(label, style: const TextStyle(fontSize: 13)),
            ],
          ),
        ),
      ),
    );
  }
}

/// 填充式选择按钮
class _ChipSelect extends StatelessWidget {
  const _ChipSelect({
    required this.value,
    required this.options,
    required this.icon,
    required this.onChanged,
  });

  final String value;
  final List<(String, String)> options;
  final IconData icon;
  final ValueChanged<String> onChanged;

  String _labelFor(String value) =>
      options.where((o) => o.$2 == value).map((o) => o.$1).firstOrNull ??
      '全部';

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return PopupMenuButton<String>(
      initialValue: value,
      onSelected: onChanged,
      itemBuilder: (context) => [
        for (final (label, v) in options)
          PopupMenuItem<String>(
            value: v,
            height: 42,
            child: Text(label, style: const TextStyle(fontSize: 14)),
          ),
      ],
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
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
            Icon(icon, size: 15, color: scheme.onSurfaceVariant),
            const SizedBox(width: 6),
            Text(_labelFor(value), style: const TextStyle(fontSize: 13)),
            const SizedBox(width: 3),
            Icon(LucideIcons.chevronDown,
                size: 14, color: scheme.onSurfaceVariant),
          ],
        ),
      ),
    );
  }
}

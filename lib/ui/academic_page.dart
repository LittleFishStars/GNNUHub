/// 学业页（GPA）+ 学籍页 + 空闲教室页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import 'home_page.dart';

// ---------------------------------------------------------------------------
// 学业（GPA）
// ---------------------------------------------------------------------------

typedef AcademicData = (List<GpaSummary>, AcademicCalendar?);

class AcademicPage extends StatelessWidget {
  const AcademicPage({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return AsyncPage<AcademicData>(
      title: '学业情况',
      emptyIcon: LucideIcons.chartNoAxesColumn,
      emptyLabel: '没有学业数据',
      load: () async {
        final service = context.read<HubService>();
        final gpa = await service.gpaSummary();
        AcademicCalendar? calendar;
        try {
          calendar = await service.academicCalendar();
        } catch (_) {
          // 校历失败不阻塞 GPA 展示
        }
        return (gpa, calendar);
      },
      builder: (context, data) {
        final (items, calendar) = data;
        final overall = GpaSummary.overallGpa(items);
        final totalCredit = GpaSummary.totalCredit(items);
        final maxGpa = items
            .map((i) => i.gpa)
            .fold(0.001, (a, b) => a > b ? a : b);

        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // 总览卡片
            Row(
              children: [
                Expanded(
                  child: _StatCard(
                    icon: LucideIcons.medal,
                    label: '加权平均绩点',
                    value: overall.toStringAsFixed(3),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: _StatCard(
                    icon: LucideIcons.layers,
                    label: '已获得学分',
                    value: totalCredit.toStringAsFixed(1),
                  ),
                ),
              ],
            ),
            if (calendar != null)
              Padding(
                padding: const EdgeInsets.only(top: 10),
                child: Card(
                  child: Padding(
                    padding: const EdgeInsets.all(14),
                    child: Row(
                      children: [
                        Icon(LucideIcons.calendarRange,
                            size: 16, color: scheme.primary),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            '${calendar.year}-${calendar.year + 1} 学年 '
                            '第 ${calendar.term} 学期：'
                            '${calendar.startDate} 至 ${calendar.endDate}',
                            style: const TextStyle(fontSize: 13),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            const SizedBox(height: 14),
            ...[
              for (final item in items)
                Card(
                  margin: const EdgeInsets.only(bottom: 10),
                  child: Padding(
                    padding: const EdgeInsets.all(14),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                item.natureName.isEmpty
                                    ? '课程性质 ${item.natureCode}'
                                    : item.natureName,
                                style: const TextStyle(
                                  fontSize: 14,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            Text(
                              'GPA ${item.gpa.toStringAsFixed(2)}',
                              style: TextStyle(
                                fontSize: 13.5,
                                fontWeight: FontWeight.w700,
                                color: scheme.primary,
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 10),
                        ClipRRect(
                          borderRadius: BorderRadius.circular(6),
                          child: LinearProgressIndicator(
                            value: (item.gpa / (maxGpa > 4 ? maxGpa : 4))
                                .clamp(0.0, 1.0),
                            minHeight: 7,
                          ),
                        ),
                        const SizedBox(height: 8),
                        Text(
                          '已获得学分 ${item.earnedCredit.toStringAsFixed(1)}',
                          style: TextStyle(
                            fontSize: 12,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
            ],
          ],
        );
      },
    );
  }
}

class _StatCard extends StatelessWidget {
  const _StatCard({
    required this.icon,
    required this.label,
    required this.value,
  });

  final IconData icon;
  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(icon, size: 16, color: scheme.primary),
                const SizedBox(width: 6),
                Text(
                  label,
                  style: TextStyle(
                    fontSize: 12.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              value,
              style: const TextStyle(
                fontSize: 24,
                fontWeight: FontWeight.w700,
                fontFeatures: [],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// 学籍
// ---------------------------------------------------------------------------

class ProfilePage extends StatelessWidget {
  const ProfilePage({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return AsyncPage<StudentInfo>(
      title: '学籍信息',
      load: () => context.read<HubService>().studentInfo(),
      builder: (context, info) {
        return Card(
          child: Padding(
            padding: const EdgeInsets.all(18),
            child: Column(
              children: [
                Row(
                  children: [
                    Container(
                      width: 54,
                      height: 54,
                      alignment: Alignment.center,
                      decoration: ShapeDecoration(
                        color: scheme.primaryContainer,
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(16),
                        ),
                      ),
                      child: Text(
                        (info.name ?? '?').characters.first,
                        style: TextStyle(
                          fontSize: 20,
                          fontWeight: FontWeight.w700,
                          color: scheme.onPrimaryContainer,
                        ),
                      ),
                    ),
                    const SizedBox(width: 14),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            info.name ?? '未知',
                            style: const TextStyle(
                              fontSize: 17,
                              fontWeight: FontWeight.w700,
                            ),
                          ),
                          Text(
                            [
                              info.studentId ?? '',
                              if (info.identity != null) info.identity!,
                            ].join(' · '),
                            style: TextStyle(
                              fontSize: 12.5,
                              color: scheme.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
                const Divider(height: 26),
                _Field(label: '学院', value: info.college),
                _Field(label: '专业', value: info.major),
                _Field(label: '班级', value: info.className),
                _Field(label: '性别', value: info.gender),
                _Field(label: '民族', value: info.ethnicity),
                _Field(label: '政治面貌', value: info.politicalStatus),
                _Field(label: '出生日期', value: info.birthday),
                _Field(label: '入学年份', value: info.enrollmentYear),
                if (info.document != null)
                  _Field(
                    label: info.document!.kind.isEmpty
                        ? '证件'
                        : info.document!.kind,
                    value: info.document!.number.isEmpty
                        ? null
                        : info.document!.number,
                  ),
                _Field(label: '辅导员', value: info.instructor),
                _Field(label: '联系地址', value: info.address),
              ],
            ),
          ),
        );
      },
    );
  }
}

class _Field extends StatelessWidget {
  const _Field({required this.label, required this.value});

  final String label;
  final String? value;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 76,
            child: Text(
              label,
              style: TextStyle(
                fontSize: 13,
                color: scheme.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(
            child: Text(
              value ?? '—',
              style: TextStyle(
                fontSize: 13.5,
                color: value == null
                    ? scheme.onSurfaceVariant.withValues(alpha: 0.55)
                    : scheme.onSurface,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

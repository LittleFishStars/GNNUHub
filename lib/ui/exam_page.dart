/// 考试安排页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import 'home_page.dart';

class ExamPage extends StatelessWidget {
  const ExamPage({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final year = DateTime.now().month >= 8
        ? DateTime.now().year
        : DateTime.now().year - 1;

    return AsyncPage<List<ExamRecord>>(
      title: '考试安排',
      emptyIcon: LucideIcons.clipboardX,
      emptyLabel: '没有考试记录',
      load: () => context.read<HubService>().examSchedule(
            AcademicTerm.first(year),
          ),
      builder: (context, exams) {
        // 服务端返回全量记录，按时间倒序展示
        final sorted = [...exams]..sort((a, b) => b.examTime.compareTo(a.examTime));
        return Column(
          children: [
            for (final exam in sorted)
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
                              exam.courseName,
                              style: const TextStyle(
                                fontSize: 15,
                                fontWeight: FontWeight.w600,
                              ),
                            ),
                          ),
                          if (exam.isMakeUp)
                            Container(
                              padding: const EdgeInsets.symmetric(
                                  horizontal: 8, vertical: 3),
                              decoration: ShapeDecoration(
                                color: scheme.errorContainer,
                                shape: RoundedRectangleBorder(
                                  borderRadius: BorderRadius.circular(8),
                                ),
                              ),
                              child: Text(
                                '补考',
                                style: TextStyle(
                                  fontSize: 11,
                                  color: scheme.onErrorContainer,
                                ),
                              ),
                            ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      _Row(
                        icon: LucideIcons.clock3,
                        text: exam.examTime.isEmpty ? '时间待定' : exam.examTime,
                      ),
                      if (exam.location.isNotEmpty)
                        _Row(icon: LucideIcons.mapPin, text: exam.location),
                      if (exam.examMode.isNotEmpty || exam.assessment.isNotEmpty)
                        _Row(
                          icon: LucideIcons.fileText,
                          text: [
                            if (exam.examMode.isNotEmpty) exam.examMode,
                            if (exam.assessment.isNotEmpty) exam.assessment,
                          ].join(' · '),
                        ),
                      if (exam.teacher.isNotEmpty)
                        _Row(icon: LucideIcons.userRound, text: exam.teacher),
                    ],
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(top: 3),
      child: Row(
        children: [
          Icon(icon, size: 14, color: scheme.onSurfaceVariant),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              text,
              style: TextStyle(
                fontSize: 13,
                color: scheme.onSurfaceVariant,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

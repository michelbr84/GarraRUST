import 'package:flutter/material.dart';

import '../widgets/garra_bottom_nav.dart';
import '../widgets/garra_page.dart';

/// Notifications tab. Local notifications are wired
/// (`services/notification_service.dart`, channels `chat_messages`,
/// `sync_status`, `system`) but nothing on the runtime pushes to the phone
/// yet — that arrives with automations. Empty state says exactly that.
class NotificationsScreen extends StatelessWidget {
  const NotificationsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return const GarraPage(
      title: 'Notifications',
      bottomNavigationBar: GarraBottomNav(current: GarraTab.notifications),
      body: EmptyState(
        icon: Icons.notifications_none_rounded,
        text:
            'Nothing yet. Notifications from automations and background agents will show up here.',
      ),
    );
  }
}

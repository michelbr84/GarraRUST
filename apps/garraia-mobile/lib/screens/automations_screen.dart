import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../widgets/garra_page.dart';

/// Set up smart workflows.
///
/// Honest state for v0.4.0: the gateway exposes no scheduling / cron REST
/// surface yet (the two-tier scheduler of ADR 0013 lives behind the CLI), so
/// `GET /api/capabilities` never lists `automations` and this screen says
/// so. When the feature ships server-side the tile lights up on its own —
/// no app update needed for the negotiation, only for the editor UI.
class AutomationsScreen extends ConsumerWidget {
  const AutomationsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final caps =
        ref.watch(runtimeCapabilitiesProvider).value ?? GarraCapabilities.empty;
    final available = caps.has(GarraFeature.automations);

    return GarraPage(
      title: l10n.automationsTitle,
      body: available
          ? EmptyState(
              icon: Icons.settings_suggest_outlined,
              text: l10n.automationsEditorComingSoon,
            )
          : UnavailableFeature(
              feature: l10n.automationsTitle,
              why: l10n.automationsUnavailableWhy,
            ),
    );
  }
}

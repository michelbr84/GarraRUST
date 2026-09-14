import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../l10n/l10n.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';

/// Shared scaffold for the feature screens: transparent app bar, dark
/// background, optional bottom nav.
class GarraPage extends StatelessWidget {
  final String title;
  final Widget body;
  final List<Widget> actions;
  final Widget? bottomNavigationBar;

  const GarraPage({
    super.key,
    required this.title,
    required this.body,
    this.actions = const [],
    this.bottomNavigationBar,
  });

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: GarraColors.bg,
      appBar: AppBar(title: Text(title), actions: actions),
      body: body,
      bottomNavigationBar: bottomNavigationBar,
    );
  }
}

/// Renders an [AsyncValue] with the house loading / error / empty states.
class AsyncBody<T> extends StatelessWidget {
  final AsyncValue<T> value;
  final Widget Function(T data) builder;

  /// Copy for the empty state; `null` falls back to the localized
  /// "Nothing here yet." (a const constructor cannot read `context.l10n`).
  final String? emptyText;
  final bool Function(T data)? isEmpty;
  final VoidCallback? onRetry;

  const AsyncBody({
    super.key,
    required this.value,
    required this.builder,
    this.emptyText,
    this.isEmpty,
    this.onRetry,
  });

  @override
  Widget build(BuildContext context) {
    return value.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) =>
          ErrorState(message: describeError(context.l10n, e), onRetry: onRetry),
      data: (d) {
        final empty = isEmpty?.call(d) ?? (d is List && d.isEmpty);
        if (empty) {
          return EmptyState(
            text: emptyText ?? context.l10n.commonEmptyNothingYet,
          );
        }
        return builder(d);
      },
    );
  }
}

class EmptyState extends StatelessWidget {
  final String text;
  final IconData icon;
  const EmptyState({
    super.key,
    required this.text,
    this.icon = Icons.inbox_outlined,
  });

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 44, color: GarraColors.textDim),
            const SizedBox(height: 12),
            Text(
              text,
              textAlign: TextAlign.center,
              style: garraText(
                size: 14,
                color: GarraColors.textMuted,
                height: 1.4,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ErrorState extends StatelessWidget {
  final String message;
  final VoidCallback? onRetry;
  const ErrorState({super.key, required this.message, this.onRetry});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(28),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(
              Icons.error_outline_rounded,
              size: 44,
              color: GarraColors.danger,
            ),
            const SizedBox(height: 12),
            Text(
              context.l10n.errorCouldNotReachRuntime,
              style: garraText(size: 16, weight: FontWeight.w700),
            ),
            const SizedBox(height: 6),
            Text(
              message,
              textAlign: TextAlign.center,
              style: garraText(size: 12.5, color: GarraColors.textMuted),
            ),
            if (onRetry != null) ...[
              const SizedBox(height: 16),
              OutlinedButton(
                onPressed: onRetry,
                child: Text(context.l10n.commonRetry),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Explains a feature the connected runtime does not expose. Used by the
/// Automations screen today and by any tile whose capability is missing.
class UnavailableFeature extends StatelessWidget {
  final String feature;
  final String why;
  const UnavailableFeature({
    super.key,
    required this.feature,
    required this.why,
  });

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(28),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(
              Icons.construction_rounded,
              size: 44,
              color: GarraColors.warn,
            ),
            const SizedBox(height: 12),
            Text(
              context.l10n.commonFeatureUnavailableOnRuntime(feature),
              textAlign: TextAlign.center,
              style: garraText(size: 16, weight: FontWeight.w700),
            ),
            const SizedBox(height: 8),
            Text(
              why,
              textAlign: TextAlign.center,
              style: garraText(
                size: 13,
                color: GarraColors.textMuted,
                height: 1.4,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Simple list row with a leading icon, title, subtitle and optional trailing.
class GarraListTile extends StatelessWidget {
  final IconData icon;
  final Color iconColor;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final VoidCallback? onTap;

  const GarraListTile({
    super.key,
    required this.icon,
    required this.title,
    this.iconColor = GarraColors.violetLight,
    this.subtitle,
    this.trailing,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 5),
      child: ListTile(
        onTap: onTap,
        leading: Icon(icon, color: iconColor),
        title: Text(title, style: garraText(size: 15, weight: FontWeight.w600)),
        subtitle: subtitle == null
            ? null
            : Text(
                subtitle!,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: garraText(size: 12.5, color: GarraColors.textMuted),
              ),
        trailing: trailing,
      ),
    );
  }
}

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../l10n/l10n.dart';
import '../providers/auth_provider.dart';
import '../providers/chat_provider.dart';
import '../providers/streaming_reply_provider.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../services/offline_queue.dart';
import '../widgets/chat_bubble.dart';
import '../widgets/mascot_widget.dart';
import '../widgets/queue_status_indicator.dart';
import '../widgets/scroll_to_bottom_button.dart';
import '../widgets/slash_suggestions.dart';
import '../widgets/streaming_bubble.dart';
import '../widgets/typing_indicator.dart';
import '../widgets/voice_input_widget.dart';

class ChatScreen extends ConsumerStatefulWidget {
  /// Gateway session to open (deep link `garraia://chat/<id>`); null keeps
  /// the persisted current session.
  final String? sessionId;

  /// Text to start the input with (`/chat?draft=/help `), from Skills.
  final String? initialDraft;

  const ChatScreen({super.key, this.sessionId, this.initialDraft});

  @override
  ConsumerState<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends ConsumerState<ChatScreen>
    with WidgetsBindingObserver {
  final _inputCtrl = TextEditingController();
  final _scrollCtrl = ScrollController();
  bool _sending = false;
  bool _showScrollToBottom = false;
  bool _showVoiceInput = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _scrollCtrl.addListener(_onScroll);
    final draft = widget.initialDraft;
    if (draft != null && draft.isNotEmpty) {
      _inputCtrl.value = TextEditingValue(
        text: draft,
        selection: TextSelection.collapsed(offset: draft.length),
      );
    }
    final id = widget.sessionId;
    if (id != null && id.isNotEmpty) {
      Future.microtask(
        () => ref.read(currentSessionProvider.notifier).select(id),
      );
    }
  }

  @override
  void didUpdateWidget(ChatScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    // `/chat?draft=a` then `/chat?draft=b` reuses this State: without this,
    // the second draft was dropped on the floor (review of #1040).
    final draft = widget.initialDraft;
    if (draft != null && draft.isNotEmpty && draft != oldWidget.initialDraft) {
      _inputCtrl.value = TextEditingValue(
        text: draft,
        selection: TextSelection.collapsed(offset: draft.length),
      );
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _inputCtrl.dispose();
    _scrollCtrl.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      // Flush offline queue on app resume
      ref.read(offlineQueueProvider).onAppResume();
    }
  }

  void _onScroll() {
    if (!_scrollCtrl.hasClients) return;
    final isAtBottom =
        _scrollCtrl.position.pixels >=
        _scrollCtrl.position.maxScrollExtent - 100;
    if (_showScrollToBottom == isAtBottom) {
      setState(() => _showScrollToBottom = !isAtBottom);
    }
  }

  Future<void> _stop() async {
    await ref.read(chatMessagesProvider.notifier).stop();
  }

  Future<void> _send() async {
    final text = _inputCtrl.text.trim();
    if (text.isEmpty || _sending) return;
    _inputCtrl.clear();
    setState(() => _sending = true);
    try {
      await ref.read(chatMessagesProvider.notifier).send(text);
      _scrollToBottom();
    } catch (e) {
      if (mounted) {
        // If sending failed due to network, queue it offline
        final queueStatus = ref.read(queueStatusProvider);
        if (!queueStatus.isOnline) {
          await ref.read(offlineQueueProvider).enqueue(text);
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text(context.l10n.chatMessageQueuedOffline)),
            );
          }
        } else {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text(
                  context.l10n.commonErrorWithDetail(
                    describeError(context.l10n, e),
                  ),
                ),
              ),
            );
          }
        }
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollCtrl.hasClients) {
        _scrollCtrl.animateTo(
          _scrollCtrl.position.maxScrollExtent,
          duration: const Duration(milliseconds: 300),
          curve: Curves.easeOut,
        );
      }
    });
  }

  Future<String> _handleAudioRecorded(String audioPath) async {
    // Resolved before the await: the widget may be gone by the time the
    // transcription fails, and `context` would be unusable then.
    final l10n = context.l10n;
    try {
      final conn = ref.read(garraConnectionProvider);
      if (conn == null) return l10n.chatVoiceNoRuntimeConfigured;
      return await conn.transcribe(audioPath);
    } catch (_) {
      return l10n.chatVoiceTranscribeError;
    }
  }

  @override
  Widget build(BuildContext context) {
    final messages = ref.watch(chatMessagesProvider);
    final mascotState = ref.watch(mascotStateProvider);
    final isThinking = mascotState == MascotState.thinking;
    // Deliberately `select`ed down to the boolean: watching the whole
    // streaming state here would rebuild the screen — and the message list
    // with it — on every token.
    final streaming = ref.watch(
      streamingReplyStateProvider.select((s) => s.active),
    );
    final isCloud =
        ref.watch(runtimeConfigStateProvider).value?.mode == RuntimeMode.cloud;
    final l10n = context.l10n;

    return Scaffold(
      appBar: AppBar(
        title: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            MascotWidget(size: 32, state: mascotState),
            const SizedBox(width: 10),
            Text(l10n.commonBrand),
          ],
        ),
        centerTitle: true,
        actions: [
          IconButton(
            icon: const Icon(Icons.devices_rounded),
            tooltip: l10n.chatPairDevicesTooltip,
            onPressed: () => context.push('/pair'),
          ),
          IconButton(
            // Plan 0029 / GAR-358 — dedicated Settings screen entry point.
            icon: const Icon(Icons.settings_rounded),
            tooltip: l10n.settingsTitle,
            onPressed: () => context.push('/settings'),
          ),
          PopupMenuButton<String>(
            onSelected: (v) async {
              if (v == 'new') {
                await ref.read(currentSessionProvider.notifier).reset();
                ref.invalidate(chatMessagesProvider);
              } else if (v == 'logout') {
                await ref.read(authStateProvider.notifier).logout();
                if (context.mounted) context.go('/login');
              }
            },
            itemBuilder: (_) => [
              if (!isCloud)
                PopupMenuItem(
                  value: 'new',
                  child: Row(
                    children: [
                      const Icon(Icons.add_comment_outlined),
                      const SizedBox(width: 8),
                      Text(l10n.chatNewSession),
                    ],
                  ),
                ),
              if (isCloud)
                PopupMenuItem(
                  value: 'logout',
                  child: Row(
                    children: [
                      const Icon(Icons.logout),
                      const SizedBox(width: 8),
                      Text(l10n.commonLogout),
                    ],
                  ),
                ),
            ],
          ),
        ],
      ),
      body: Column(
        children: [
          // Offline queue status indicator
          const QueueStatusIndicator(),

          // Message list
          Expanded(
            child: messages.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (e, _) => Center(
                child: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Text(
                    l10n.chatLoadConversationError(describeError(l10n, e)),
                    textAlign: TextAlign.center,
                  ),
                ),
              ),
              data: (msgs) => msgs.isEmpty && !isThinking && !streaming
                  ? _EmptyChat(
                      onPrompt: (p) {
                        _inputCtrl.text = p;
                        _send();
                      },
                    )
                  : Stack(
                      children: [
                        ListView.builder(
                          controller: _scrollCtrl,
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 8,
                          ),
                          // One trailing slot for the turn in flight: the
                          // streaming bubble when the socket is carrying it,
                          // the old typing dots when the reply comes over
                          // POST (cloud, or the fallback).
                          itemCount:
                              msgs.length + (streaming || isThinking ? 1 : 0),
                          itemBuilder: (_, i) {
                            if (i == msgs.length) {
                              return streaming
                                  ? const StreamingBubble()
                                  : const TypingIndicator();
                            }
                            return ChatBubble(message: msgs[i]);
                          },
                        ),
                        if (_showScrollToBottom)
                          ScrollToBottomButton(onPressed: _scrollToBottom),
                      ],
                    ),
            ),
          ),

          // Input bar
          _InputBar(
            controller: _inputCtrl,
            sending: _sending,
            streaming: streaming,
            onSend: _send,
            onStop: _stop,
            showVoiceInput: _showVoiceInput,
            onToggleVoice: () {
              setState(() => _showVoiceInput = !_showVoiceInput);
            },
            onAudioRecorded: _handleAudioRecorded,
            onTranscription: (text) {
              _inputCtrl.text = text;
              setState(() => _showVoiceInput = false);
            },
          ),
        ],
      ),
    );
  }
}

class _InputBar extends StatelessWidget {
  final TextEditingController controller;
  final bool sending;

  /// A turn is in flight over the socket, so the send button becomes Stop.
  final bool streaming;
  final VoidCallback onSend;
  final VoidCallback onStop;
  final bool showVoiceInput;
  final VoidCallback onToggleVoice;
  final Future<String> Function(String) onAudioRecorded;
  final void Function(String) onTranscription;

  const _InputBar({
    required this.controller,
    required this.sending,
    required this.streaming,
    required this.onSend,
    required this.onStop,
    required this.showVoiceInput,
    required this.onToggleVoice,
    required this.onAudioRecorded,
    required this.onTranscription,
  });

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return SafeArea(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Divider(height: 1),
          SlashSuggestions(controller: controller),
          if (showVoiceInput)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
              child: VoiceInputWidget(
                onAudioRecorded: onAudioRecorded,
                onTranscription: onTranscription,
              ),
            ),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
            child: Row(
              children: [
                // Voice toggle button
                IconButton(
                  icon: Icon(
                    showVoiceInput ? Icons.keyboard_rounded : Icons.mic_rounded,
                    size: 22,
                  ),
                  onPressed: onToggleVoice,
                  tooltip: showVoiceInput
                      ? l10n.chatKeyboardTooltip
                      : l10n.chatVoiceTooltip,
                ),
                Expanded(
                  child: TextField(
                    controller: controller,
                    maxLines: 4,
                    minLines: 1,
                    textInputAction: TextInputAction.newline,
                    decoration: InputDecoration(hintText: l10n.chatInputHint),
                  ),
                ),
                const SizedBox(width: 8),
                streaming
                    ? _StopButton(onStop: onStop)
                    : _SendButton(sending: sending, onSend: onSend),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _SendButton extends StatelessWidget {
  final bool sending;
  final VoidCallback onSend;

  const _SendButton({required this.sending, required this.onSend});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Material(
      color: cs.primary,
      borderRadius: BorderRadius.circular(14),
      child: InkWell(
        onTap: sending ? null : onSend,
        borderRadius: BorderRadius.circular(14),
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: sending
              ? const SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: Colors.white,
                  ),
                )
              : const Icon(Icons.send_rounded, color: Colors.white, size: 22),
        ),
      ),
    );
  }
}

/// Replaces the send button while a turn streams. Only reachable then, which
/// is why it has no disabled state.
class _StopButton extends StatelessWidget {
  final VoidCallback onStop;

  const _StopButton({required this.onStop});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Material(
      color: cs.errorContainer,
      borderRadius: BorderRadius.circular(14),
      child: InkWell(
        key: const ValueKey('stop-turn'),
        onTap: onStop,
        borderRadius: BorderRadius.circular(14),
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: Icon(Icons.stop_rounded, color: cs.onErrorContainer, size: 22),
        ),
      ),
    );
  }
}

class _EmptyChat extends StatelessWidget {
  final void Function(String) onPrompt;
  const _EmptyChat({required this.onPrompt});

  /// The chip label doubles as the prompt sent to the model, so the
  /// suggestions follow the UI language and are built per `build`.
  static List<String> _suggestions(AppLocalizations l10n) => [
    l10n.chatSuggestionWhoAreYou,
    l10n.chatSuggestionFunFact,
    l10n.chatSuggestionOrganizeDay,
    l10n.chatSuggestionSuperpower,
    l10n.chatSuggestionJoke,
    l10n.chatSuggestionWhatCanYouDo,
  ];

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final l10n = context.l10n;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 24),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const MascotWidget(size: 80),
          const SizedBox(height: 16),
          Text(
            l10n.chatEmptyTitle,
            textAlign: TextAlign.center,
            style: Theme.of(
              context,
            ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 8),
          Text(
            l10n.chatEmptySubtitle,
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
              color: cs.onSurface.withValues(alpha: 0.6),
            ),
          ),
          const SizedBox(height: 28),
          Wrap(
            alignment: WrapAlignment.center,
            spacing: 8,
            runSpacing: 8,
            children: _suggestions(l10n)
                .map(
                  (s) =>
                      ActionChip(label: Text(s), onPressed: () => onPrompt(s)),
                )
                .toList(),
          ),
        ],
      ),
    );
  }
}

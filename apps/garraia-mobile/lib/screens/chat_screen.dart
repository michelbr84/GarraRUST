import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../providers/auth_provider.dart';
import '../providers/chat_provider.dart';
import '../services/api_service.dart';
import '../services/offline_queue.dart';
import '../widgets/chat_bubble.dart';
import '../widgets/mascot_widget.dart';
import '../widgets/queue_status_indicator.dart';
import '../widgets/scroll_to_bottom_button.dart';
import '../widgets/typing_indicator.dart';
import '../widgets/voice_input_widget.dart';

class ChatScreen extends ConsumerStatefulWidget {
  const ChatScreen({super.key});

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
    final isAtBottom = _scrollCtrl.position.pixels >=
        _scrollCtrl.position.maxScrollExtent - 100;
    if (_showScrollToBottom == isAtBottom) {
      setState(() => _showScrollToBottom = !isAtBottom);
    }
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
              const SnackBar(
                content: Text('Mensagem salva para envio quando online'),
              ),
            );
          }
        } else {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Erro: $e')),
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
    try {
      final api = ref.read(apiServiceProvider);
      return await api.transcribeAudio(audioPath);
    } catch (_) {
      return 'Erro ao transcrever audio';
    }
  }

  @override
  Widget build(BuildContext context) {
    final messages = ref.watch(chatMessagesProvider);
    final mascotState = ref.watch(mascotStateProvider);
    final isThinking = mascotState == MascotState.thinking;

    return Scaffold(
      appBar: AppBar(
        title: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            MascotWidget(size: 32, state: mascotState),
            const SizedBox(width: 10),
            const Text('Garra'),
          ],
        ),
        centerTitle: true,
        actions: [
          IconButton(
            icon: const Icon(Icons.devices_rounded),
            tooltip: 'Parear dispositivos',
            onPressed: () => context.push('/pair'),
          ),
          PopupMenuButton<String>(
            onSelected: (v) async {
              if (v == 'logout') {
                await ref.read(authStateProvider.notifier).logout();
                if (context.mounted) context.go('/login');
              }
            },
            itemBuilder: (_) => [
              const PopupMenuItem(
                value: 'logout',
                child: Row(
                  children: [
                    Icon(Icons.logout),
                    SizedBox(width: 8),
                    Text('Sair'),
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
              error: (e, _) => Center(child: Text('Erro ao carregar: $e')),
              data: (msgs) => msgs.isEmpty && !isThinking
                  ? _EmptyChat(onPrompt: (p) {
                      _inputCtrl.text = p;
                      _send();
                    })
                  : Stack(
                      children: [
                        ListView.builder(
                          controller: _scrollCtrl,
                          padding: const EdgeInsets.symmetric(
                              horizontal: 12, vertical: 8),
                          itemCount: msgs.length + (isThinking ? 1 : 0),
                          itemBuilder: (_, i) {
                            if (i == msgs.length && isThinking) {
                              return const TypingIndicator();
                            }
                            return ChatBubble(message: msgs[i]);
                          },
                        ),
                        if (_showScrollToBottom)
                          ScrollToBottomButton(
                            onPressed: _scrollToBottom,
                          ),
                      ],
                    ),
            ),
          ),

          // Input bar
          _InputBar(
            controller: _inputCtrl,
            sending: _sending,
            onSend: _send,
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
  final VoidCallback onSend;
  final bool showVoiceInput;
  final VoidCallback onToggleVoice;
  final Future<String> Function(String) onAudioRecorded;
  final void Function(String) onTranscription;

  const _InputBar({
    required this.controller,
    required this.sending,
    required this.onSend,
    required this.showVoiceInput,
    required this.onToggleVoice,
    required this.onAudioRecorded,
    required this.onTranscription,
  });

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Divider(height: 1),
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
                    showVoiceInput
                        ? Icons.keyboard_rounded
                        : Icons.mic_rounded,
                    size: 22,
                  ),
                  onPressed: onToggleVoice,
                  tooltip: showVoiceInput ? 'Teclado' : 'Voz',
                ),
                Expanded(
                  child: TextField(
                    controller: controller,
                    maxLines: 4,
                    minLines: 1,
                    textInputAction: TextInputAction.newline,
                    decoration: const InputDecoration(
                      hintText: 'Digite uma mensagem...',
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                _SendButton(sending: sending, onSend: onSend),
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
                      strokeWidth: 2, color: Colors.white),
                )
              : const Icon(Icons.send_rounded, color: Colors.white, size: 22),
        ),
      ),
    );
  }
}

class _EmptyChat extends StatelessWidget {
  final void Function(String) onPrompt;
  const _EmptyChat({required this.onPrompt});

  static const _suggestions = [
    'Quem e voce, Garra?',
    'Me conta uma curiosidade insana',
    'Me ajuda a organizar meu dia',
    'Qual e seu superpoder?',
    'Me conta uma piada',
    'O que voce consegue fazer?',
  ];

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 24),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const MascotWidget(size: 80),
          const SizedBox(height: 16),
          Text(
            'Oi! Eu sou o Garra.',
            textAlign: TextAlign.center,
            style: Theme.of(context)
                .textTheme
                .titleLarge
                ?.copyWith(fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 8),
          Text(
            'Seu assistente pessoal de IA.\nMe pergunte qualquer coisa!',
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
            children: _suggestions
                .map(
                  (s) => ActionChip(
                    label: Text(s),
                    onPressed: () => onPrompt(s),
                  ),
                )
                .toList(),
          ),
        ],
      ),
    );
  }
}

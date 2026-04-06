import 'dart:convert';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../services/sync_service.dart';

/// Screen for pairing devices via QR code scanning.
class PairScreen extends ConsumerStatefulWidget {
  const PairScreen({super.key});

  @override
  ConsumerState<PairScreen> createState() => _PairScreenState();
}

class _PairScreenState extends ConsumerState<PairScreen>
    with SingleTickerProviderStateMixin {
  late TabController _tabController;
  String? _pairingToken;
  bool _scanning = false;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 2, vsync: this);
    _generatePairingToken();
    // Request device list from sync service
    ref.read(syncServiceProvider).requestDeviceList();
  }

  @override
  void dispose() {
    _tabController.dispose();
    super.dispose();
  }

  void _generatePairingToken() {
    // Generate a random pairing token
    final random = Random.secure();
    final bytes = List.generate(32, (_) => random.nextInt(256));
    setState(() {
      _pairingToken = base64Url.encode(bytes);
    });
  }

  void _onQrScanned(String code) {
    if (_scanning) return;
    _scanning = true;

    // Send the scanned pairing token to the sync service
    ref.read(syncServiceProvider).pairDevice(code);

    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Pareamento iniciado...')),
    );

    // Reset scanning flag after a delay to prevent duplicate scans
    Future.delayed(const Duration(seconds: 3), () {
      if (mounted) _scanning = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final devices = ref.watch(pairedDevicesProvider);
    final syncStatus = ref.watch(syncConnectionStateProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Parear Dispositivos'),
        leading: IconButton(
          icon: const Icon(Icons.arrow_back),
          onPressed: () => context.pop(),
        ),
        bottom: TabBar(
          controller: _tabController,
          tabs: const [
            Tab(text: 'Meu QR Code', icon: Icon(Icons.qr_code_2_rounded)),
            Tab(text: 'Escanear', icon: Icon(Icons.qr_code_scanner_rounded)),
          ],
        ),
      ),
      body: Column(
        children: [
          // Sync status indicator
          _SyncStatusBanner(status: syncStatus),

          // Tab content
          Expanded(
            child: TabBarView(
              controller: _tabController,
              children: [
                // Tab 1: Show QR code
                _ShowQrTab(
                  pairingToken: _pairingToken,
                  onRegenerate: _generatePairingToken,
                ),
                // Tab 2: Scan QR code
                _ScanQrTab(onScanned: _onQrScanned),
              ],
            ),
          ),

          // Paired devices list
          if (devices.isNotEmpty) ...[
            const Divider(height: 1),
            Padding(
              padding: const EdgeInsets.all(12),
              child: Text(
                'Dispositivos Pareados',
                style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      fontWeight: FontWeight.w600,
                    ),
              ),
            ),
            SizedBox(
              height: min(devices.length * 64.0, 192),
              child: ListView.builder(
                itemCount: devices.length,
                itemBuilder: (_, i) {
                  final device = devices[i];
                  return ListTile(
                    leading: Icon(
                      _platformIcon(device.platform),
                      color: device.isOnline ? cs.primary : cs.onSurface.withValues(alpha: 0.4),
                    ),
                    title: Text(device.platform),
                    subtitle: Text(
                      device.isOnline ? 'Online' : 'Visto: ${device.lastSeen}',
                      style: TextStyle(
                        color: device.isOnline
                            ? cs.primary
                            : cs.onSurface.withValues(alpha: 0.5),
                        fontSize: 12,
                      ),
                    ),
                    trailing: device.isOnline
                        ? Container(
                            width: 8,
                            height: 8,
                            decoration: const BoxDecoration(
                              color: Colors.green,
                              shape: BoxShape.circle,
                            ),
                          )
                        : null,
                  );
                },
              ),
            ),
          ],
        ],
      ),
    );
  }

  IconData _platformIcon(String platform) => switch (platform.toLowerCase()) {
        'android' => Icons.phone_android_rounded,
        'ios' => Icons.phone_iphone_rounded,
        'windows' => Icons.desktop_windows_rounded,
        'macos' => Icons.laptop_mac_rounded,
        'linux' => Icons.computer_rounded,
        'web' => Icons.language_rounded,
        _ => Icons.devices_rounded,
      };
}

class _SyncStatusBanner extends StatelessWidget {
  final SyncStatus status;

  const _SyncStatusBanner({required this.status});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    if (status == SyncStatus.connected) return const SizedBox.shrink();

    final Color bgColor;
    final String text;

    switch (status) {
      case SyncStatus.connecting:
        bgColor = cs.tertiary;
        text = 'Conectando ao servidor de sincronizacao...';
      case SyncStatus.error:
        bgColor = cs.error;
        text = 'Erro de conexao. Tentando reconectar...';
      case SyncStatus.disconnected:
        bgColor = cs.secondary;
        text = 'Desconectado do servidor de sincronizacao';
      case SyncStatus.connected:
        bgColor = cs.primary;
        text = 'Conectado';
    }

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      color: bgColor,
      child: Text(
        text,
        style: TextStyle(color: cs.onError, fontSize: 12),
        textAlign: TextAlign.center,
      ),
    );
  }
}

class _ShowQrTab extends StatelessWidget {
  final String? pairingToken;
  final VoidCallback onRegenerate;

  const _ShowQrTab({required this.pairingToken, required this.onRegenerate});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    if (pairingToken == null) {
      return const Center(child: CircularProgressIndicator());
    }

    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(
              'Escaneie este QR Code\nno outro dispositivo',
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyLarge?.copyWith(
                    color: cs.onSurface.withValues(alpha: 0.7),
                  ),
            ),
            const SizedBox(height: 24),
            Container(
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                color: Colors.white,
                borderRadius: BorderRadius.circular(16),
              ),
              child: QrImageView(
                data: pairingToken!,
                version: QrVersions.auto,
                size: 220,
                backgroundColor: Colors.white,
              ),
            ),
            const SizedBox(height: 24),
            OutlinedButton.icon(
              onPressed: onRegenerate,
              icon: const Icon(Icons.refresh_rounded),
              label: const Text('Gerar novo codigo'),
            ),
          ],
        ),
      ),
    );
  }
}

class _ScanQrTab extends StatefulWidget {
  final void Function(String code) onScanned;

  const _ScanQrTab({required this.onScanned});

  @override
  State<_ScanQrTab> createState() => _ScanQrTabState();
}

class _ScanQrTabState extends State<_ScanQrTab> {
  final MobileScannerController _scannerController = MobileScannerController();

  @override
  void dispose() {
    _scannerController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(16),
          child: Text(
            'Aponte a camera para o QR Code\ndo outro dispositivo',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyLarge?.copyWith(
                  color: cs.onSurface.withValues(alpha: 0.7),
                ),
          ),
        ),
        Expanded(
          child: ClipRRect(
            borderRadius: BorderRadius.circular(16),
            child: MobileScanner(
              controller: _scannerController,
              onDetect: (capture) {
                final barcodes = capture.barcodes;
                for (final barcode in barcodes) {
                  final rawValue = barcode.rawValue;
                  if (rawValue != null && rawValue.isNotEmpty) {
                    widget.onScanned(rawValue);
                    return;
                  }
                }
              },
            ),
          ),
        ),
      ],
    );
  }
}

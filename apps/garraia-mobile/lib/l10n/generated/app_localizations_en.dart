// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get activityCopyLogToast => 'Log copied';

  @override
  String get activityCopyLogTooltip => 'Copy log';

  @override
  String get activityCurrentSession => 'Current session';

  @override
  String get activityLogEmpty => '(empty)';

  @override
  String activityMessageCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count messages',
      one: '1 message',
    );
    return '$_temp0';
  }

  @override
  String get activityNoSessions => 'No sessions on the runtime. Start a chat.';

  @override
  String get activitySectionRuntimeLog => 'Runtime log';

  @override
  String get activitySectionSessions => 'Sessions';

  @override
  String get activityTitle => 'Activity';

  @override
  String get agentsMcpConnected => 'connected';

  @override
  String get agentsMcpDisconnected => 'disconnected';

  @override
  String get agentsNoMcpServers => 'No MCP servers configured on this runtime.';

  @override
  String get agentsNoModes => 'No agent modes reported.';

  @override
  String get agentsSectionMcpServers => 'MCP servers';

  @override
  String get agentsSectionModes => 'Agent modes';

  @override
  String get agentsTitle => 'Agents';

  @override
  String agentsToolCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count tools',
      one: '1 tool',
    );
    return '$_temp0';
  }

  @override
  String get appTitle => 'Garra Mobile';

  @override
  String get automationsEditorComingSoon =>
      'This runtime supports automations, but the mobile editor lands in a later release.';

  @override
  String get automationsTitle => 'Automations';

  @override
  String get automationsUnavailableWhy =>
      'The connected Garra runtime does not expose a scheduling API yet. Scheduled tasks are managed from the Garra CLI today; this screen will light up automatically when the runtime advertises the capability.';

  @override
  String get biometricPromptReason => 'Authenticate to access Garra';

  @override
  String get chatCodeCopiedToast => 'Code copied';

  @override
  String get chatCopyCodeTooltip => 'Copy code';

  @override
  String get chatCopyMessageTooltip => 'Copy message';

  @override
  String get chatEmptySubtitle =>
      'Your personal AI assistant.\nAsk me anything!';

  @override
  String get chatEmptyTitle => 'Hi! I\'m Garra.';

  @override
  String get chatErrorConnectionDropped =>
      'The connection dropped before the reply finished.';

  @override
  String get chatErrorReplyMissing =>
      'The gateway reply came back without content.';

  @override
  String get chatErrorRuntimeReported => 'The runtime reported an error.';

  @override
  String get chatErrorSessionIdMissing =>
      'The gateway did not return a session id.';

  @override
  String get chatInputHint => 'Type a message...';

  @override
  String get chatKeyboardTooltip => 'Keyboard';

  @override
  String chatLoadConversationError(String error) {
    return 'Could not load the conversation: $error';
  }

  @override
  String get chatMessageCopiedToast => 'Message copied';

  @override
  String get chatMessageQueuedOffline =>
      'Message saved and will be sent when you\'re back online';

  @override
  String get chatNewSession => 'New session';

  @override
  String get chatPairDevicesTooltip => 'Pair devices';

  @override
  String chatStreamingRunningTool(String tool) {
    return 'running $tool';
  }

  @override
  String get chatSuggestionFunFact => 'Tell me a wild fun fact';

  @override
  String get chatSuggestionJoke => 'Tell me a joke';

  @override
  String get chatSuggestionOrganizeDay => 'Help me organize my day';

  @override
  String get chatSuggestionSuperpower => 'What\'s your superpower?';

  @override
  String get chatSuggestionWhatCanYouDo => 'What can you do?';

  @override
  String get chatSuggestionWhoAreYou => 'Who are you, Garra?';

  @override
  String get chatVoiceNoRuntimeConfigured => 'No runtime configured';

  @override
  String get chatVoiceTooltip => 'Voice';

  @override
  String get chatVoiceTranscribeError => 'Could not transcribe audio';

  @override
  String get commonBack => 'Back';

  @override
  String get commonBrand => 'Garra';

  @override
  String get commonCancel => 'Cancel';

  @override
  String get commonChecking => 'Checking…';

  @override
  String get commonContinue => 'Continue';

  @override
  String get commonCopied => 'Copied';

  @override
  String get commonCopy => 'Copy';

  @override
  String get commonDelete => 'Delete';

  @override
  String get commonEmailInvalid => 'Invalid email';

  @override
  String get commonEmailLabel => 'Email';

  @override
  String get commonEmptyNothingYet => 'Nothing here yet.';

  @override
  String get commonErrorNoConnection => 'No connection. Check your internet.';

  @override
  String commonErrorWithDetail(String error) {
    return 'Error: $error';
  }

  @override
  String commonFeatureUnavailableOnRuntime(String feature) {
    return '$feature is not available on this runtime';
  }

  @override
  String get commonLogout => 'Sign out';

  @override
  String get commonNotConfigured => 'Not configured';

  @override
  String get commonNotSet => 'Not set';

  @override
  String get commonPasswordLabel => 'Password';

  @override
  String get commonPasswordMinLength => 'At least 8 characters';

  @override
  String get commonRefresh => 'Refresh';

  @override
  String get commonRetry => 'Retry';

  @override
  String get commonSave => 'Save';

  @override
  String get commonSaving => 'Saving…';

  @override
  String get commonSignIn => 'Sign in';

  @override
  String get errorCouldNotReachRuntime => 'Could not reach the runtime';

  @override
  String get errorNoRuntimeConfigured => 'No Garra runtime configured.';

  @override
  String get filesNoProjects =>
      'No projects on this runtime yet. Create one from the Garra console or CLI.';

  @override
  String get filesProjectNoTrackedFiles => 'This project has no tracked files.';

  @override
  String get filesTitle => 'Files';

  @override
  String get homeBannerHeadline => 'LOCAL AI. A BRIGHTER YOU.';

  @override
  String get homeBannerSubtitle => 'More control. A more capable you.';

  @override
  String homeFeatureTileSemantics(String title, String subtitle) {
    return '$title. $subtitle';
  }

  @override
  String homeFeatureTileSemanticsUnavailable(String title, String subtitle) {
    return '$title. $subtitle. Unavailable on this runtime';
  }

  @override
  String get homeFeatureUnavailable => 'Unavailable';

  @override
  String get homeGreetingAnonymous => 'Hello 👋';

  @override
  String get homeGreetingCaption => 'Smaller device.\nBigger possibilities.';

  @override
  String homeGreetingNamed(String name) {
    return 'Hello, $name 👋';
  }

  @override
  String get homeGreetingSubtitle =>
      'Good to see you again.\nYour AI assistant is ready.';

  @override
  String get homeHeaderQuote => '“AI that works for you.\nOn your terms.”';

  @override
  String get homeHeaderTagline => 'Local-first AI Assistant';

  @override
  String get homeHeaderValuePowerful => 'Powerful';

  @override
  String get homeHeaderValuePrivate => 'Private';

  @override
  String get homeHeaderValueYours => 'Yours';

  @override
  String get homeLlmConnectedToPc => 'Connected to PC';

  @override
  String get homeLlmLabel => 'LLM:';

  @override
  String get homeLlmLocalServer => 'Local server';

  @override
  String get homeLlmNoProvider => 'No provider set';

  @override
  String get homeLlmNotConnected => 'Not connected';

  @override
  String get homeLlmPickProvider => 'Pick a provider';

  @override
  String get homeLlmWaitingRuntime => 'Waiting for the runtime';

  @override
  String get homeQuickActionsSubtitle => 'Common tasks, one tap away.';

  @override
  String get homeQuickActionsTagline => 'Get more done, locally.';

  @override
  String get homeQuickActionsTitle => 'Quick Actions';

  @override
  String get homeQuickPairPc => 'Pair PC';

  @override
  String get homeRuntimeLabel => 'Runtime:';

  @override
  String get homeRuntimeLocal => 'Local on this phone';

  @override
  String get homeRuntimeLocalTagline => 'Fast. Private. Always with you.';

  @override
  String get homeRuntimeNotRunning =>
      'Not running — open Termux and run `garra start`';

  @override
  String get homeRuntimeRemote => 'Garra on your network';

  @override
  String get homeRuntimeUnreachable => 'Unreachable — check the address';

  @override
  String homeRuntimeVersionSessions(String version, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count sessions',
      one: '$count session',
    );
    return 'v$version · $_temp0';
  }

  @override
  String homeStatusCardSemantics(String label, String value, String subtitle) {
    return '$label $value. $subtitle';
  }

  @override
  String get homeTileAgentsSubtitle => 'Create and manage agents';

  @override
  String get homeTileAutomationsSubtitle => 'Set up smart workflows';

  @override
  String get homeTileChat => 'Chat';

  @override
  String get homeTileChatSubtitle => 'Talk with your AI assistant';

  @override
  String get homeTileFilesSubtitle => 'Access and manage files';

  @override
  String get homeTileMemorySubtitle => 'What Garra remembers';

  @override
  String get homeTileSkillsSubtitle => 'Extend what Garra can do';

  @override
  String get loginErrorGeneric => 'Couldn\'t sign in. Please try again.';

  @override
  String get loginErrorInvalidCredentials => 'Incorrect email or password.';

  @override
  String get loginGreeting => 'Hi! I\'m Garra.';

  @override
  String get loginNoAccountCta => 'Don\'t have an account? Create one';

  @override
  String get loginSubtitle => 'Sign in to continue';

  @override
  String get memoryAlreadyGone => 'It was already gone';

  @override
  String get memoryCopied => 'Memory copied';

  @override
  String get memoryDeleteBody =>
      'It disappears from the runtime and cannot be undone.';

  @override
  String memoryDeleteFailed(String error) {
    return 'Could not delete: $error';
  }

  @override
  String get memoryDeletePinnedBody =>
      'It is pinned, but deleting does not ask twice: it disappears from the runtime and cannot be undone.';

  @override
  String get memoryDeleteTitle => 'Delete this memory?';

  @override
  String get memoryDeleted => 'Memory deleted';

  @override
  String get memoryEmpty =>
      'Garra has not stored any memory yet. Chat a little and come back.';

  @override
  String memoryNoMatches(String query) {
    return 'No memory matches \"$query\".';
  }

  @override
  String get memorySearchHint => 'Search memories';

  @override
  String get memoryTitle => 'Memory';

  @override
  String get navHome => 'Home';

  @override
  String get notificationsChannelChatDescription =>
      'Notifications for new chat messages';

  @override
  String get notificationsChannelChatName => 'Chat messages';

  @override
  String get notificationsChannelSyncDescription =>
      'Notifications about sync between your devices';

  @override
  String get notificationsChannelSyncName => 'Sync status';

  @override
  String get notificationsChannelSystemDescription => 'System notifications';

  @override
  String get notificationsChannelSystemName => 'System';

  @override
  String get notificationsEmpty =>
      'Nothing yet. Notifications from automations and background agents will show up here.';

  @override
  String get notificationsTitle => 'Notifications';

  @override
  String get onboardingDoneNextCloud => 'Next: sign in to your account.';

  @override
  String get onboardingDoneNextLocal =>
      'Your AI lives on your device. The model doesn\'t have to.';

  @override
  String get onboardingDoneOpenGarra => 'Open Garra';

  @override
  String onboardingDoneRuntime(String runtime) {
    return 'Runtime: $runtime.';
  }

  @override
  String get onboardingDoneTitle => 'All set.';

  @override
  String onboardingDoneTitleNamed(String name) {
    return 'All set, $name.';
  }

  @override
  String get onboardingErrorCloudUnreachable => 'Could not reach Garra Cloud';

  @override
  String onboardingErrorHttpStatus(int code) {
    return 'HTTP $code from the gateway';
  }

  @override
  String get onboardingErrorLocalUnreachable =>
      'Nothing answered on this phone. Is `garra start` running in Termux?';

  @override
  String get onboardingErrorNeedsApiKey => 'The gateway asked for an API key';

  @override
  String get onboardingErrorRemoteUnreachable =>
      'No answer. Same Wi-Fi? Is the gateway bound to 0.0.0.0?';

  @override
  String get onboardingGatewayAddressLabel => 'Gateway address';

  @override
  String get onboardingGatewayApiKeyHint => 'Only if gateway.api_key is set';

  @override
  String get onboardingGatewayApiKeyLabel => 'Gateway API key (optional)';

  @override
  String get onboardingInvalidAddress => 'Enter a valid http(s) address';

  @override
  String get onboardingNameSubtitle =>
      'Only used for the greeting. It never leaves this device.';

  @override
  String get onboardingNameTitle => 'What should Garra\ncall you?';

  @override
  String get onboardingPlainHttpWarning =>
      'Plain HTTP: your messages and the API key are readable on this network. Fine on your home Wi-Fi, not on a public one.';

  @override
  String onboardingProbeResult(String version, String status) {
    return 'Garra $version · $status';
  }

  @override
  String onboardingProbeResultWithProvider(
    String version,
    String status,
    String provider,
  ) {
    return 'Garra $version · $status · $provider';
  }

  @override
  String get onboardingRuntimeSubtitle =>
      'Your memory, skills and files stay on the runtime you pick. The LLM can be anywhere.';

  @override
  String get onboardingRuntimeTitle => 'Where does\nGarra run?';

  @override
  String get onboardingTermuxInstallHint =>
      'Then come back and tap \"Test connection\". The app talks to it on 127.0.0.1.';

  @override
  String get onboardingTermuxInstallTitle => 'Install Garra in Termux (once):';

  @override
  String get onboardingTestConnection => 'Test connection';

  @override
  String get onboardingTesting => 'Testing…';

  @override
  String pairDeviceLastSeen(String lastSeen) {
    return 'Last seen: $lastSeen';
  }

  @override
  String get pairDeviceOnline => 'Online';

  @override
  String get pairPairedDevicesHeader => 'Paired Devices';

  @override
  String get pairPairingStarted => 'Pairing started...';

  @override
  String get pairRegenerateCode => 'Generate new code';

  @override
  String get pairScanInstruction =>
      'Point the camera at the QR code\non the other device';

  @override
  String get pairShowQrInstruction => 'Scan this QR code\non the other device';

  @override
  String get pairSyncConnected => 'Connected';

  @override
  String get pairSyncConnecting => 'Connecting to sync server...';

  @override
  String get pairSyncDisconnected => 'Disconnected from sync server';

  @override
  String get pairSyncError => 'Connection error. Reconnecting...';

  @override
  String get pairTabMyQrCode => 'My QR Code';

  @override
  String get pairTabScan => 'Scan';

  @override
  String get pairTitle => 'Pair Devices';

  @override
  String get profileDefaultName => 'Garra user';

  @override
  String get profileDisplayName => 'Display name';

  @override
  String get profileNameHint => 'Your name';

  @override
  String get profileNoRuntime => 'No runtime';

  @override
  String get profilePairedDevices => 'Paired devices';

  @override
  String get profilePairedDevicesSubtitle =>
      'Sync sessions across your devices';

  @override
  String profileRuntimeSummary(String mode, String baseUrl) {
    return '$mode · $baseUrl';
  }

  @override
  String profileRuntimeSummaryWithVersion(
    String mode,
    String baseUrl,
    String version,
  ) {
    return '$mode · $baseUrl · v$version';
  }

  @override
  String get profileSettingsSubtitle => 'Runtime, account and app details';

  @override
  String get profileSignOutSubtitle => 'Clear the cloud session on this device';

  @override
  String get profileTitle => 'Profile';

  @override
  String get providersApiKeysNote =>
      'API keys are configured on the runtime (garra init / web console), never stored on this phone.';

  @override
  String get providersDefaultBadge => 'default';

  @override
  String get providersEmpty =>
      'The runtime reported no providers. Run `garra init` to configure one.';

  @override
  String get providersNoModel => 'no model';

  @override
  String get providersStatusActive => 'active';

  @override
  String get providersStatusInactive => 'inactive';

  @override
  String get providersStatusNeedsApiKey => 'needs API key';

  @override
  String get providersTitle => 'Providers';

  @override
  String get queueOffline => 'No connection';

  @override
  String queueOfflineWithPending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count pending messages',
      one: '$count pending message',
    );
    return 'No connection - $_temp0';
  }

  @override
  String queuePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count pending messages',
      one: '$count pending message',
    );
    return '$_temp0';
  }

  @override
  String queueSyncing(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count messages',
      one: '$count message',
    );
    return 'Syncing $_temp0...';
  }

  @override
  String get registerConfirmPasswordLabel => 'Confirm password';

  @override
  String get registerCreateAccount => 'Create account';

  @override
  String get registerErrorEmailTaken => 'This email is already registered.';

  @override
  String get registerErrorGeneric =>
      'Couldn\'t create account. Please try again.';

  @override
  String get registerHaveAccountCta => 'Already have an account? Sign in';

  @override
  String get registerPasswordsMismatch => 'Passwords don\'t match';

  @override
  String get runtimeModeCloudDescription =>
      'Use the hosted Garra service with your account.';

  @override
  String get runtimeModeCloudTitle => 'Garra Cloud';

  @override
  String get runtimeModeLocalDescription =>
      'Garra runs inside Termux on this device. Memory, skills and files stay here.';

  @override
  String get runtimeModeLocalTitle => 'On this phone';

  @override
  String get runtimeModeRemoteDescription =>
      'Connect to a Garra gateway on your PC or home server over Wi-Fi.';

  @override
  String get runtimeModeRemoteTitle => 'Another Garra';

  @override
  String get settingsAccountCreatedLabel => 'Joined';

  @override
  String get settingsAccountIdLabel => 'ID';

  @override
  String get settingsAccountLoadError => 'Couldn\'t load account details';

  @override
  String get settingsAccountTitle => 'Account';

  @override
  String get settingsChangeRuntime => 'Change runtime';

  @override
  String get settingsLanguageEnglish => 'English';

  @override
  String get settingsLanguageHint => 'Applies immediately, no restart needed.';

  @override
  String get settingsLanguagePortuguese => 'Português (Brasil)';

  @override
  String get settingsLanguageSystem => 'System default';

  @override
  String get settingsLanguageTitle => 'Language';

  @override
  String get settingsLogout => 'Sign out';

  @override
  String get settingsLogoutConfirmBody =>
      'You\'ll be signed out and will need to sign in again the next time you open the app.';

  @override
  String get settingsLogoutConfirmTitle => 'Sign out?';

  @override
  String get settingsRuntimeAddressLabel => 'Address';

  @override
  String get settingsRuntimeChecking => 'checking…';

  @override
  String get settingsRuntimeModeLabel => 'Mode';

  @override
  String get settingsRuntimeProviderLabel => 'Provider';

  @override
  String settingsRuntimeProviderModel(String provider, String model) {
    return '$provider / $model';
  }

  @override
  String get settingsRuntimeStatusLabel => 'Status';

  @override
  String settingsRuntimeStatusVersion(String status, String version) {
    return '$status · v$version';
  }

  @override
  String get settingsRuntimeTitle => 'Runtime';

  @override
  String get settingsRuntimeUnreachable => 'unreachable';

  @override
  String get settingsSessionExpiredRedirecting =>
      'Session expired. Redirecting…';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get skillsDeprecated => 'deprecated';

  @override
  String skillsFailCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count fails',
      one: '1 fail',
    );
    return '$_temp0';
  }

  @override
  String get skillsNoLearnedSkills =>
      'No learned skills yet. Garra mines them from your sessions (ADR 0010).';

  @override
  String skillsScore(String score) {
    return 'score $score';
  }

  @override
  String get skillsSectionLearned => 'Learned skills';

  @override
  String get skillsSectionSlashCommands => 'Slash commands';

  @override
  String get skillsTitle => 'Skills';

  @override
  String skillsVersion(String version) {
    return 'v$version';
  }

  @override
  String get voiceHoldToRecord => 'Hold to record';

  @override
  String get voiceMicPermissionRequired => 'Microphone permission required';

  @override
  String voiceProcessAudioError(String error) {
    return 'Could not process audio: $error';
  }

  @override
  String voiceStartRecordingError(String error) {
    return 'Could not start recording: $error';
  }

  @override
  String get voiceTranscribing => 'Transcribing...';

  @override
  String get voiceTranscriptionUnavailable => 'Transcription unavailable';
}

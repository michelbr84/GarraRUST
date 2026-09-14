import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_pt.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'generated/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('pt'),
  ];

  /// CopyIconButton toast: shown after the log is copied to the clipboard
  ///
  /// In en, this message translates to:
  /// **'Log copied'**
  String get activityCopyLogToast;

  /// CopyIconButton tooltip: on the runtime log box
  ///
  /// In en, this message translates to:
  /// **'Copy log'**
  String get activityCopyLogTooltip;

  /// GarraListTile title when the session is the currently selected one
  ///
  /// In en, this message translates to:
  /// **'Current session'**
  String get activityCurrentSession;

  /// SelectableText content of the log box when the runtime returned no log lines
  ///
  /// In en, this message translates to:
  /// **'(empty)'**
  String get activityLogEmpty;

  /// GarraListTile subtitle: message count with hand-rolled plural; an adjacent literal '${s.channelId != null ? ' · ${s.chan
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 message} other{{count} messages}}'**
  String activityMessageCount(int count);

  /// EmptyState text when the session list is empty
  ///
  /// In en, this message translates to:
  /// **'No sessions on the runtime. Start a chat.'**
  String get activityNoSessions;

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'Runtime log'**
  String get activitySectionRuntimeLog;

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'Sessions'**
  String get activitySectionSessions;

  /// GarraPage title (AppBar)
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get activityTitle;

  /// Status word inside the MCP server subtitle interpolation (s.connected == true)
  ///
  /// In en, this message translates to:
  /// **'connected'**
  String get agentsMcpConnected;

  /// Status word inside the MCP server subtitle interpolation (s.connected == false)
  ///
  /// In en, this message translates to:
  /// **'disconnected'**
  String get agentsMcpDisconnected;

  /// EmptyState text when /api/mcp returns an empty list
  ///
  /// In en, this message translates to:
  /// **'No MCP servers configured on this runtime.'**
  String get agentsNoMcpServers;

  /// EmptyState text when /api/modes returns an empty list
  ///
  /// In en, this message translates to:
  /// **'No agent modes reported.'**
  String get agentsNoModes;

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'MCP servers'**
  String get agentsSectionMcpServers;

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'Agent modes'**
  String get agentsSectionModes;

  /// FeatureTile title:
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get agentsTitle;

  /// GarraListTile subtitle of an MCP server row: tool count with hand-rolled plural, then ' · ' and a connection status word
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 tool} other{{count} tools}}'**
  String agentsToolCount(int count);

  /// Text() app title in the onboarding header row next to WolfMark
  ///
  /// In en, this message translates to:
  /// **'Garra Mobile'**
  String get appTitle;

  /// EmptyState text: when the runtime advertises the automations capability
  ///
  /// In en, this message translates to:
  /// **'This runtime supports automations, but the mobile editor lands in a later release.'**
  String get automationsEditorComingSoon;

  /// GarraPage title of AutomationsScreen
  ///
  /// In en, this message translates to:
  /// **'Automations'**
  String get automationsTitle;

  /// UnavailableFeature(why:) explanation body (three adjacent literals concatenated at lines 35-37)
  ///
  /// In en, this message translates to:
  /// **'The connected Garra runtime does not expose a scheduling API yet. Scheduled tasks are managed from the Garra CLI today; this screen will light up automatically when the runtime advertises the capability.'**
  String get automationsUnavailableWhy;

  /// Default `reason` → localizedReason of the OS biometric prompt (system dialog the user reads)
  ///
  /// In en, this message translates to:
  /// **'Authenticate to access Garra'**
  String get biometricPromptReason;

  /// toast: of CopyIconButton (key copy-code), shown after copying a code block
  ///
  /// In en, this message translates to:
  /// **'Code copied'**
  String get chatCodeCopiedToast;

  /// tooltip: of CopyIconButton (key copy-code) on fenced code blocks in assistant markdown
  ///
  /// In en, this message translates to:
  /// **'Copy code'**
  String get chatCopyCodeTooltip;

  /// tooltip: of CopyIconButton (key copy-message) next to the bubble timestamp
  ///
  /// In en, this message translates to:
  /// **'Copy message'**
  String get chatCopyMessageTooltip;

  /// Text() subtitle of the empty-chat state (bodyMedium, two lines separated by \n)
  ///
  /// In en, this message translates to:
  /// **'Your personal AI assistant.\nAsk me anything!'**
  String get chatEmptySubtitle;

  /// Text() headline of the empty-chat state (titleLarge, bold)
  ///
  /// In en, this message translates to:
  /// **'Hi! I\'m Garra.'**
  String get chatEmptyTitle;

  /// ChatFailed message emitted when the WebSocket stream ends without a terminator after the message was submitted → becomes
  ///
  /// In en, this message translates to:
  /// **'The connection dropped before the reply finished.'**
  String get chatErrorConnectionDropped;

  /// DioException message when the reply has no content; surfaced via commonErrorWithDetail
  ///
  /// In en, this message translates to:
  /// **'The gateway reply came back without content.'**
  String get chatErrorReplyMissing;

  /// ChatFailed fallback message when an `error` frame carries no `message` → ChatTurnFailed → chat screen turn error
  ///
  /// In en, this message translates to:
  /// **'The runtime reported an error.'**
  String get chatErrorRuntimeReported;

  /// DioException message when POST /api/sessions returns no id; surfaced via commonErrorWithDetail
  ///
  /// In en, this message translates to:
  /// **'The gateway did not return a session id.'**
  String get chatErrorSessionIdMissing;

  /// hintText: of the chat input TextField
  ///
  /// In en, this message translates to:
  /// **'Type a message...'**
  String get chatInputHint;

  /// tooltip: of the voice/keyboard toggle IconButton when voice input is shown (showVoiceInput ? 'Teclado' : 'Voz')
  ///
  /// In en, this message translates to:
  /// **'Keyboard'**
  String get chatKeyboardTooltip;

  /// Text() in the AsyncValue error branch of the message list
  ///
  /// In en, this message translates to:
  /// **'Could not load the conversation: {error}'**
  String chatLoadConversationError(String error);

  /// toast: of CopyIconButton (key copy-message), shown after copying a message
  ///
  /// In en, this message translates to:
  /// **'Message copied'**
  String get chatMessageCopiedToast;

  /// SnackBar content shown when send fails while offline and the message is queued
  ///
  /// In en, this message translates to:
  /// **'Message saved and will be sent when you\'re back online'**
  String get chatMessageQueuedOffline;

  /// PopupMenuItem label (value 'new'), shown when runtime is not cloud
  ///
  /// In en, this message translates to:
  /// **'New session'**
  String get chatNewSession;

  /// tooltip: of the AppBar IconButton that opens /pair
  ///
  /// In en, this message translates to:
  /// **'Pair devices'**
  String get chatPairDevicesTooltip;

  /// Text() in _ToolChip inside the streaming assistant bubble while a tool call is open (italic, onSurfaceVariant)
  ///
  /// In en, this message translates to:
  /// **'running {tool}'**
  String chatStreamingRunningTool(String tool);

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'Tell me a wild fun fact'**
  String get chatSuggestionFunFact;

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'Tell me a joke'**
  String get chatSuggestionJoke;

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'Help me organize my day'**
  String get chatSuggestionOrganizeDay;

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'What\'s your superpower?'**
  String get chatSuggestionSuperpower;

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'What can you do?'**
  String get chatSuggestionWhatCanYouDo;

  /// ActionChip label in _EmptyChat._suggestions (also sent as the prompt when tapped)
  ///
  /// In en, this message translates to:
  /// **'Who are you, Garra?'**
  String get chatSuggestionWhoAreYou;

  /// Return value of _handleAudioRecorded; VoiceInputWidget forwards it to onTranscription, which writes it into the chat Tex
  ///
  /// In en, this message translates to:
  /// **'No runtime configured'**
  String get chatVoiceNoRuntimeConfigured;

  /// tooltip: of the voice/keyboard toggle IconButton when voice input is hidden (showVoiceInput ? 'Teclado' : 'Voz')
  ///
  /// In en, this message translates to:
  /// **'Voice'**
  String get chatVoiceTooltip;

  /// Return value of _handleAudioRecorded on exception; forwarded by VoiceInputWidget into the chat TextField
  ///
  /// In en, this message translates to:
  /// **'Could not transcribe audio'**
  String get chatVoiceTranscribeError;

  /// TextButton label, step 2 footer row (_RuntimeStep)
  ///
  /// In en, this message translates to:
  /// **'Back'**
  String get commonBack;

  /// AppBar title Text next to the mascot
  ///
  /// In en, this message translates to:
  /// **'Garra'**
  String get commonBrand;

  /// TextButton action in the logout AlertDialog (dismiss)
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get commonCancel;

  /// RuntimeStatusCard subtitle: while health is loading
  ///
  /// In en, this message translates to:
  /// **'Checking…'**
  String get commonChecking;

  /// ElevatedButton label, step 1 (_NameStep)
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get commonContinue;

  /// default value of the `toast` parameter of copyToClipboard(), shown as SnackBar content for 1s after copying
  ///
  /// In en, this message translates to:
  /// **'Copied'**
  String get commonCopied;

  /// FilledButton.icon label: Text()
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get commonCopy;

  /// AlertDialog action FilledButton child Text() (confirm delete)
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get commonDelete;

  /// TextFormField validator error for the email field (also register_screen.dart:76)
  ///
  /// In en, this message translates to:
  /// **'Invalid email'**
  String get commonEmailInvalid;

  /// _InfoRow label: (account e-mail row)
  ///
  /// In en, this message translates to:
  /// **'Email'**
  String get commonEmailLabel;

  /// Default value of AsyncBody.emptyText, rendered by EmptyState Text() for empty lists
  ///
  /// In en, this message translates to:
  /// **'Nothing here yet.'**
  String get commonEmptyNothingYet;

  /// Error Text() under the form, returned by _friendlyError on network errors (also register_screen.dart:145)
  ///
  /// In en, this message translates to:
  /// **'No connection. Check your internet.'**
  String get commonErrorNoConnection;

  /// SnackBar content when sending fails while online
  ///
  /// In en, this message translates to:
  /// **'Error: {error}'**
  String commonErrorWithDetail(String error);

  /// Text() title of the UnavailableFeature widget (capability-missing explanation screen)
  ///
  /// In en, this message translates to:
  /// **'{feature} is not available on this runtime'**
  String commonFeatureUnavailableOnRuntime(String feature);

  /// PopupMenuItem label (value 'logout'), shown when runtime is cloud
  ///
  /// In en, this message translates to:
  /// **'Sign out'**
  String get commonLogout;

  /// _InfoRow value fallback when config?.mode is null
  ///
  /// In en, this message translates to:
  /// **'Not configured'**
  String get commonNotConfigured;

  /// _Row subtitle: when ownerName is empty
  ///
  /// In en, this message translates to:
  /// **'Not set'**
  String get commonNotSet;

  /// InputDecoration labelText of the password field (also register_screen.dart:83)
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get commonPasswordLabel;

  /// TextFormField validator error for the password field (also register_screen.dart:93)
  ///
  /// In en, this message translates to:
  /// **'At least 8 characters'**
  String get commonPasswordMinLength;

  /// IconButton tooltip on the refresh action
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get commonRefresh;

  /// OutlinedButton child Text() in ErrorState
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get commonRetry;

  /// AlertDialog action FilledButton child Text()
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get commonSave;

  /// ElevatedButton label of step 3 while the config is being persisted (saving == true)
  ///
  /// In en, this message translates to:
  /// **'Saving…'**
  String get commonSaving;

  /// ElevatedButton label of step 3 when mode == RuntimeMode.cloud (navigates to /login)
  ///
  /// In en, this message translates to:
  /// **'Sign in'**
  String get commonSignIn;

  /// Text() title of the shared ErrorState widget (shown by AsyncBody on error)
  ///
  /// In en, this message translates to:
  /// **'Could not reach the runtime'**
  String get errorCouldNotReachRuntime;

  /// ErrorState message when NoRuntimeConfigured is thrown (requireConnection())
  ///
  /// In en, this message translates to:
  /// **'No Garra runtime configured.'**
  String get errorNoRuntimeConfigured;

  /// AsyncBody emptyText when /api/projects returns an empty list
  ///
  /// In en, this message translates to:
  /// **'No projects on this runtime yet. Create one from the Garra console or CLI.'**
  String get filesNoProjects;

  /// AsyncBody emptyText when /api/projects/{id}/files returns an empty list
  ///
  /// In en, this message translates to:
  /// **'This project has no tracked files.'**
  String get filesProjectNoTrackedFiles;

  /// FeatureTile title:
  ///
  /// In en, this message translates to:
  /// **'Files'**
  String get filesTitle;

  /// Text() all-caps headline of the footer BrandBanner (letter-spaced)
  ///
  /// In en, this message translates to:
  /// **'LOCAL AI. A BRIGHTER YOU.'**
  String get homeBannerHeadline;

  /// Text() muted subtitle under the BrandBanner headline
  ///
  /// In en, this message translates to:
  /// **'More control. A more capable you.'**
  String get homeBannerSubtitle;

  /// Semantics label of the FeatureTile button (available == true branch)
  ///
  /// In en, this message translates to:
  /// **'{title}. {subtitle}'**
  String homeFeatureTileSemantics(String title, String subtitle);

  /// Semantics label of the FeatureTile button (available == false branch, suffix appended to '$title. $subtitle')
  ///
  /// In en, this message translates to:
  /// **'{title}. {subtitle}. Unavailable on this runtime'**
  String homeFeatureTileSemanticsUnavailable(String title, String subtitle);

  /// Text() inside the _UnavailablePill badge shown on dimmed feature tiles
  ///
  /// In en, this message translates to:
  /// **'Unavailable'**
  String get homeFeatureUnavailable;

  /// Text() hero greeting when the onboarding display name is empty
  ///
  /// In en, this message translates to:
  /// **'Hello 👋'**
  String get homeGreetingAnonymous;

  /// Text() right-aligned caption over the NightRidge art in HeroGreeting
  ///
  /// In en, this message translates to:
  /// **'Smaller device.\nBigger possibilities.'**
  String get homeGreetingCaption;

  /// Text() hero greeting with the user's display name
  ///
  /// In en, this message translates to:
  /// **'Hello, {name} 👋'**
  String homeGreetingNamed(String name);

  /// Text() two-line muted subtitle under the hero greeting
  ///
  /// In en, this message translates to:
  /// **'Good to see you again.\nYour AI assistant is ready.'**
  String get homeGreetingSubtitle;

  /// Text() italic tagline quote on the right side of HomeHeader (multi-line, right-aligned)
  ///
  /// In en, this message translates to:
  /// **'“AI that works for you.\nOn your terms.”'**
  String get homeHeaderQuote;

  /// Text() subtitle under the 'Garra Mobile' brand name in HomeHeader
  ///
  /// In en, this message translates to:
  /// **'Local-first AI Assistant'**
  String get homeHeaderTagline;

  /// TextSpan (violet) inside the _ValuesPill 'Private • Powerful • Yours' badge
  ///
  /// In en, this message translates to:
  /// **'Powerful'**
  String get homeHeaderValuePowerful;

  /// TextSpan (cyan) inside the _ValuesPill 'Private • Powerful • Yours' badge
  ///
  /// In en, this message translates to:
  /// **'Private'**
  String get homeHeaderValuePrivate;

  /// TextSpan (cyan) inside the _ValuesPill 'Private • Powerful • Yours' badge
  ///
  /// In en, this message translates to:
  /// **'Yours'**
  String get homeHeaderValueYours;

  /// _llmLabel return value shown as LLM card value: local mode with a local-server provider (ollama/llamacpp/lmstudio/vllm)
  ///
  /// In en, this message translates to:
  /// **'Connected to PC'**
  String get homeLlmConnectedToPc;

  /// RuntimeStatusCard label: (small caption above value)
  ///
  /// In en, this message translates to:
  /// **'LLM:'**
  String get homeLlmLabel;

  /// _llmLabel return value shown as LLM card value: remote/cloud mode with a local-server provider
  ///
  /// In en, this message translates to:
  /// **'Local server'**
  String get homeLlmLocalServer;

  /// RuntimeStatusCard value: LLM card when health.provider is null
  ///
  /// In en, this message translates to:
  /// **'No provider set'**
  String get homeLlmNoProvider;

  /// RuntimeStatusCard value: LLM card when runtime unreachable
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get homeLlmNotConnected;

  /// RuntimeStatusCard subtitle: LLM card fallback when model and provider are null
  ///
  /// In en, this message translates to:
  /// **'Pick a provider'**
  String get homeLlmPickProvider;

  /// RuntimeStatusCard subtitle: LLM card when runtime unreachable
  ///
  /// In en, this message translates to:
  /// **'Waiting for the runtime'**
  String get homeLlmWaitingRuntime;

  /// Text() subtitle under the QuickActionsPanel title
  ///
  /// In en, this message translates to:
  /// **'Common tasks, one tap away.'**
  String get homeQuickActionsSubtitle;

  /// Text() right-aligned muted caption in the QuickActionsPanel header row
  ///
  /// In en, this message translates to:
  /// **'Get more done, locally.'**
  String get homeQuickActionsTagline;

  /// Text() panel title next to the bolt icon in QuickActionsPanel
  ///
  /// In en, this message translates to:
  /// **'Quick Actions'**
  String get homeQuickActionsTitle;

  /// QuickAction label (QuickActionsPanel button)
  ///
  /// In en, this message translates to:
  /// **'Pair PC'**
  String get homeQuickPairPc;

  /// RuntimeStatusCard label: (small caption above value)
  ///
  /// In en, this message translates to:
  /// **'Runtime:'**
  String get homeRuntimeLabel;

  /// RuntimeStatusCard value: for RuntimeMode.local
  ///
  /// In en, this message translates to:
  /// **'Local on this phone'**
  String get homeRuntimeLocal;

  /// RuntimeStatusCard subtitle: healthy local runtime tagline
  ///
  /// In en, this message translates to:
  /// **'Fast. Private. Always with you.'**
  String get homeRuntimeLocalTagline;

  /// RuntimeStatusCard subtitle: when health errored in local mode
  ///
  /// In en, this message translates to:
  /// **'Not running — open Termux and run `garra start`'**
  String get homeRuntimeNotRunning;

  /// RuntimeStatusCard value: for RuntimeMode.remote
  ///
  /// In en, this message translates to:
  /// **'Garra on your network'**
  String get homeRuntimeRemote;

  /// RuntimeStatusCard subtitle: when health errored in remote/cloud mode
  ///
  /// In en, this message translates to:
  /// **'Unreachable — check the address'**
  String get homeRuntimeUnreachable;

  /// RuntimeStatusCard subtitle: healthy remote/cloud runtime, version + session count
  ///
  /// In en, this message translates to:
  /// **'v{version} · {count, plural, =1{{count} session} other{{count} sessions}}'**
  String homeRuntimeVersionSessions(String version, int count);

  /// Semantics label of the RuntimeStatusCard (Runtime/LLM status cards under the hero)
  ///
  /// In en, this message translates to:
  /// **'{label} {value}. {subtitle}'**
  String homeStatusCardSemantics(String label, String value, String subtitle);

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'Create and manage agents'**
  String get homeTileAgentsSubtitle;

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'Set up smart workflows'**
  String get homeTileAutomationsSubtitle;

  /// FeatureTile title:
  ///
  /// In en, this message translates to:
  /// **'Chat'**
  String get homeTileChat;

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'Talk with your AI assistant'**
  String get homeTileChatSubtitle;

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'Access and manage files'**
  String get homeTileFilesSubtitle;

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'What Garra remembers'**
  String get homeTileMemorySubtitle;

  /// FeatureTile subtitle:
  ///
  /// In en, this message translates to:
  /// **'Extend what Garra can do'**
  String get homeTileSkillsSubtitle;

  /// Error Text() under the form, generic fallback of _friendlyError
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t sign in. Please try again.'**
  String get loginErrorGeneric;

  /// Error Text() under the form, returned by _friendlyError on 401 / invalid credentials
  ///
  /// In en, this message translates to:
  /// **'Incorrect email or password.'**
  String get loginErrorInvalidCredentials;

  /// Text() greeting under the mascot on the login screen
  ///
  /// In en, this message translates to:
  /// **'Hi! I\'m Garra.'**
  String get loginGreeting;

  /// TextButton label linking to /register
  ///
  /// In en, this message translates to:
  /// **'Don\'t have an account? Create one'**
  String get loginNoAccountCta;

  /// Text() subtitle under the greeting
  ///
  /// In en, this message translates to:
  /// **'Sign in to continue'**
  String get loginSubtitle;

  /// SnackBar content after DELETE returned 404 (existed == false)
  ///
  /// In en, this message translates to:
  /// **'It was already gone'**
  String get memoryAlreadyGone;

  /// copyToClipboard toast: (SnackBar shown after copying the memory text)
  ///
  /// In en, this message translates to:
  /// **'Memory copied'**
  String get memoryCopied;

  /// AlertDialog content (delete confirmation, entry.pinned == false)
  ///
  /// In en, this message translates to:
  /// **'It disappears from the runtime and cannot be undone.'**
  String get memoryDeleteBody;

  /// SnackBar content on delete failure (catch block)
  ///
  /// In en, this message translates to:
  /// **'Could not delete: {error}'**
  String memoryDeleteFailed(String error);

  /// AlertDialog content (delete confirmation, entry.pinned == true)
  ///
  /// In en, this message translates to:
  /// **'It is pinned, but deleting does not ask twice: it disappears from the runtime and cannot be undone.'**
  String get memoryDeletePinnedBody;

  /// AlertDialog title (delete confirmation)
  ///
  /// In en, this message translates to:
  /// **'Delete this memory?'**
  String get memoryDeleteTitle;

  /// SnackBar content after successful DELETE (existed == true)
  ///
  /// In en, this message translates to:
  /// **'Memory deleted'**
  String get memoryDeleted;

  /// AsyncBody emptyText: when recent memory list is empty
  ///
  /// In en, this message translates to:
  /// **'Garra has not stored any memory yet. Chat a little and come back.'**
  String get memoryEmpty;

  /// AsyncBody emptyText: when a search returns nothing
  ///
  /// In en, this message translates to:
  /// **'No memory matches \"{query}\".'**
  String memoryNoMatches(String query);

  /// TextField hintText:
  ///
  /// In en, this message translates to:
  /// **'Search memories'**
  String get memorySearchHint;

  /// FeatureTile title:
  ///
  /// In en, this message translates to:
  /// **'Memory'**
  String get memoryTitle;

  /// enum display name: GarraTab.label getter, rendered as Text() and Semantics label in the bottom nav
  ///
  /// In en, this message translates to:
  /// **'Home'**
  String get navHome;

  /// AndroidNotificationChannel description for channel 'chat_messages'
  ///
  /// In en, this message translates to:
  /// **'Notifications for new chat messages'**
  String get notificationsChannelChatDescription;

  /// AndroidNotificationChannel name for channel id 'chat_messages' (visible in Android system notification settings)
  ///
  /// In en, this message translates to:
  /// **'Chat messages'**
  String get notificationsChannelChatName;

  /// AndroidNotificationChannel description for channel 'sync_status'
  ///
  /// In en, this message translates to:
  /// **'Notifications about sync between your devices'**
  String get notificationsChannelSyncDescription;

  /// AndroidNotificationChannel name for channel id 'sync_status'
  ///
  /// In en, this message translates to:
  /// **'Sync status'**
  String get notificationsChannelSyncName;

  /// AndroidNotificationChannel description for channel 'system'
  ///
  /// In en, this message translates to:
  /// **'System notifications'**
  String get notificationsChannelSystemDescription;

  /// AndroidNotificationChannel name for channel id 'system'
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get notificationsChannelSystemName;

  /// EmptyState text: on the notifications tab
  ///
  /// In en, this message translates to:
  /// **'Nothing yet. Notifications from automations and background agents will show up here.'**
  String get notificationsEmpty;

  /// GarraPage title of NotificationsScreen
  ///
  /// In en, this message translates to:
  /// **'Notifications'**
  String get notificationsTitle;

  /// Second line of the muted Text() under the step-3 headline when mode == RuntimeMode.cloud
  ///
  /// In en, this message translates to:
  /// **'Next: sign in to your account.'**
  String get onboardingDoneNextCloud;

  /// Second line of the muted Text() under the step-3 headline when mode is local or remote
  ///
  /// In en, this message translates to:
  /// **'Your AI lives on your device. The model doesn\'t have to.'**
  String get onboardingDoneNextLocal;

  /// ElevatedButton label of step 3 for local/remote modes (navigates to /home)
  ///
  /// In en, this message translates to:
  /// **'Open Garra'**
  String get onboardingDoneOpenGarra;

  /// First line of the muted Text() under the step-3 headline (_DoneStep); concatenated with the next-step sentence via '\n'
  ///
  /// In en, this message translates to:
  /// **'Runtime: {runtime}.'**
  String onboardingDoneRuntime(String runtime);

  /// Text() centered headline of step 3 (_DoneStep) when the name is empty
  ///
  /// In en, this message translates to:
  /// **'All set.'**
  String get onboardingDoneTitle;

  /// Text() centered headline of step 3 (_DoneStep) when the user typed a name
  ///
  /// In en, this message translates to:
  /// **'All set, {name}.'**
  String onboardingDoneTitleNamed(String name);

  /// _explain(DioException) fallback for RuntimeMode.cloud; shown as warning Text(error!) (line 350)
  ///
  /// In en, this message translates to:
  /// **'Could not reach Garra Cloud'**
  String get onboardingErrorCloudUnreachable;

  /// _explain(DioException) return value for any other HTTP status; shown as warning Text(error!) (line 350)
  ///
  /// In en, this message translates to:
  /// **'HTTP {code} from the gateway'**
  String onboardingErrorHttpStatus(int code);

  /// _explain(DioException) fallback for RuntimeMode.local (no HTTP status); shown as warning Text(error!) (line 350)
  ///
  /// In en, this message translates to:
  /// **'Nothing answered on this phone. Is `garra start` running in Termux?'**
  String get onboardingErrorLocalUnreachable;

  /// _explain(DioException) return value for HTTP 401/403; shown as warning Text(error!) (line 350)
  ///
  /// In en, this message translates to:
  /// **'The gateway asked for an API key'**
  String get onboardingErrorNeedsApiKey;

  /// _explain(DioException) fallback for RuntimeMode.remote; shown as warning Text(error!) (line 350)
  ///
  /// In en, this message translates to:
  /// **'No answer. Same Wi-Fi? Is the gateway bound to 0.0.0.0?'**
  String get onboardingErrorRemoteUnreachable;

  /// Text() field label above the URL TextField (shown for local and remote modes)
  ///
  /// In en, this message translates to:
  /// **'Gateway address'**
  String get onboardingGatewayAddressLabel;

  /// InputDecoration(hintText:) of the API key TextField (remote mode only)
  ///
  /// In en, this message translates to:
  /// **'Only if gateway.api_key is set'**
  String get onboardingGatewayApiKeyHint;

  /// Text() field label above the API key TextField (remote mode only)
  ///
  /// In en, this message translates to:
  /// **'Gateway API key (optional)'**
  String get onboardingGatewayApiKeyLabel;

  /// _probeError assigned in _test(); rendered as warning Text(error!) under the Test connection button (line 350)
  ///
  /// In en, this message translates to:
  /// **'Enter a valid http(s) address'**
  String get onboardingInvalidAddress;

  /// Text() muted subtitle of step 1 (_NameStep)
  ///
  /// In en, this message translates to:
  /// **'Only used for the greeting. It never leaves this device.'**
  String get onboardingNameSubtitle;

  /// Text() headline of step 1 (_NameStep)
  ///
  /// In en, this message translates to:
  /// **'What should Garra\ncall you?'**
  String get onboardingNameTitle;

  /// Text() warning under the address field in _PlainHttpHint, shown while the LAN URL is not https:// (remote mode)
  ///
  /// In en, this message translates to:
  /// **'Plain HTTP: your messages and the API key are readable on this network. Fine on your home Wi-Fi, not on a public one.'**
  String get onboardingPlainHttpWarning;

  /// Text() in _ProbeResult next to the ok/warn icon after a successful health probe (provider absent)
  ///
  /// In en, this message translates to:
  /// **'Garra {version} · {status}'**
  String onboardingProbeResult(String version, String status);

  /// Text() in _ProbeResult after a successful health probe when health.provider is set (conditional suffix of the line-546 s
  ///
  /// In en, this message translates to:
  /// **'Garra {version} · {status} · {provider}'**
  String onboardingProbeResultWithProvider(
    String version,
    String status,
    String provider,
  );

  /// Text() muted subtitle of step 2 (_RuntimeStep)
  ///
  /// In en, this message translates to:
  /// **'Your memory, skills and files stay on the runtime you pick. The LLM can be anywhere.'**
  String get onboardingRuntimeSubtitle;

  /// Text() headline of step 2 (_RuntimeStep)
  ///
  /// In en, this message translates to:
  /// **'Where does\nGarra run?'**
  String get onboardingRuntimeTitle;

  /// Text() muted footer inside the _TermuxHint card (local mode only)
  ///
  /// In en, this message translates to:
  /// **'Then come back and tap \"Test connection\". The app talks to it on 127.0.0.1.'**
  String get onboardingTermuxInstallHint;

  /// Text() heading inside the _TermuxHint card (local mode only)
  ///
  /// In en, this message translates to:
  /// **'Install Garra in Termux (once):'**
  String get onboardingTermuxInstallTitle;

  /// OutlinedButton.icon label of the health probe button (idle state)
  ///
  /// In en, this message translates to:
  /// **'Test connection'**
  String get onboardingTestConnection;

  /// OutlinedButton.icon label while the health probe is in flight (testing == true)
  ///
  /// In en, this message translates to:
  /// **'Testing…'**
  String get onboardingTesting;

  /// ListTile subtitle when device is offline; shows last-seen timestamp string from the sync server
  ///
  /// In en, this message translates to:
  /// **'Last seen: {lastSeen}'**
  String pairDeviceLastSeen(String lastSeen);

  /// ListTile subtitle when device.isOnline is true
  ///
  /// In en, this message translates to:
  /// **'Online'**
  String get pairDeviceOnline;

  /// Section header Text() above the paired devices ListView
  ///
  /// In en, this message translates to:
  /// **'Paired Devices'**
  String get pairPairedDevicesHeader;

  /// SnackBar content shown after a QR code is scanned
  ///
  /// In en, this message translates to:
  /// **'Pairing started...'**
  String get pairPairingStarted;

  /// OutlinedButton.icon label that regenerates the pairing token
  ///
  /// In en, this message translates to:
  /// **'Generate new code'**
  String get pairRegenerateCode;

  /// Instruction Text() above the MobileScanner camera view (contains a hard line break)
  ///
  /// In en, this message translates to:
  /// **'Point the camera at the QR code\non the other device'**
  String get pairScanInstruction;

  /// Instruction Text() above the QrImageView in the Show-QR tab (contains a hard line break)
  ///
  /// In en, this message translates to:
  /// **'Scan this QR code\non the other device'**
  String get pairShowQrInstruction;

  /// _SyncStatusBanner text for SyncStatus.connected (switch arm; effectively unreachable because line 184 returns early for
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get pairSyncConnected;

  /// _SyncStatusBanner text for SyncStatus.connecting
  ///
  /// In en, this message translates to:
  /// **'Connecting to sync server...'**
  String get pairSyncConnecting;

  /// _SyncStatusBanner text for SyncStatus.disconnected
  ///
  /// In en, this message translates to:
  /// **'Disconnected from sync server'**
  String get pairSyncDisconnected;

  /// _SyncStatusBanner text for SyncStatus.error
  ///
  /// In en, this message translates to:
  /// **'Connection error. Reconnecting...'**
  String get pairSyncError;

  /// Tab(text:) label, first tab (show own QR)
  ///
  /// In en, this message translates to:
  /// **'My QR Code'**
  String get pairTabMyQrCode;

  /// Tab(text:) label, second tab (camera scanner)
  ///
  /// In en, this message translates to:
  /// **'Scan'**
  String get pairTabScan;

  /// AppBar title of PairScreen
  ///
  /// In en, this message translates to:
  /// **'Pair Devices'**
  String get pairTitle;

  /// Text() headline fallback when ownerName is empty
  ///
  /// In en, this message translates to:
  /// **'Garra user'**
  String get profileDefaultName;

  /// _Row title: (GarraListTile title)
  ///
  /// In en, this message translates to:
  /// **'Display name'**
  String get profileDisplayName;

  /// InputDecoration(hintText:) of the name TextField (_NameStep)
  ///
  /// In en, this message translates to:
  /// **'Your name'**
  String get profileNameHint;

  /// Text() under the name, fallback when config is null (otherwise config.mode.title)
  ///
  /// In en, this message translates to:
  /// **'No runtime'**
  String get profileNoRuntime;

  /// _Row title: (GarraListTile title)
  ///
  /// In en, this message translates to:
  /// **'Paired devices'**
  String get profilePairedDevices;

  /// _Row subtitle:
  ///
  /// In en, this message translates to:
  /// **'Sync sessions across your devices'**
  String get profilePairedDevicesSubtitle;

  /// _Row subtitle: runtime summary (mode · base URL, optionally · vVERSION)
  ///
  /// In en, this message translates to:
  /// **'{mode} · {baseUrl}'**
  String profileRuntimeSummary(String mode, String baseUrl);

  /// _Row subtitle: nested version suffix appended to the runtime summary when health != null
  ///
  /// In en, this message translates to:
  /// **'{mode} · {baseUrl} · v{version}'**
  String profileRuntimeSummaryWithVersion(
    String mode,
    String baseUrl,
    String version,
  );

  /// _Row subtitle:
  ///
  /// In en, this message translates to:
  /// **'Runtime, account and app details'**
  String get profileSettingsSubtitle;

  /// _Row subtitle: (sign out row)
  ///
  /// In en, this message translates to:
  /// **'Clear the cloud session on this device'**
  String get profileSignOutSubtitle;

  /// GarraPage title:
  ///
  /// In en, this message translates to:
  /// **'Profile'**
  String get profileTitle;

  /// Footer Text() under the provider list
  ///
  /// In en, this message translates to:
  /// **'API keys are configured on the runtime (garra init / web console), never stored on this phone.'**
  String get providersApiKeysNote;

  /// Text() inside the pill badge shown as trailing on the default provider row
  ///
  /// In en, this message translates to:
  /// **'default'**
  String get providersDefaultBadge;

  /// AsyncBody emptyText when /api/providers returns an empty list
  ///
  /// In en, this message translates to:
  /// **'The runtime reported no providers. Run `garra init` to configure one.'**
  String get providersEmpty;

  /// First segment of the provider row subtitle when the provider has no model configured or listed
  ///
  /// In en, this message translates to:
  /// **'no model'**
  String get providersNoModel;

  /// Status word in the provider row subtitle (info.active == true), joined with ' · '
  ///
  /// In en, this message translates to:
  /// **'active'**
  String get providersStatusActive;

  /// Status word in the provider row subtitle (inactive, no key required)
  ///
  /// In en, this message translates to:
  /// **'inactive'**
  String get providersStatusInactive;

  /// Status word in the provider row subtitle (inactive and needsApiKey)
  ///
  /// In en, this message translates to:
  /// **'needs API key'**
  String get providersStatusNeedsApiKey;

  /// QuickAction label (QuickActionsPanel button)
  ///
  /// In en, this message translates to:
  /// **'Providers'**
  String get providersTitle;

  /// Text() in the offline-queue banner (cs.error background) when offline and no pending messages
  ///
  /// In en, this message translates to:
  /// **'No connection'**
  String get queueOffline;

  /// Text() in the offline-queue banner (cs.error background) when offline AND pendingCount > 0
  ///
  /// In en, this message translates to:
  /// **'No connection - {count, plural, =1{{count} pending message} other{{count} pending messages}}'**
  String queueOfflineWithPending(int count);

  /// Text() in the offline-queue banner (cs.secondary background) when online, idle, with pending messages
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{{count} pending message} other{{count} pending messages}}'**
  String queuePending(int count);

  /// Text() in the offline-queue banner (cs.tertiary background) while status.isSyncing, next to a small spinner
  ///
  /// In en, this message translates to:
  /// **'Syncing {count, plural, =1{{count} message} other{{count} messages}}...'**
  String queueSyncing(int count);

  /// InputDecoration labelText of the confirm-password field
  ///
  /// In en, this message translates to:
  /// **'Confirm password'**
  String get registerConfirmPasswordLabel;

  /// Text() headline title of the register screen (same text reused as button label at line 124)
  ///
  /// In en, this message translates to:
  /// **'Create account'**
  String get registerCreateAccount;

  /// Error Text() under the form, returned by _friendlyError on 409 / already registered
  ///
  /// In en, this message translates to:
  /// **'This email is already registered.'**
  String get registerErrorEmailTaken;

  /// Error Text() under the form, generic fallback of _friendlyError
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t create account. Please try again.'**
  String get registerErrorGeneric;

  /// TextButton label linking to /login
  ///
  /// In en, this message translates to:
  /// **'Already have an account? Sign in'**
  String get registerHaveAccountCta;

  /// TextFormField validator error for the confirm-password field
  ///
  /// In en, this message translates to:
  /// **'Passwords don\'t match'**
  String get registerPasswordsMismatch;

  /// RuntimeModeLabels.description getter — subtitle of the cloud runtime option in onboarding
  ///
  /// In en, this message translates to:
  /// **'Use the hosted Garra service with your account.'**
  String get runtimeModeCloudDescription;

  /// RuntimeStatusCard value: for RuntimeMode.cloud
  ///
  /// In en, this message translates to:
  /// **'Garra Cloud'**
  String get runtimeModeCloudTitle;

  /// RuntimeModeLabels.description getter — subtitle of the local runtime option in onboarding
  ///
  /// In en, this message translates to:
  /// **'Garra runs inside Termux on this device. Memory, skills and files stay here.'**
  String get runtimeModeLocalDescription;

  /// RuntimeModeLabels.title getter (enum display name) — runtime option title shown in onboarding cards and as the runtime l
  ///
  /// In en, this message translates to:
  /// **'On this phone'**
  String get runtimeModeLocalTitle;

  /// RuntimeModeLabels.description getter — subtitle of the remote (LAN) runtime option in onboarding
  ///
  /// In en, this message translates to:
  /// **'Connect to a Garra gateway on your PC or home server over Wi-Fi.'**
  String get runtimeModeRemoteDescription;

  /// RuntimeModeLabels.title getter (enum display name) for RuntimeMode.remote
  ///
  /// In en, this message translates to:
  /// **'Another Garra'**
  String get runtimeModeRemoteTitle;

  /// _InfoRow label: (account creation date row)
  ///
  /// In en, this message translates to:
  /// **'Joined'**
  String get settingsAccountCreatedLabel;

  /// _InfoRow label: (shortened user UUID row)
  ///
  /// In en, this message translates to:
  /// **'ID'**
  String get settingsAccountIdLabel;

  /// _ErrorState headline Text() when authStateProvider errors
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load account details'**
  String get settingsAccountLoadError;

  /// Card heading Text() of the account card (cloud mode only)
  ///
  /// In en, this message translates to:
  /// **'Account'**
  String get settingsAccountTitle;

  /// OutlinedButton.icon label that pushes /onboarding
  ///
  /// In en, this message translates to:
  /// **'Change runtime'**
  String get settingsChangeRuntime;

  /// settings language card
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get settingsLanguageEnglish;

  /// settings language card
  ///
  /// In en, this message translates to:
  /// **'Applies immediately, no restart needed.'**
  String get settingsLanguageHint;

  /// settings language card
  ///
  /// In en, this message translates to:
  /// **'Português (Brasil)'**
  String get settingsLanguagePortuguese;

  /// settings language card
  ///
  /// In en, this message translates to:
  /// **'System default'**
  String get settingsLanguageSystem;

  /// settings language card
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get settingsLanguageTitle;

  /// FilledButton.tonalIcon label for logout
  ///
  /// In en, this message translates to:
  /// **'Sign out'**
  String get settingsLogout;

  /// AlertDialog content of the logout confirmation
  ///
  /// In en, this message translates to:
  /// **'You\'ll be signed out and will need to sign in again the next time you open the app.'**
  String get settingsLogoutConfirmBody;

  /// AlertDialog title of the logout confirmation
  ///
  /// In en, this message translates to:
  /// **'Sign out?'**
  String get settingsLogoutConfirmTitle;

  /// _InfoRow label: (runtime base URL row)
  ///
  /// In en, this message translates to:
  /// **'Address'**
  String get settingsRuntimeAddressLabel;

  /// _InfoRow value while the health probe is still loading (h == null)
  ///
  /// In en, this message translates to:
  /// **'checking…'**
  String get settingsRuntimeChecking;

  /// _InfoRow label: (runtime mode row)
  ///
  /// In en, this message translates to:
  /// **'Mode'**
  String get settingsRuntimeModeLabel;

  /// _InfoRow label: (LLM provider row, shown only when h.provider != null)
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get settingsRuntimeProviderLabel;

  /// _InfoRow value assembled from provider and optional model, e.g. 'openrouter / gpt-4o'
  ///
  /// In en, this message translates to:
  /// **'{provider} / {model}'**
  String settingsRuntimeProviderModel(String provider, String model);

  /// _InfoRow label: (runtime health row)
  ///
  /// In en, this message translates to:
  /// **'Status'**
  String get settingsRuntimeStatusLabel;

  /// _InfoRow value when health loaded: status word plus version, e.g. 'healthy · v0.4.2'
  ///
  /// In en, this message translates to:
  /// **'{status} · v{version}'**
  String settingsRuntimeStatusVersion(String status, String version);

  /// Card heading Text() of the runtime card
  ///
  /// In en, this message translates to:
  /// **'Runtime'**
  String get settingsRuntimeTitle;

  /// _InfoRow value when healthValue.hasError (runtime health probe failed)
  ///
  /// In en, this message translates to:
  /// **'unreachable'**
  String get settingsRuntimeUnreachable;

  /// Centered Text() empty state when authState resolves to null while on the screen (race before router redirect)
  ///
  /// In en, this message translates to:
  /// **'Session expired. Redirecting…'**
  String get settingsSessionExpiredRedirecting;

  /// tooltip: of the AppBar IconButton that opens /settings
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settingsTitle;

  /// Segment of the skill row subtitle shown when skill.deprecated is true
  ///
  /// In en, this message translates to:
  /// **'deprecated'**
  String get skillsDeprecated;

  /// Segment of the skill row subtitle shown only when failCount > 0, hand-rolled plural
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 fail} other{{count} fails}}'**
  String skillsFailCount(int count);

  /// EmptyState text when /api/learning/skills returns an empty list
  ///
  /// In en, this message translates to:
  /// **'No learned skills yet. Garra mines them from your sessions (ADR 0010).'**
  String get skillsNoLearnedSkills;

  /// Segment of the skill row subtitle (score label + formatted value), joined with ' · '
  ///
  /// In en, this message translates to:
  /// **'score {score}'**
  String skillsScore(String score);

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'Learned skills'**
  String get skillsSectionLearned;

  /// SectionHeader label
  ///
  /// In en, this message translates to:
  /// **'Slash commands'**
  String get skillsSectionSlashCommands;

  /// FeatureTile title:
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get skillsTitle;

  /// Segment of the skill row subtitle (version prefix), joined with ' · '
  ///
  /// In en, this message translates to:
  /// **'v{version}'**
  String skillsVersion(String version);

  /// Text() hint below the mic button when not recording (fontSize 10, 40% alpha)
  ///
  /// In en, this message translates to:
  /// **'Hold to record'**
  String get voiceHoldToRecord;

  /// assigned to _error, rendered via Text(_error!) in cs.error color above the record button
  ///
  /// In en, this message translates to:
  /// **'Microphone permission required'**
  String get voiceMicPermissionRequired;

  /// assigned to _error in catch block of _stopRecording, rendered via Text(_error!) in cs.error color
  ///
  /// In en, this message translates to:
  /// **'Could not process audio: {error}'**
  String voiceProcessAudioError(String error);

  /// assigned to _error in catch block, rendered via Text(_error!) in cs.error color
  ///
  /// In en, this message translates to:
  /// **'Could not start recording: {error}'**
  String voiceStartRecordingError(String error);

  /// Text() next to CircularProgressIndicator while _isProcessing (waiting for transcription)
  ///
  /// In en, this message translates to:
  /// **'Transcribing...'**
  String get voiceTranscribing;

  /// Fallback transcription text returned by ApiService.transcribeAudio when the STT response has no `text` — flows into the
  ///
  /// In en, this message translates to:
  /// **'Transcription unavailable'**
  String get voiceTranscriptionUnavailable;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'pt'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'pt':
      return AppLocalizationsPt();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}

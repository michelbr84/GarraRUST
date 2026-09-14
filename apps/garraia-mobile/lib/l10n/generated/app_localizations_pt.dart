// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Portuguese (`pt`).
class AppLocalizationsPt extends AppLocalizations {
  AppLocalizationsPt([String locale = 'pt']) : super(locale);

  @override
  String get activityCopyLogToast => 'Log copiado';

  @override
  String get activityCopyLogTooltip => 'Copiar log';

  @override
  String get activityCurrentSession => 'Sessão atual';

  @override
  String get activityLogEmpty => '(vazio)';

  @override
  String activityMessageCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count mensagens',
      one: '1 mensagem',
    );
    return '$_temp0';
  }

  @override
  String get activityNoSessions =>
      'Nenhuma sessão no runtime. Comece uma conversa.';

  @override
  String get activitySectionRuntimeLog => 'Log do runtime';

  @override
  String get activitySectionSessions => 'Sessões';

  @override
  String get activityTitle => 'Atividade';

  @override
  String get agentsMcpConnected => 'conectado';

  @override
  String get agentsMcpDisconnected => 'desconectado';

  @override
  String get agentsNoMcpServers =>
      'Nenhum servidor MCP configurado neste runtime.';

  @override
  String get agentsNoModes => 'Nenhum modo de agente informado.';

  @override
  String get agentsSectionMcpServers => 'Servidores MCP';

  @override
  String get agentsSectionModes => 'Modos de agente';

  @override
  String get agentsTitle => 'Agentes';

  @override
  String agentsToolCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count ferramentas',
      one: '1 ferramenta',
    );
    return '$_temp0';
  }

  @override
  String get appTitle => 'Garra Mobile';

  @override
  String get automationsEditorComingSoon =>
      'Este runtime suporta automações, mas o editor no celular chega em uma versão futura.';

  @override
  String get automationsTitle => 'Automações';

  @override
  String get automationsUnavailableWhy =>
      'O runtime Garra conectado ainda não expõe uma API de agendamento. Hoje as tarefas agendadas são gerenciadas pela CLI do Garra; esta tela vai ser ativada automaticamente quando o runtime anunciar esse recurso.';

  @override
  String get biometricPromptReason => 'Autentique-se para acessar o Garra';

  @override
  String get chatCodeCopiedToast => 'Código copiado';

  @override
  String get chatCopyCodeTooltip => 'Copiar código';

  @override
  String get chatCopyMessageTooltip => 'Copiar mensagem';

  @override
  String get chatEmptySubtitle =>
      'Seu assistente pessoal de IA.\nMe pergunte qualquer coisa!';

  @override
  String get chatEmptyTitle => 'Oi! Eu sou o Garra.';

  @override
  String get chatErrorConnectionDropped =>
      'A conexão caiu antes de a resposta terminar.';

  @override
  String get chatErrorReplyMissing =>
      'A resposta do gateway veio sem conteúdo.';

  @override
  String get chatErrorRuntimeReported => 'O runtime informou um erro.';

  @override
  String get chatErrorSessionIdMissing =>
      'O gateway não devolveu um id de sessão.';

  @override
  String get chatInputHint => 'Digite uma mensagem...';

  @override
  String get chatKeyboardTooltip => 'Teclado';

  @override
  String chatLoadConversationError(String error) {
    return 'Não foi possível carregar a conversa: $error';
  }

  @override
  String get chatMessageCopiedToast => 'Mensagem copiada';

  @override
  String get chatMessageQueuedOffline =>
      'Mensagem salva para envio quando você estiver online';

  @override
  String get chatNewSession => 'Nova sessão';

  @override
  String get chatPairDevicesTooltip => 'Parear dispositivos';

  @override
  String chatStreamingRunningTool(String tool) {
    return 'rodando $tool';
  }

  @override
  String get chatSuggestionFunFact => 'Me conta uma curiosidade insana';

  @override
  String get chatSuggestionJoke => 'Me conta uma piada';

  @override
  String get chatSuggestionOrganizeDay => 'Me ajuda a organizar meu dia';

  @override
  String get chatSuggestionSuperpower => 'Qual é o seu superpoder?';

  @override
  String get chatSuggestionWhatCanYouDo => 'O que você consegue fazer?';

  @override
  String get chatSuggestionWhoAreYou => 'Quem é você, Garra?';

  @override
  String get chatVoiceNoRuntimeConfigured => 'Nenhum runtime configurado';

  @override
  String get chatVoiceTooltip => 'Voz';

  @override
  String get chatVoiceTranscribeError => 'Erro ao transcrever áudio';

  @override
  String get commonBack => 'Voltar';

  @override
  String get commonBrand => 'Garra';

  @override
  String get commonCancel => 'Cancelar';

  @override
  String get commonChecking => 'Verificando…';

  @override
  String get commonContinue => 'Continuar';

  @override
  String get commonCopied => 'Copiado';

  @override
  String get commonCopy => 'Copiar';

  @override
  String get commonDelete => 'Apagar';

  @override
  String get commonEmailInvalid => 'E-mail inválido';

  @override
  String get commonEmailLabel => 'E-mail';

  @override
  String get commonEmptyNothingYet => 'Nada por aqui ainda.';

  @override
  String get commonErrorNoConnection => 'Sem conexão. Verifique sua internet.';

  @override
  String commonErrorWithDetail(String error) {
    return 'Erro: $error';
  }

  @override
  String commonFeatureUnavailableOnRuntime(String feature) {
    return '$feature não está disponível neste runtime';
  }

  @override
  String get commonLogout => 'Sair';

  @override
  String get commonNotConfigured => 'Não configurado';

  @override
  String get commonNotSet => 'Não definido';

  @override
  String get commonPasswordLabel => 'Senha';

  @override
  String get commonPasswordMinLength => 'Mínimo 8 caracteres';

  @override
  String get commonRefresh => 'Atualizar';

  @override
  String get commonRetry => 'Tentar novamente';

  @override
  String get commonSave => 'Salvar';

  @override
  String get commonSaving => 'Salvando…';

  @override
  String get commonSignIn => 'Entrar';

  @override
  String get errorCouldNotReachRuntime =>
      'Não foi possível conectar ao runtime';

  @override
  String get errorNoRuntimeConfigured => 'Nenhum runtime do Garra configurado.';

  @override
  String get filesNoProjects =>
      'Ainda não há projetos neste runtime. Crie um pelo console do Garra ou pela CLI.';

  @override
  String get filesProjectNoTrackedFiles =>
      'Este projeto não tem arquivos rastreados.';

  @override
  String get filesTitle => 'Arquivos';

  @override
  String get homeBannerHeadline => 'IA LOCAL. UM VOCÊ MAIS BRILHANTE.';

  @override
  String get homeBannerSubtitle => 'Mais controle. Um você mais capaz.';

  @override
  String homeFeatureTileSemantics(String title, String subtitle) {
    return '$title. $subtitle';
  }

  @override
  String homeFeatureTileSemanticsUnavailable(String title, String subtitle) {
    return '$title. $subtitle. Indisponível neste runtime';
  }

  @override
  String get homeFeatureUnavailable => 'Indisponível';

  @override
  String get homeGreetingAnonymous => 'Olá 👋';

  @override
  String get homeGreetingCaption =>
      'Dispositivo menor.\nPossibilidades maiores.';

  @override
  String homeGreetingNamed(String name) {
    return 'Olá, $name 👋';
  }

  @override
  String get homeGreetingSubtitle =>
      'Bom te ver de novo.\nSeu assistente de IA está pronto.';

  @override
  String get homeHeaderQuote => '“IA que trabalha para você.\nDo seu jeito.”';

  @override
  String get homeHeaderTagline => 'Assistente de IA local-first';

  @override
  String get homeHeaderValuePowerful => 'Poderoso';

  @override
  String get homeHeaderValuePrivate => 'Privado';

  @override
  String get homeHeaderValueYours => 'Seu';

  @override
  String get homeLlmConnectedToPc => 'Conectado ao PC';

  @override
  String get homeLlmLabel => 'LLM:';

  @override
  String get homeLlmLocalServer => 'Servidor local';

  @override
  String get homeLlmNoProvider => 'Nenhum provedor definido';

  @override
  String get homeLlmNotConnected => 'Não conectado';

  @override
  String get homeLlmPickProvider => 'Escolha um provedor';

  @override
  String get homeLlmWaitingRuntime => 'Aguardando o runtime';

  @override
  String get homeQuickActionsSubtitle =>
      'Tarefas comuns, a um toque de distância.';

  @override
  String get homeQuickActionsTagline => 'Faça mais, localmente.';

  @override
  String get homeQuickActionsTitle => 'Ações rápidas';

  @override
  String get homeQuickPairPc => 'Parear PC';

  @override
  String get homeRuntimeLabel => 'Runtime:';

  @override
  String get homeRuntimeLocal => 'Local neste celular';

  @override
  String get homeRuntimeLocalTagline => 'Rápido. Privado. Sempre com você.';

  @override
  String get homeRuntimeNotRunning =>
      'Não está rodando — abra o Termux e execute `garra start`';

  @override
  String get homeRuntimeRemote => 'Garra na sua rede';

  @override
  String get homeRuntimeUnreachable => 'Inacessível — verifique o endereço';

  @override
  String homeRuntimeVersionSessions(String version, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count sessões',
      one: '$count sessão',
    );
    return 'v$version · $_temp0';
  }

  @override
  String homeStatusCardSemantics(String label, String value, String subtitle) {
    return '$label $value. $subtitle';
  }

  @override
  String get homeTileAgentsSubtitle => 'Crie e gerencie agentes';

  @override
  String get homeTileAutomationsSubtitle => 'Configure fluxos inteligentes';

  @override
  String get homeTileChat => 'Chat';

  @override
  String get homeTileChatSubtitle => 'Converse com seu assistente de IA';

  @override
  String get homeTileFilesSubtitle => 'Acesse e gerencie arquivos';

  @override
  String get homeTileMemorySubtitle => 'O que o Garra lembra';

  @override
  String get homeTileSkillsSubtitle => 'Amplie o que o Garra pode fazer';

  @override
  String get loginErrorGeneric => 'Erro ao entrar. Tente novamente.';

  @override
  String get loginErrorInvalidCredentials => 'E-mail ou senha incorretos.';

  @override
  String get loginGreeting => 'Olá! Eu sou o Garra.';

  @override
  String get loginNoAccountCta => 'Não tem conta? Criar agora';

  @override
  String get loginSubtitle => 'Faça login para continuar';

  @override
  String get memoryAlreadyGone => 'Já não existia';

  @override
  String get memoryCopied => 'Memória copiada';

  @override
  String get memoryDeleteBody => 'Some do runtime e não dá para desfazer.';

  @override
  String memoryDeleteFailed(String error) {
    return 'Não deu para apagar: $error';
  }

  @override
  String get memoryDeletePinnedBody =>
      'Ela está fixada, mas apagar não pergunta duas vezes: some do runtime e não dá para desfazer.';

  @override
  String get memoryDeleteTitle => 'Apagar esta memória?';

  @override
  String get memoryDeleted => 'Memória apagada';

  @override
  String get memoryEmpty =>
      'O Garra ainda não guardou nenhuma memória. Converse um pouco e volte depois.';

  @override
  String memoryNoMatches(String query) {
    return 'Nenhuma memória corresponde a \"$query\".';
  }

  @override
  String get memorySearchHint => 'Buscar memórias';

  @override
  String get memoryTitle => 'Memória';

  @override
  String get navHome => 'Início';

  @override
  String get notificationsChannelChatDescription =>
      'Notificações de novas mensagens do chat';

  @override
  String get notificationsChannelChatName => 'Mensagens do chat';

  @override
  String get notificationsChannelSyncDescription =>
      'Notificações de sincronização entre dispositivos';

  @override
  String get notificationsChannelSyncName => 'Status de sincronização';

  @override
  String get notificationsChannelSystemDescription => 'Notificações do sistema';

  @override
  String get notificationsChannelSystemName => 'Sistema';

  @override
  String get notificationsEmpty =>
      'Nada por aqui ainda. Notificações de automações e agentes em segundo plano vão aparecer aqui.';

  @override
  String get notificationsTitle => 'Notificações';

  @override
  String get onboardingDoneNextCloud => 'Próximo passo: entre na sua conta.';

  @override
  String get onboardingDoneNextLocal =>
      'Sua IA mora no seu aparelho. O modelo não precisa.';

  @override
  String get onboardingDoneOpenGarra => 'Abrir o Garra';

  @override
  String onboardingDoneRuntime(String runtime) {
    return 'Runtime: $runtime.';
  }

  @override
  String get onboardingDoneTitle => 'Tudo pronto.';

  @override
  String onboardingDoneTitleNamed(String name) {
    return 'Tudo pronto, $name.';
  }

  @override
  String get onboardingErrorCloudUnreachable =>
      'Não foi possível alcançar o Garra Cloud';

  @override
  String onboardingErrorHttpStatus(int code) {
    return 'O gateway respondeu HTTP $code';
  }

  @override
  String get onboardingErrorLocalUnreachable =>
      'Nada respondeu neste celular. O `garra start` está rodando no Termux?';

  @override
  String get onboardingErrorNeedsApiKey => 'O gateway pediu uma chave de API';

  @override
  String get onboardingErrorRemoteUnreachable =>
      'Sem resposta. Está na mesma rede Wi-Fi? O gateway está escutando em 0.0.0.0?';

  @override
  String get onboardingGatewayAddressLabel => 'Endereço do gateway';

  @override
  String get onboardingGatewayApiKeyHint =>
      'Só se gateway.api_key estiver definido';

  @override
  String get onboardingGatewayApiKeyLabel =>
      'Chave de API do gateway (opcional)';

  @override
  String get onboardingInvalidAddress => 'Digite um endereço http(s) válido';

  @override
  String get onboardingNameSubtitle =>
      'Usado só na saudação. Nunca sai deste aparelho.';

  @override
  String get onboardingNameTitle => 'Como o Garra\ndeve te chamar?';

  @override
  String get onboardingPlainHttpWarning =>
      'HTTP sem criptografia: suas mensagens e a chave de API ficam legíveis nesta rede. Tudo bem no Wi-Fi de casa, não em uma rede pública.';

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
      'Sua memória, skills e arquivos ficam no runtime que você escolher. O LLM pode estar em qualquer lugar.';

  @override
  String get onboardingRuntimeTitle => 'Onde o Garra\nvai rodar?';

  @override
  String get onboardingTermuxInstallHint =>
      'Depois volte aqui e toque em \"Testar conexão\". O app fala com ele em 127.0.0.1.';

  @override
  String get onboardingTermuxInstallTitle =>
      'Instale o Garra no Termux (uma vez só):';

  @override
  String get onboardingTestConnection => 'Testar conexão';

  @override
  String get onboardingTesting => 'Testando…';

  @override
  String pairDeviceLastSeen(String lastSeen) {
    return 'Visto: $lastSeen';
  }

  @override
  String get pairDeviceOnline => 'Online';

  @override
  String get pairPairedDevicesHeader => 'Dispositivos pareados';

  @override
  String get pairPairingStarted => 'Pareamento iniciado...';

  @override
  String get pairRegenerateCode => 'Gerar novo código';

  @override
  String get pairScanInstruction =>
      'Aponte a câmera para o QR Code\ndo outro dispositivo';

  @override
  String get pairShowQrInstruction =>
      'Escaneie este QR Code\nno outro dispositivo';

  @override
  String get pairSyncConnected => 'Conectado';

  @override
  String get pairSyncConnecting => 'Conectando ao servidor de sincronização...';

  @override
  String get pairSyncDisconnected =>
      'Desconectado do servidor de sincronização';

  @override
  String get pairSyncError => 'Erro de conexão. Tentando reconectar...';

  @override
  String get pairTabMyQrCode => 'Meu QR Code';

  @override
  String get pairTabScan => 'Escanear';

  @override
  String get pairTitle => 'Parear dispositivos';

  @override
  String get profileDefaultName => 'Usuário Garra';

  @override
  String get profileDisplayName => 'Nome de exibição';

  @override
  String get profileNameHint => 'Seu nome';

  @override
  String get profileNoRuntime => 'Sem runtime';

  @override
  String get profilePairedDevices => 'Dispositivos pareados';

  @override
  String get profilePairedDevicesSubtitle =>
      'Sincronize sessões entre seus dispositivos';

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
  String get profileSettingsSubtitle => 'Runtime, conta e detalhes do app';

  @override
  String get profileSignOutSubtitle =>
      'Encerra a sessão na nuvem neste dispositivo';

  @override
  String get profileTitle => 'Perfil';

  @override
  String get providersApiKeysNote =>
      'As chaves de API são configuradas no runtime (garra init / console web) e nunca ficam salvas neste celular.';

  @override
  String get providersDefaultBadge => 'padrão';

  @override
  String get providersEmpty =>
      'O runtime não informou nenhum provedor. Rode `garra init` para configurar um.';

  @override
  String get providersNoModel => 'sem modelo';

  @override
  String get providersStatusActive => 'ativo';

  @override
  String get providersStatusInactive => 'inativo';

  @override
  String get providersStatusNeedsApiKey => 'precisa de chave de API';

  @override
  String get providersTitle => 'Provedores';

  @override
  String get queueOffline => 'Sem conexão';

  @override
  String queueOfflineWithPending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count mensagens pendentes',
      one: '$count mensagem pendente',
    );
    return 'Sem conexão - $_temp0';
  }

  @override
  String queuePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count mensagens pendentes',
      one: '$count mensagem pendente',
    );
    return '$_temp0';
  }

  @override
  String queueSyncing(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count mensagens',
      one: '$count mensagem',
    );
    return 'Sincronizando $_temp0...';
  }

  @override
  String get registerConfirmPasswordLabel => 'Confirmar senha';

  @override
  String get registerCreateAccount => 'Criar conta';

  @override
  String get registerErrorEmailTaken => 'Este e-mail já está cadastrado.';

  @override
  String get registerErrorGeneric => 'Erro ao criar conta. Tente novamente.';

  @override
  String get registerHaveAccountCta => 'Já tem conta? Entrar';

  @override
  String get registerPasswordsMismatch => 'As senhas não coincidem';

  @override
  String get runtimeModeCloudDescription =>
      'Use o serviço Garra hospedado com a sua conta.';

  @override
  String get runtimeModeCloudTitle => 'Garra Cloud';

  @override
  String get runtimeModeLocalDescription =>
      'O Garra roda dentro do Termux neste aparelho. Memória, skills e arquivos ficam aqui.';

  @override
  String get runtimeModeLocalTitle => 'Neste celular';

  @override
  String get runtimeModeRemoteDescription =>
      'Conecte a um gateway Garra no seu PC ou servidor de casa pelo Wi-Fi.';

  @override
  String get runtimeModeRemoteTitle => 'Outro Garra';

  @override
  String get settingsAccountCreatedLabel => 'Cadastro';

  @override
  String get settingsAccountIdLabel => 'ID';

  @override
  String get settingsAccountLoadError => 'Erro ao carregar informações';

  @override
  String get settingsAccountTitle => 'Conta';

  @override
  String get settingsChangeRuntime => 'Trocar runtime';

  @override
  String get settingsLanguageEnglish => 'English';

  @override
  String get settingsLanguageHint => 'Aplica na hora, sem reiniciar o app.';

  @override
  String get settingsLanguagePortuguese => 'Português (Brasil)';

  @override
  String get settingsLanguageSystem => 'Padrão do sistema';

  @override
  String get settingsLanguageTitle => 'Idioma';

  @override
  String get settingsLogout => 'Sair da conta';

  @override
  String get settingsLogoutConfirmBody =>
      'Você será desconectado e precisará entrar novamente na próxima vez que abrir o app.';

  @override
  String get settingsLogoutConfirmTitle => 'Sair da conta?';

  @override
  String get settingsRuntimeAddressLabel => 'Endereço';

  @override
  String get settingsRuntimeChecking => 'verificando…';

  @override
  String get settingsRuntimeModeLabel => 'Modo';

  @override
  String get settingsRuntimeProviderLabel => 'Provedor';

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
  String get settingsRuntimeUnreachable => 'inacessível';

  @override
  String get settingsSessionExpiredRedirecting =>
      'Sessão expirada. Redirecionando…';

  @override
  String get settingsTitle => 'Configurações';

  @override
  String get skillsDeprecated => 'obsoleta';

  @override
  String skillsFailCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count falhas',
      one: '1 falha',
    );
    return '$_temp0';
  }

  @override
  String get skillsNoLearnedSkills =>
      'Nenhuma skill aprendida ainda. O Garra as extrai das suas sessões (ADR 0010).';

  @override
  String skillsScore(String score) {
    return 'pontuação $score';
  }

  @override
  String get skillsSectionLearned => 'Skills aprendidas';

  @override
  String get skillsSectionSlashCommands => 'Comandos de barra';

  @override
  String get skillsTitle => 'Skills';

  @override
  String skillsVersion(String version) {
    return 'v$version';
  }

  @override
  String get voiceHoldToRecord => 'Segure para gravar';

  @override
  String get voiceMicPermissionRequired => 'Permissão de microfone necessária';

  @override
  String voiceProcessAudioError(String error) {
    return 'Erro ao processar áudio: $error';
  }

  @override
  String voiceStartRecordingError(String error) {
    return 'Erro ao iniciar gravação: $error';
  }

  @override
  String get voiceTranscribing => 'Transcrevendo...';

  @override
  String get voiceTranscriptionUnavailable => 'Transcrição indisponível';
}

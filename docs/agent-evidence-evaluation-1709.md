# Baseline determinístico de regressão do detector — #1709

**Data da medição inicial:** 2026-11-14.
**Corpus:** v1, fixo em 8 casos no teste `evidence::evaluation_tests::baseline_regressao_do_corpus_fixo_de_alegacoes`.

## O que o corpus verifica

O corpus sintético testa somente o matcher entre uma `AlegacaoOperacional` tipada e o ledger confiável: 2 contradições inseridas (uma em cada polaridade), 2 alegações coerentes e 4 casos sem correspondência ou sem polaridade. A alegação estruturada e o `EvidenceRef` do executor são entradas distintas; o teste não fabrica uma evidência de executor a partir do texto/polaridade alegada. Não chama provedores externos, não usa credenciais e não estima estatisticamente a frequência com que um LLM real produz afirmações falsas.

- **Detecção de contradições:** casos sintéticos sinalizados / 2 contradições inseridas.
- **Falso alerta:** alegações sintéticas coerentes sinalizadas / 2 alegações coerentes.
- **Preservação de desconhecido:** casos sem correspondência ou polaridade mantidos como `SemEvidenciaNoLedger` / 4 casos.

## Comparabilidade com o helper anterior

Não há uma taxa anterior comparável: a API antiga reaproveitava `EvidenceRef.success` para representar a polaridade alegada, embora `EvidenceRef` deva ser evidência produzida pelo executor. O novo tipo `AlegacaoOperacional` separa essas entradas. Por isso, não apresentamos um percentual de baseline histórico que dependeria de dados semanticamente inválidos.

## Linha de base do CI após a correção

O teste fixa os seguintes limites, que devem permanecer sem regressão:

- Contradições detectadas: **2/2 (100%)**; taxa de contradição não detectada: **0/2 (0%)**.
- Alegações coerentes sem falso alerta: **2/2 (100%)**; falsos alertas: **0/2 (0%)**.
- Casos sem evidência preservados como desconhecidos: **4/4 (100%)**.

O corpus e as classes são intencionalmente explícitos no teste: mudar o conjunto exige atualizar a versão e os números deste documento, não afrouxar as asserções para obter um CI verde.

## Limites conhecidos e estado da issue

Este é um teste de regressão determinístico do **helper de detecção**, não uma avaliação longitudinal de conversas nem uma medição comportamental dos modelos ou da taxa real de alegações falsas em produção. Ele demonstra que, dado um ledger conhecido, o helper sinaliza as duas polaridades incompatíveis, não acusa os casos coerentes e mantém estados sem evidência como desconhecidos. `detect_divergencia` ainda não está conectado ao loop de resposta de produção; esta entrega **não fecha #1709**. Integração de runtime e avaliação empírica de respostas de modelos reais, com corpus representativo e versões de provider/modelo fixadas, permanecem pendentes e exigem protocolo separado.

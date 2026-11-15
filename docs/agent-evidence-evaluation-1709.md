# Baseline determinístico de alegações operacionais — #1709

**Data da medição inicial:** 2026-11-14.
**Corpus:** v1, fixo em 7 casos no teste `evidence::evaluation_tests::baseline_longitudinal_do_corpus_fixo_de_alegacoes`.

## O que a métrica mede

O corpus avalia o contrato de `detect_divergencia` entre uma alegação estruturada e o resultado do executor: 2 contradições conhecidas (uma em cada polaridade), 2 alegações coerentes e 3 casos sem evidência correspondente. Não chama provedores externos, não usa credenciais e não estima estatisticamente a frequência com que um LLM real produz afirmações falsas.

- **Detecção de contradições:** contradições sinalizadas / 2 contradições inseridas.
- **Falso alerta:** alegações coerentes sinalizadas / 2 alegações coerentes.
- **Preservação de desconhecido:** casos sem evidência mantidos como `SemEvidenciaNoLedger` / 3 casos sem evidência.

## Medição da implementação anterior

Na `main` `ed9019d1515e9472c317829486ec4c97fc757217`, antes da correção bidirecional, o mesmo corpus mediu:

- Contradições detectadas: **1/2 (50%)**; uma alegação de falha diante de sucesso do ledger era aceita como `Sustentada`.
- Alegações coerentes sem falso alerta: **2/2 (100%)**.
- Casos sem evidência preservados como desconhecidos: **3/3 (100%)**.

## Linha de base do CI após a correção

O teste fixa os seguintes limites, que devem permanecer sem regressão:

- Contradições detectadas: **2/2 (100%)**; taxa de contradição não detectada: **0/2 (0%)**.
- Alegações coerentes sem falso alerta: **2/2 (100%)**; falsos alertas: **0/2 (0%)**.
- Casos sem evidência preservados como desconhecidos: **3/3 (100%)**.

O corpus e as classes são intencionalmente explícitos no teste: mudar o conjunto exige atualizar a versão e os números deste documento, não afrouxar as asserções para obter um CI verde.

## Limites conhecidos

Esta é uma avaliação longitudinal e determinística do **detector de divergência**, não uma medição comportamental dos modelos nem da taxa real de alegações falsas em conversas de produção. Ela demonstra que, dado um ledger conhecido, o detector não aceita as duas polaridades incompatíveis, não acusa os dois casos coerentes e não transforma ausência de registro em prova de ausência. Uma avaliação empírica de respostas de modelos reais exigiria corpus representativo, versões de provider/modelo fixadas e protocolo separado; estes resultados não a substituem.

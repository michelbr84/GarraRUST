- **O agente passa a saber a data e a hora atuais (#1725).** Nenhuma superficie
  dizia ao modelo que dia e, e ele inventava datas -- a auditoria de 2026-10-27 o
  pegou gravando "Registrado em 2025-11-14" -- e nao tinha base para "amanha",
  "esta semana" ou "ate sexta". Agora o runtime anexa ao system prompt de cada
  turno uma linha no idioma da persona com dia da semana, data, hora local e o
  deslocamento do fuso (`Data e hora atuais: terca-feira, 2026-10-27 23:55
  (UTC-04:00).`), depois do objetivo da sessao. Vale para o gateway (todos os
  canais) e para `garra chat`; o modo `neutral`, que por contrato nao tem system
  prompt, continua sem nenhum.

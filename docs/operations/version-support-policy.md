# GarraIA — Política de Versões Suportadas — PROPOSTA

> **Status: PROPOSTA de política — aguarda ratificação do maintainer**
> (issue de GA de operação). Enquanto não ratificada, vale o comportamento
> observado: releases frequentes documentados no CHANGELOG, sem suporte
> contratual a versões antigas.

## Proposta

1. **Versão suportada = a última release estável** (`vX.Y.Z` mais recente).
   Correções de segurança e bugs entram nela.
2. **Penúltima minor** recebe correções de **segurança** por até **90 dias**
   após o lançamento da minor seguinte; bugs comuns não são backportados.
3. **Qualquer versão anterior** é "melhor esforço comunitário" — sem
   backport garantido.
4. **Releases de segurança** seguem o processo do `SECURITY.md` (advisory
   privado) e podem ser publicadas como patch da versão suportada, com
   menção no CHANGELOG na categoria `security`.
5. **Versões 0.x**: enquanto 0.x, minor pode conter breaking changes; cada
   release declara breaking changes no CHANGELOG. Ao sair de 0.x, aplica-se
   semver estrito.

## Por que 90 dias

Janela curta o bastante para não segurar correções, longa o bastante para
quem auto-hospeda conseguir atualizar (produto instalado na máquina do
cliente; atualização é ação do operador).

## Evidência de que a infra suporta a política

- CHANGELOG fragmentado por categoria com gate obrigatório em CI.
- Release automatizada por tag (`release.yml`) com notas geradas do
  CHANGELOG (`scripts/release/notes.py`).
- CI com testes em Linux, Windows e macOS.

## O que muda quando ratificada

Publicar esta política em `docs/` no site/documentação, linkar no `SUPPORT.md`
e no ToS (seção de versões). Enquanto isso, esta página é a fonte interna da
decisão.

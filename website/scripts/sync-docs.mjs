#!/usr/bin/env node
// @ts-nocheck
/**
 * sync-docs.mjs — gera a arvore `website/docs/` (e os overrides em ingles em
 * `website/i18n/en/...`) a partir do markdown que ja existe no repositorio.
 *
 * Node puro, sem dependencias. A fonte unica de verdade e o repositorio: este
 * script copia um conjunto EXPLICITO de arquivos, injeta front matter, reescreve
 * links relativos (os que apontam para dentro do conjunto viram links internos;
 * o resto vira link para o GitHub) e trunca o CHANGELOG. Nunca inventa conteudo.
 *
 * Saida gitignored. Rode via `npm run sync` (o `start`/`build` ja o chamam).
 */

import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const WEBSITE = path.resolve(SCRIPT_DIR, '..');
const REPO = path.resolve(SCRIPT_DIR, '..', '..');
const DOCS_OUT = path.join(WEBSITE, 'docs');
const CONTENT_DIR = path.join(WEBSITE, 'content');
const EN_OUT = path.join(
  WEBSITE,
  'i18n',
  'en',
  'docusaurus-plugin-content-docs',
  'current',
);

const BLOB = 'https://github.com/michelbr84/GarraRUST/blob/main/';
const TREE = 'https://github.com/michelbr84/GarraRUST/tree/main/';
const RAW = 'https://raw.githubusercontent.com/michelbr84/GarraRUST/main/';
const EDIT = 'https://github.com/michelbr84/GarraRUST/edit/main/';

// Nota honesta sobre traducao parcial (usada nos overrides em ingles).
const EN_NOTE =
  '> **Note:** This page is in English. Pages not yet translated are shown in Portuguese.';

// Versoes com pagina de novidades na wiki (pt-BR `Novidades-vX.Y.Z.md` E
// inglesa `Whats-New-vX.Y.Z.md`), descobertas no repo e ordenadas da mais
// nova para a mais antiga — uma release nova entra aqui sem editar nada.
function compararSemverDesc(a, b) {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pb[i] - pa[i];
  return 0;
}
const VERSIONS = fs
  .readdirSync(path.join(REPO, 'wiki'))
  .map((f) => /^Novidades-v(\d+\.\d+\.\d+)\.md$/.exec(f)?.[1])
  .filter((v) => v && fs.existsSync(path.join(REPO, 'wiki', `Whats-New-v${v}.md`)))
  .sort(compararSemverDesc);

// Listas de versoes injetadas nas paginas de releases (content/*.md) no lugar
// dos marcadores <!-- VERSOES:PT --> e <!-- VERSOES:EN -->.
function listaDeVersoes(locale) {
  return VERSIONS.map((v) => {
    const stem = `novidades-v${v.replace(/\./g, '-')}`;
    const rotulo = locale === 'en' ? `What's New v${v}` : `Novidades v${v}`;
    return `- [${rotulo}](/releases/${stem})`;
  }).join('\n');
}

// --- Secoes (viram _category_.json). dir relativo a docs/. ---
const SECTIONS = [
  {dir: 'instalacao', label: 'Instalação', position: 2},
  {dir: 'cli', label: 'CLI e comandos', position: 3},
  {dir: 'gateway', label: 'Gateway', position: 4},
  {dir: 'mcp', label: 'MCP', position: 5},
  {dir: 'whatsapp', label: 'WhatsApp', position: 6},
  {dir: 'desktop', label: 'Desktop', position: 7},
  {dir: 'mobile', label: 'Mobile', position: 8},
  {dir: 'arquitetura', label: 'Arquitetura', position: 9},
  {dir: 'arquitetura/adr', label: 'ADRs', position: 99},
  {dir: 'seguranca', label: 'Segurança', position: 10},
  {dir: 'configuracao', label: 'Configuração', position: 11},
  {dir: 'api', label: 'API e integrações', position: 13},
  {dir: 'guias', label: 'Guias para desenvolvedores', position: 14},
  {dir: 'releases', label: 'Releases', position: 16},
  {dir: 'roadmap', label: 'Roadmap', position: 17},
  {dir: 'contribuicao', label: 'Contribuição', position: 18},
];

// --- Documentos vindos do repositorio (locale default pt-BR). ---
const DOCS = [
  // Instalacao
  {src: 'docs/installation.md', out: 'instalacao/installation', pos: 1},
  {src: 'docs/src/getting-started.md', out: 'instalacao/getting-started', pos: 2},
  // CLI
  {src: 'docs/cli-ask.md', out: 'cli/cli-ask', pos: 1},
  {src: 'docs/cli-mcp-server.md', out: 'cli/cli-mcp-server', pos: 2},
  {src: 'wiki/Referencia-da-CLI.md', out: 'cli/referencia-da-cli', pos: 3},
  // Gateway
  {src: 'docs/gateway-mcp-http.md', out: 'gateway/gateway-mcp-http', pos: 1},
  {src: 'docs/hardening-gateway.md', out: 'gateway/hardening-gateway', pos: 2},
  {src: 'docs/deployment.md', out: 'gateway/deployment', pos: 3},
  {src: 'docs/deployment-runpod.md', out: 'gateway/deployment-runpod', pos: 4},
  {src: 'docs/execution-profiles.md', out: 'gateway/execution-profiles', pos: 5},
  // MCP
  {src: 'docs/mcp.md', out: 'mcp/mcp', pos: 1},
  {src: 'docs/src/mcp.md', out: 'mcp/mcp-guia', pos: 2, label: 'MCP (guia do livro)'},
  // WhatsApp
  {src: 'docs/whatsapp.md', out: 'whatsapp/whatsapp', pos: 1},
  // Desktop
  {src: 'crates/garraia-desktop/README.md', out: 'desktop/desktop', pos: 1},
  // Mobile
  {src: 'docs/mobile/architecture.md', out: 'mobile/architecture', pos: 1},
  {src: 'apps/garraia-mobile/README.md', out: 'mobile/readme', pos: 2, label: 'Garra Mobile (README)'},
  // Arquitetura
  {src: 'docs/architecture.md', out: 'arquitetura/architecture', pos: 1},
  {src: 'docs/adr/README.md', out: 'arquitetura/adr/overview', pos: 0, label: 'Índice dos ADRs'},
  // Seguranca
  {src: 'docs/security.md', out: 'seguranca/security', pos: 1},
  {src: 'SECURITY.md', out: 'seguranca/security-policy', pos: 2, label: 'Security Policy'},
  {src: 'docs/auth-config.md', out: 'seguranca/auth-config', pos: 3},
  {src: 'docs/security/README.md', out: 'seguranca/overview', pos: 4, label: 'Visão geral de segurança'},
  {src: 'docs/security/threat-model.md', out: 'seguranca/threat-model', pos: 5},
  {src: 'docs/security/hardening-decisions.md', out: 'seguranca/hardening-decisions', pos: 6},
  {src: 'docs/security/secret-scanning-runbook.md', out: 'seguranca/secret-scanning-runbook', pos: 7},
  {src: 'docs/security/codeql-setup.md', out: 'seguranca/codeql-setup', pos: 8},
  {src: 'docs/security/codeql-suppressions.md', out: 'seguranca/codeql-suppressions', pos: 9},
  {src: 'docs/security/dependabot-status.md', out: 'seguranca/dependabot-status', pos: 10},
  {src: 'docs/security/protect-main-ruleset.md', out: 'seguranca/protect-main-ruleset', pos: 11},
  {src: 'docs/compliance/README.md', out: 'seguranca/compliance-overview', pos: 12, label: 'Conformidade (visão geral)'},
  {src: 'docs/compliance/dpia.md', out: 'seguranca/dpia', pos: 13},
  {src: 'docs/compliance/lia.md', out: 'seguranca/lia', pos: 14},
  {src: 'docs/compliance/incident-response.md', out: 'seguranca/incident-response', pos: 15},
  // Configuracao
  {src: 'docs/configuration.md', out: 'configuracao/configuration', pos: 1},
  {src: 'wiki/Configuracao.md', out: 'configuracao/wiki-configuracao', pos: 2, label: 'Configuração (wiki)'},
  // API e integracoes
  {src: 'docs/src/api-reference.md', out: 'api/api-reference', pos: 1},
  {src: 'docs/channels.md', out: 'api/channels', pos: 2},
  {src: 'docs/integrations/hermes-mcp.md', out: 'api/hermes-mcp', pos: 3},
  {src: 'docs/integrations/mcp-capacidades.md', out: 'api/mcp-capacidades', pos: 4},
  {src: 'docs/integrations/ollama-launch.md', out: 'api/ollama-launch', pos: 5},
  {src: 'docs/hermes-integration.md', out: 'api/hermes-integration', pos: 6},
  {src: 'docs/vscode/setup.md', out: 'api/vscode-setup', pos: 7, label: 'VS Code / Continue'},
  {src: 'docs/hardware.md', out: 'api/hardware', pos: 8},
  {src: 'docs/hardware-skills.md', out: 'api/hardware-skills', pos: 9},
  {src: 'docs/voice.md', out: 'api/voice', pos: 10},
  {src: 'docs/memory.md', out: 'api/memory', pos: 11},
  {src: 'docs/telemetry.md', out: 'api/telemetry', pos: 12},
  // Guias
  {src: 'docs/src/guides/add-lm-studio.md', out: 'guias/add-lm-studio', pos: 1},
  {src: 'docs/src/guides/connect-telegram.md', out: 'guias/connect-telegram', pos: 2},
  {src: 'docs/src/guides/create-plugin.md', out: 'guias/create-plugin', pos: 3},
  {src: 'docs/src/guides/plugin-sdk.md', out: 'guias/plugin-sdk', pos: 4},
  // Changelog (top-level, truncado)
  {src: 'CHANGELOG.md', out: 'changelog', pos: 15, slug: '/changelog', truncate: 8},
  // Releases
  {src: 'docs/releasing.md', out: 'releases/releasing', pos: 2, label: 'Processo de release'},
  // Roadmap
  {src: 'ROADMAP.md', out: 'roadmap/roadmap', pos: 1},
  {src: 'docs/roadmap.md', out: 'roadmap/roadmap-docs', pos: 2, label: 'Roadmap (docs)'},
  // Contribuicao
  {src: 'CONTRIBUTING.md', out: 'contribuicao/contributing', pos: 1},
  {src: 'docs/contributing.md', out: 'contribuicao/contributing-docs', pos: 2, label: 'Contribuição (docs)'},
  {src: 'CODE_OF_CONDUCT.md', out: 'contribuicao/code-of-conduct', pos: 3},
  {src: 'SUPPORT.md', out: 'contribuicao/support', pos: 4},
];

// ADRs numerados 0001..0025 -> arquitetura/adr/<stem>
for (const file of fs.readdirSync(path.join(REPO, 'docs', 'adr'))) {
  const m = file.match(/^(\d{4})-.*\.md$/);
  if (!m) continue;
  DOCS.push({
    src: `docs/adr/${file}`,
    out: `arquitetura/adr/${file.replace(/\.md$/, '')}`,
    pos: Number(m[1]),
  });
}

// Novidades (pt-BR) sob releases/.
VERSIONS.forEach((v, i) => {
  const stem = `novidades-v${v.replace(/\./g, '-')}`;
  DOCS.push({
    src: `wiki/Novidades-v${v}.md`,
    out: `releases/${stem}`,
    pos: 10 + i,
    label: `Novidades v${v}`,
  });
});

// --- Paginas escritas a mao pelo site (website/content, versionadas). ---
const CONTENT = [
  {src: 'intro.md', out: 'intro', slug: '/', pos: 1, title: 'Introdução'},
  {src: 'troubleshooting.md', out: 'troubleshooting', slug: '/troubleshooting', pos: 12, title: 'Troubleshooting'},
  {src: 'releases.md', out: 'releases/releases', slug: '/releases', pos: 1, title: 'Releases'},
];

// --- Overrides em ingles (i18n/en). O `out` casa com o doc pt-BR. ---
const EN_DOCS = [
  {kind: 'repo', src: 'README.md', out: 'intro', slug: '/', pos: 1, title: 'Introduction', note: true},
  {kind: 'content', src: 'en/instalacao.md', out: 'instalacao/installation', pos: 1, title: 'Installation', note: true},
  {kind: 'content', src: 'en/releases.md', out: 'releases/releases', slug: '/releases', pos: 1, title: 'Releases', note: true},
];
VERSIONS.forEach((v, i) => {
  const stem = `novidades-v${v.replace(/\./g, '-')}`;
  EN_DOCS.push({
    kind: 'repo',
    src: `wiki/Whats-New-v${v}.md`,
    out: `releases/${stem}`,
    pos: 10 + i,
    title: `What's New v${v}`,
  });
});

// --- Mapa repo-path(sem ext) -> outPath, para reescrever links internos. ---
const OUT_MAP = new Map();
for (const d of DOCS) {
  OUT_MAP.set(stripExt(d.src), d.out);
}

const skipped = [];

function stripExt(p) {
  return p.replace(/\.(md|markdown)$/i, '');
}

// Rota Docusaurus de um outPath. Docusaurus colapsa docs de indice de categoria
// (arquivo `index`/`README` ou cujo nome == a pasta) na rota da pasta:
// `mcp/mcp` -> `/mcp`, `mobile/readme` -> `/mobile`.
function outToRoute(out) {
  const parts = out.split('/');
  const last = parts[parts.length - 1].toLowerCase();
  const parent = parts.length > 1 ? parts[parts.length - 2].toLowerCase() : null;
  if (parts.length > 1 && (last === 'index' || last === 'readme' || last === parent)) {
    return '/' + parts.slice(0, -1).join('/');
  }
  return '/' + out;
}

function esc(s) {
  return String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

function extractTitle(body) {
  for (const line of body.split('\n')) {
    const m = line.match(/^#\s+(.+?)\s*#*\s*$/);
    if (m) return m[1].replace(/`/g, '').trim();
    if (line.trim() && !line.startsWith('<!--')) break; // H1 nao e a primeira coisa
  }
  return null;
}

function stripFrontMatter(body) {
  if (body.startsWith('---\n')) {
    const end = body.indexOf('\n---', 4);
    if (end !== -1) {
      const after = body.indexOf('\n', end + 1);
      return body.slice(after + 1);
    }
  }
  return body;
}

function truncateChangelog(body, keep) {
  const lines = body.split('\n');
  const heads = [];
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+\[/.test(lines[i])) heads.push(i);
  }
  // heads[0] costuma ser [Unreleased]; manter Unreleased + `keep` versoes.
  const firstIsUnreleased = /\[Unreleased\]/i.test(lines[heads[0]] || '');
  const cutHeadIdx = (firstIsUnreleased ? 1 : 0) + keep;
  if (heads.length <= cutHeadIdx) return body;
  const cut = heads[cutHeadIdx];
  const head = lines.slice(0, cut).join('\n').trimEnd();
  return (
    head +
    '\n\n---\n\n> As versoes anteriores a essa ficam no ' +
    `[CHANGELOG.md completo no GitHub](${BLOB}CHANGELOG.md).\n`
  );
}

// Reescreve um unico alvo de link markdown (nao-imagem).
// `localeAbsolute`: nos overrides em ingles, um link `.md` relativo nao resolve
// para um doc ainda nao traduzido; usamos rota absoluta (`/<out>`), que o
// Docusaurus prefixa com o baseUrl e valida como rota (sem depender do arquivo).
function rewriteLinkTarget(target, srcRepoPath, currentOut, localeAbsolute) {
  if (!target || target.startsWith('#')) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) return null;

  let anchor = '';
  const hash = target.indexOf('#');
  let pathPart = target;
  if (hash !== -1) {
    anchor = target.slice(hash);
    pathPart = target.slice(0, hash);
  }
  if (!pathPart) return null;

  const repoPath = pathPart.startsWith('/')
    ? pathPart.slice(1)
    : path.posix.normalize(path.posix.join(path.posix.dirname(srcRepoPath), pathPart));
  const noExt = stripExt(repoPath);

  if (OUT_MAP.has(noExt)) {
    const targetOut = OUT_MAP.get(noExt);
    if (localeAbsolute) return outToRoute(targetOut) + anchor;
    let rel = path.posix.relative(path.posix.dirname(currentOut), targetOut);
    if (!rel.startsWith('.')) rel = './' + rel;
    return rel + '.md' + anchor;
  }
  const lastSeg = repoPath.split('/').pop() || '';
  const base = lastSeg.includes('.') ? BLOB : TREE;
  return base + repoPath + anchor;
}

function rewriteImageTarget(target, srcRepoPath) {
  if (!target || target.startsWith('#')) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) return null;
  const repoPath = target.startsWith('/')
    ? target.slice(1)
    : path.posix.normalize(path.posix.join(path.posix.dirname(srcRepoPath), target));
  return RAW + repoPath;
}

// Reescreve links/imagens num bloco de texto (fora de codigo). Opera sobre o
// segmento inteiro, nao linha a linha, para pegar links markdown que o autor
// quebrou em duas linhas: `[texto\n  cont](url)`.
function rewriteText(text, srcRepoPath, currentOut, localeAbsolute) {
  // Markdown: ![alt](src) e [text](url "title") — `[^\]]`/`[^)]` incluem \n.
  text = text.replace(
    /(!?)\[([^\]]*)\]\(([^)]+)\)/g,
    (full, bang, label, inside) => {
      const mm = inside.trim().match(/^(\S+)(\s+["'][^"']*["'])?$/);
      if (!mm) return full;
      const url = mm[1];
      const title = mm[2] || '';
      const rewritten = bang
        ? rewriteImageTarget(url, srcRepoPath)
        : rewriteLinkTarget(url, srcRepoPath, currentOut, localeAbsolute);
      if (rewritten == null) return full;
      return `${bang}[${label}](${rewritten}${title})`;
    },
  );
  // HTML <img src="...">
  text = text.replace(
    /(<img\b[^>]*?\ssrc=)(["'])(.*?)\2/gi,
    (full, pre, q, src) => {
      const r = rewriteImageTarget(src, srcRepoPath);
      return r == null ? full : `${pre}${q}${r}${q}`;
    },
  );
  // HTML <a href="..."> -> link externo para o GitHub (HTML nao vira link interno)
  text = text.replace(
    /(<a\b[^>]*?\shref=)(["'])(.*?)\2/gi,
    (full, pre, q, href) => {
      if (!href || href.startsWith('#')) return full;
      if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//')) return full;
      const repoPath = href.startsWith('/')
        ? href.slice(1)
        : path.posix.normalize(path.posix.join(path.posix.dirname(srcRepoPath), href));
      const lastSeg = repoPath.split('/').pop() || '';
      const base = lastSeg.includes('.') ? BLOB : TREE;
      return `${pre}${q}${base}${repoPath}${q}`;
    },
  );
  return text;
}

// Divide o corpo em segmentos de codigo (fence) e de texto; so reescreve texto.
function rewriteBody(body, srcRepoPath, currentOut, localeAbsolute) {
  const lines = body.split('\n');
  const segs = [];
  let inFence = false;
  let fence = '';
  for (const line of lines) {
    const fm = line.match(/^\s*(```+|~~~+)/);
    let marker = false;
    if (fm) {
      const ch = fm[1][0];
      if (!inFence) {
        inFence = true;
        fence = ch;
        marker = true;
      } else if (ch === fence) {
        inFence = false;
        fence = '';
        marker = true;
      }
    }
    const code = inFence || marker;
    if (!segs.length || segs[segs.length - 1].code !== code) {
      segs.push({code, lines: []});
    }
    segs[segs.length - 1].lines.push(line);
  }
  return segs
    .map((s) =>
      s.code
        ? s.lines.join('\n')
        : rewriteText(s.lines.join('\n'), srcRepoPath, currentOut, localeAbsolute),
    )
    .join('\n');
}

function frontMatter({id, title, label, pos, slug, editUrl, description}) {
  const out = ['---'];
  out.push(`id: ${id}`);
  out.push(`title: "${esc(title)}"`);
  if (label) out.push(`sidebar_label: "${esc(label)}"`);
  if (typeof pos === 'number') out.push(`sidebar_position: ${pos}`);
  if (slug) out.push(`slug: ${slug}`);
  if (description) out.push(`description: "${esc(description)}"`);
  if (editUrl) out.push(`custom_edit_url: ${editUrl}`);
  out.push('---', '');
  return out.join('\n');
}

function writeDoc(outRoot, entry, {body, srcRepoPath, rewrite, editUrl, note, localeAbsolute}) {
  body = stripFrontMatter(body);
  body = body
    .replace('<!-- VERSOES:PT -->', listaDeVersoes('pt-BR'))
    .replace('<!-- VERSOES:EN -->', listaDeVersoes('en'));
  if (entry.truncate) body = truncateChangelog(body, entry.truncate);
  if (rewrite) body = rewriteBody(body, srcRepoPath, entry.out, localeAbsolute);
  const title = entry.title || extractTitle(body) || entry.out.split('/').pop();
  const id = entry.out.split('/').pop();
  const fm = frontMatter({
    id,
    title,
    label: entry.label,
    pos: entry.pos,
    slug: entry.slug,
    editUrl,
    description: entry.description,
  });
  const prefix = note ? EN_NOTE + '\n\n' : '';
  const dest = path.join(outRoot, entry.out + '.md');
  fs.mkdirSync(path.dirname(dest), {recursive: true});
  fs.writeFileSync(dest, fm + prefix + body.replace(/^\n+/, ''), 'utf8');
}

function readIf(abs) {
  if (!fs.existsSync(abs)) return null;
  return fs.readFileSync(abs, 'utf8');
}

function main() {
  // 1. Limpa a saida gerada.
  fs.rmSync(DOCS_OUT, {recursive: true, force: true});
  fs.rmSync(EN_OUT, {recursive: true, force: true});
  fs.mkdirSync(DOCS_OUT, {recursive: true});
  fs.mkdirSync(EN_OUT, {recursive: true});

  // 2. _category_.json por secao.
  for (const s of SECTIONS) {
    const dir = path.join(DOCS_OUT, s.dir);
    fs.mkdirSync(dir, {recursive: true});
    fs.writeFileSync(
      path.join(dir, '_category_.json'),
      JSON.stringify({label: s.label, position: s.position}, null, 2) + '\n',
      'utf8',
    );
  }

  // 3. Docs vindos do repositorio (pt-BR).
  let nDocs = 0;
  for (const entry of DOCS) {
    const abs = path.join(REPO, entry.src);
    const body = readIf(abs);
    if (body == null) {
      skipped.push(entry.src);
      continue;
    }
    writeDoc(DOCS_OUT, entry, {
      body,
      srcRepoPath: entry.src,
      rewrite: true,
      editUrl: EDIT + entry.src,
    });
    nDocs++;
  }

  // 4. Paginas escritas a mao (content/).
  let nContent = 0;
  for (const entry of CONTENT) {
    const abs = path.join(CONTENT_DIR, entry.src);
    const body = readIf(abs);
    if (body == null) {
      skipped.push('content/' + entry.src);
      continue;
    }
    writeDoc(DOCS_OUT, entry, {
      body,
      srcRepoPath: entry.src,
      rewrite: false, // links ja escritos corretos a mao
      editUrl: EDIT + 'website/content/' + entry.src,
    });
    nContent++;
  }

  // 5. Overrides em ingles (i18n/en).
  let nEn = 0;
  for (const entry of EN_DOCS) {
    const abs =
      entry.kind === 'content'
        ? path.join(CONTENT_DIR, entry.src)
        : path.join(REPO, entry.src);
    const body = readIf(abs);
    if (body == null) {
      skipped.push((entry.kind === 'content' ? 'content/' : '') + entry.src);
      continue;
    }
    writeDoc(EN_OUT, entry, {
      body,
      srcRepoPath: entry.src,
      rewrite: entry.kind === 'repo',
      localeAbsolute: true,
      editUrl:
        entry.kind === 'content'
          ? EDIT + 'website/content/' + entry.src
          : EDIT + entry.src,
      note: entry.note,
    });
    nEn++;
  }

  console.log(
    `[sync-docs] pt-BR docs: ${nDocs} | content: ${nContent} | en overrides: ${nEn}`,
  );
  if (skipped.length) {
    console.log(`[sync-docs] AVISO: ${skipped.length} fonte(s) ausente(s):`);
    for (const s of skipped) console.log(`  - ${s}`);
  }
}

main();

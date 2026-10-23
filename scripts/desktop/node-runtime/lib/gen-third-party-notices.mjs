// gen-third-party-notices.mjs — fatia B da issue #1636
//
// Gera THIRD_PARTY_NOTICES.txt com a licenca do Node.js e as licencas de TODOS
// os pacotes presentes em node_modules (incluindo aninhados e com escopo).
// FALHA (exit 1) se algum pacote nao tiver licenca identificavel — isto e, nem
// campo `license`/`licenses` no package.json, nem arquivo LICENSE/COPYING.
//
// So usa builtins do Node (prefixo `node:` para impedir sequestro de modulo por
// um arquivo plantado em node_modules). Nenhuma dependencia nova.

import { readFileSync, readdirSync, existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    const k = argv[i];
    if (!k || !k.startsWith('--')) throw new Error(`argumento invalido: ${k}`);
    out[k.slice(2)] = argv[i + 1];
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
for (const req of ['node-modules', 'node-license', 'node-version', 'out']) {
  if (!args[req]) {
    console.error(`ERRO: faltou --${req}`);
    process.exit(2);
  }
}

const LICENSE_FILE_RE = /^(licen[cs]e|copying|copyright|notice|unlicense)(\.|$)/i;

function licenseIdFromPkg(pkg) {
  if (typeof pkg.license === 'string' && pkg.license.trim()) return pkg.license.trim();
  if (pkg.license && typeof pkg.license === 'object' && pkg.license.type) return String(pkg.license.type);
  if (Array.isArray(pkg.licenses)) {
    const ids = pkg.licenses.map((l) => (typeof l === 'string' ? l : l && l.type)).filter(Boolean);
    if (ids.length) return ids.join(' OR ');
  }
  return null;
}

function findLicenseFile(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return null;
  }
  for (const e of entries) {
    if (e.isFile() && LICENSE_FILE_RE.test(e.name)) return join(dir, e.name);
  }
  return null;
}

const packages = new Map(); // "name@version" -> record

function handlePackage(dir) {
  const pjPath = join(dir, 'package.json');
  if (existsSync(pjPath)) {
    let pkg;
    try {
      pkg = JSON.parse(readFileSync(pjPath, 'utf8'));
    } catch {
      pkg = null;
    }
    if (pkg && pkg.name) {
      const key = `${pkg.name}@${pkg.version || '0.0.0'}`;
      if (!packages.has(key)) {
        const licenseId = licenseIdFromPkg(pkg);
        const licenseFile = findLicenseFile(dir);
        packages.set(key, {
          name: pkg.name,
          version: pkg.version || '0.0.0',
          licenseId,
          licenseFile,
        });
      }
    }
  }
  const nested = join(dir, 'node_modules');
  if (existsSync(nested)) collect(nested);
}

function collect(nmDir) {
  let entries;
  try {
    entries = readdirSync(nmDir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    if (e.name.startsWith('.')) continue; // .bin, .package-lock.json, .cache
    if (!e.isDirectory()) continue;
    const full = join(nmDir, e.name);
    if (e.name.startsWith('@')) {
      // pacote com escopo: desce mais um nivel
      let subs;
      try {
        subs = readdirSync(full, { withFileTypes: true });
      } catch {
        subs = [];
      }
      for (const s of subs) {
        if (s.name.startsWith('.') || !s.isDirectory()) continue;
        handlePackage(join(full, s.name));
      }
    } else {
      handlePackage(full);
    }
  }
}

const nmRoot = args['node-modules'];
if (!existsSync(nmRoot)) {
  console.error(`ERRO: node_modules nao existe: ${nmRoot}`);
  process.exit(1);
}
collect(nmRoot);

if (packages.size === 0) {
  console.error('ERRO: nenhum pacote encontrado em node_modules');
  process.exit(1);
}

const missing = [];
for (const rec of packages.values()) {
  if (!rec.licenseId && !rec.licenseFile) missing.push(`${rec.name}@${rec.version}`);
}
if (missing.length) {
  console.error('ERRO: pacotes sem licenca identificavel (sem campo license e sem arquivo LICENSE):');
  for (const m of missing) console.error(`  - ${m}`);
  process.exit(1);
}

const sorted = [...packages.values()].sort((a, b) =>
  a.name < b.name ? -1 : a.name > b.name ? 1 : a.version < b.version ? -1 : 1,
);

const SEP = '='.repeat(78);
const nodeLicenseText = readFileSync(args['node-license'], 'utf8');
const nowUtc = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

const chunks = [];
chunks.push('GarraIA Desktop — runtime Node.js privado e ponte do WhatsApp');
chunks.push('THIRD-PARTY SOFTWARE NOTICES AND LICENSES');
chunks.push('');
chunks.push(`Gerado em: ${nowUtc} (UTC)`);
chunks.push(`Node.js: v${args['node-version']}`);
chunks.push(`Pacotes npm em node_modules: ${sorted.length}`);
chunks.push('');
chunks.push('Este arquivo e gerado automaticamente por scripts/desktop/node-runtime/.');
chunks.push('');
chunks.push(SEP);
chunks.push(`Node.js  (v${args['node-version']})`);
chunks.push('Fonte: https://nodejs.org/  — artefato oficial verificado por GPG + SHA-256');
chunks.push(SEP);
chunks.push('');
chunks.push(nodeLicenseText.trimEnd());
chunks.push('');

for (const rec of sorted) {
  chunks.push(SEP);
  chunks.push(`${rec.name}  (${rec.version})`);
  chunks.push(`License: ${rec.licenseId || '(ver texto abaixo)'}`);
  chunks.push(SEP);
  chunks.push('');
  if (rec.licenseFile) {
    chunks.push(readFileSync(rec.licenseFile, 'utf8').trimEnd());
  } else {
    chunks.push(`Licenca declarada no package.json: ${rec.licenseId}`);
    chunks.push('(nenhum arquivo LICENSE/COPYING empacotado por este pacote)');
  }
  chunks.push('');
}

writeFileSync(args['out'], chunks.join('\n') + '\n', 'utf8');
console.log(`THIRD_PARTY_NOTICES.txt: ${sorted.length} pacotes npm + Node.js -> ${args['out']}`);

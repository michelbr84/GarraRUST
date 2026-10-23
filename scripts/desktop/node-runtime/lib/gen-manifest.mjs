// gen-manifest.mjs — fatia B da issue #1636
//
// Grava manifest.json descrevendo o bundle: versao do Node, plataforma, sha256
// do artefato oficial, fingerprint da chave que assinou, e o estado do lock da
// ponte (versao, lockfileVersion, versao do Baileys e sha256 do
// package-lock.json). So usa builtins do Node (prefixo `node:`).

import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

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
for (const req of [
  'out', 'platform', 'node-version', 'node-archive',
  'node-archive-sha256', 'signer-fingerprint', 'bridge-lock',
]) {
  if (!args[req]) {
    console.error(`ERRO: faltou --${req}`);
    process.exit(2);
  }
}

const lockRaw = readFileSync(args['bridge-lock'], 'utf8');
const lock = JSON.parse(lockRaw);
const lockSha = createHash('sha256').update(lockRaw, 'utf8').digest('hex');

// versao do Baileys declarada no lock (o pino de versao exata da ponte)
let baileysVersion = null;
const bailKey = 'node_modules/@whiskeysockets/baileys';
if (lock.packages && lock.packages[bailKey] && lock.packages[bailKey].version) {
  baileysVersion = lock.packages[bailKey].version;
} else if (lock.dependencies && lock.dependencies['@whiskeysockets/baileys']) {
  baileysVersion = lock.dependencies['@whiskeysockets/baileys'].version;
}

const nowUtc = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

const manifest = {
  schema: 'garraia.node-runtime-bundle/1',
  generated_at: nowUtc,
  platform: args['platform'],
  node: {
    version: args['node-version'],
    archive: args['node-archive'],
    archive_sha256: args['node-archive-sha256'],
    source: `https://nodejs.org/dist/v${args['node-version']}/`,
    signer_fingerprint: args['signer-fingerprint'],
  },
  bridge: {
    name: lock.name || null,
    package_version: lock.version || null,
    lockfile_version: typeof lock.lockfileVersion === 'number' ? lock.lockfileVersion : null,
    package_lock_sha256: lockSha,
    baileys_version: baileysVersion,
  },
};

writeFileSync(args['out'], JSON.stringify(manifest, null, 2) + '\n', 'utf8');
console.log(`manifest.json -> ${args['out']}`);

// Invariantes que so um teste varrendo o fonte consegue segurar: e facil
// alguem "so debugar rapidinho" com um console.log e envenenar o NDJSON, ou
// gravar um creds.json e desfazer a razao de a ponte ser stateless.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { BRIDGE_DIR } from './helpers.mjs';

/** Comentario citando uma API proibida nao e uso dela: varremos so o codigo. */
const stripComments = (source) =>
  source.replace(/^\s*\/\/.*$/gm, '').replace(/\/\*[\s\S]*?\*\//g, '');

const sources = ['bridge.mjs', 'scripts/protocol-lint.mjs'].map((rel) => [
  rel,
  stripComments(fs.readFileSync(path.join(BRIDGE_DIR, rel), 'utf8')),
]);

test('nenhum console.* — stdout e exclusivamente NDJSON', () => {
  for (const [name, source] of sources) {
    assert.equal(/\bconsole\.(log|info|warn|error|debug)\b/.test(source), false, name);
  }
});

test('a ponte nunca escreve no disco', () => {
  const forbidden = /\b(writeFile|writeFileSync|appendFile|appendFileSync|createWriteStream|mkdir|mkdirSync|unlink|useMultiFileAuthState)\b/;
  const [, bridge] = sources.find(([name]) => name === 'bridge.mjs');
  assert.equal(forbidden.test(bridge), false, 'bridge.mjs nao pode ter API de escrita em disco');
});

test('quem desenha o QR e o Rust: nada de qrcode-terminal', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(BRIDGE_DIR, 'package.json'), 'utf8'));
  const deps = Object.keys(pkg.dependencies ?? {});
  assert.equal(deps.some((d) => d.includes('qrcode')), false);
  const [, bridge] = sources.find(([name]) => name === 'bridge.mjs');
  assert.match(bridge, /printQRInTerminal: false/);
});

test('dependencias pinadas exatas — sem ^ nem ~', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(BRIDGE_DIR, 'package.json'), 'utf8'));
  for (const [name, range] of Object.entries(pkg.dependencies ?? {})) {
    assert.match(range, /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/, `${name} deve estar pinada em versao exata`);
  }
  assert.equal(pkg.type, 'module');
  assert.equal(pkg.private, true);
});

test('o package-lock esta versionado e casa com o package.json', () => {
  const lockPath = path.join(BRIDGE_DIR, 'package-lock.json');
  assert.ok(fs.existsSync(lockPath), 'package-lock.json precisa estar no repo');
  const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
  const pkg = JSON.parse(fs.readFileSync(path.join(BRIDGE_DIR, 'package.json'), 'utf8'));
  for (const [name, range] of Object.entries(pkg.dependencies)) {
    assert.equal(lock.packages[`node_modules/${name}`].version, range, name);
  }
});

// Estado de autenticacao em memoria: precisa ser intercambiavel com o
// useMultiFileAuthState oficial, menos o disco.
import test from 'node:test';
import assert from 'node:assert/strict';
import { makeMemoryAuthState, encodeSnapshot, decodeSnapshot, loadBaileys } from '../bridge.mjs';

test('sem snapshot: gera credenciais novas e um store de keys vazio', async () => {
  const auth = await makeMemoryAuthState(null);
  const snap = auth.snapshot();
  assert.ok(snap.creds.noiseKey, 'initAuthCreds deve ter rodado');
  assert.deepEqual(snap.keys, {});
  assert.equal(snap.creds.registered, false);
});

test('round-trip: snapshot -> base64 -> restore preserva bytes', async () => {
  const auth = await makeMemoryAuthState(null);
  await auth.state.keys.set({
    'pre-key': { 1: { public: Buffer.from([1, 2, 3]), private: Buffer.from([4, 5, 6]) } },
  });

  const encoded = await encodeSnapshot(auth.snapshot());
  const restored = await decodeSnapshot(encoded);
  const revived = await makeMemoryAuthState(restored);

  assert.equal(await encodeSnapshot(revived.snapshot()), encoded, 'snapshot deve ser estavel');
  assert.deepEqual(revived.snapshot().creds.noiseKey, auth.snapshot().creds.noiseKey);

  const got = await revived.state.keys.get('pre-key', ['1']);
  assert.ok(Buffer.isBuffer(got['1'].public), 'BufferJSON.reviver deve devolver Buffer');
  assert.deepEqual(got['1'].public, Buffer.from([1, 2, 3]));
});

test('app-state-sync-key volta como mensagem protobuf, nao objeto plano', async () => {
  const { proto } = await loadBaileys();
  const auth = await makeMemoryAuthState(null);
  await auth.state.keys.set({
    'app-state-sync-key': {
      AAAAAA: { keyData: Buffer.alloc(32, 7), fingerprint: { rawId: 1 }, timestamp: 1 },
    },
  });
  const restored = await makeMemoryAuthState(await decodeSnapshot(await encodeSnapshot(auth.snapshot())));
  const got = await restored.state.keys.get('app-state-sync-key', ['AAAAAA']);
  assert.ok(got.AAAAAA instanceof proto.Message.AppStateSyncKeyData);
  assert.deepEqual(Buffer.from(got.AAAAAA.keyData), Buffer.alloc(32, 7));
});

test('keys.set com valor nulo apaga a entrada e limpa a categoria vazia', async () => {
  const auth = await makeMemoryAuthState(null);
  await auth.state.keys.set({ session: { a: { x: 1 }, b: { y: 2 } } });
  await auth.state.keys.set({ session: { a: null } });
  assert.deepEqual(Object.keys(auth.snapshot().keys.session), ['b']);
  await auth.state.keys.set({ session: { b: null } });
  assert.deepEqual(auth.snapshot().keys, {});
});

test('keys.get devolve undefined para id ausente (contrato do Baileys)', async () => {
  const auth = await makeMemoryAuthState(null);
  const got = await auth.state.keys.get('session', ['nao-existe']);
  assert.deepEqual(Object.keys(got), ['nao-existe']);
  assert.equal(got['nao-existe'], undefined);
});

test('toda mudanca em keys notifica o onChange', async () => {
  let changes = 0;
  const auth = await makeMemoryAuthState(null, () => {
    changes += 1;
  });
  await auth.state.keys.set({ session: { a: { x: 1 } } });
  await auth.state.keys.set({ session: { a: null } });
  assert.equal(changes, 2);
});

test('decodeSnapshot: null passa, lixo explode', async () => {
  assert.equal(await decodeSnapshot(null), null);
  await assert.rejects(() => decodeSnapshot(Buffer.from('[]').toString('base64')));
  await assert.rejects(() => decodeSnapshot('nao-e-base64-de-json'));
});

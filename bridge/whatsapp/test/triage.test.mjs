// #1813/#1814/#1815: o que cada item de `messages.upsert` vira — `message`,
// `dropped` ou nada. Nenhuma rede: so a funcao pura.
import test from 'node:test';
import assert from 'node:assert/strict';
import { triageUpsert, isSelfChat, normalizeUserJid, OFFLINE_REPLY_WINDOW_SECS } from '../bridge.mjs';

const OWN = '5511999990000:1@s.whatsapp.net';
const OWN_LID = '123456789012345:1@lid';
const SELF = '5511999990000@s.whatsapp.net';
const PEER = '5511888880000@s.whatsapp.net';
const NOW = 1_800_000_000;

const msg = (key, message = { conversation: 'oi' }, extra = {}) => ({
  key: { id: 'M1', fromMe: false, ...key },
  message,
  messageTimestamp: NOW - 5,
  ...extra,
});
const opts = (extra = {}) => ({ ownJid: OWN, ownLid: OWN_LID, nowSecs: NOW, sentIds: new Set(), ...extra });

test('normalizeUserJid tira o aparelho e mantem o dominio', () => {
  assert.equal(normalizeUserJid(OWN), SELF);
  assert.equal(normalizeUserJid(OWN_LID), '123456789012345@lid');
  assert.equal(normalizeUserJid('sem-arroba'), null);
  assert.equal(normalizeUserJid(null), null);
});

test('isSelfChat reconhece a conversa "Voce" por telefone, por LID e pelo remoteJidAlt', () => {
  assert.equal(isSelfChat({ remoteJid: SELF }, [OWN, OWN_LID]), true);
  assert.equal(isSelfChat({ remoteJid: '123456789012345@lid' }, [OWN, OWN_LID]), true);
  assert.equal(isSelfChat({ remoteJid: '999@lid', remoteJidAlt: SELF }, [OWN, null]), true);
  assert.equal(isSelfChat({ remoteJid: PEER }, [OWN, OWN_LID]), false);
  assert.equal(isSelfChat({ remoteJid: SELF }, [null, null]), false, 'sem a propria conta, nunca');
});

test('notify de terceiro vira message, sem self_chat', () => {
  const ev = triageUpsert(msg({ remoteJid: PEER }), 'notify', opts());
  assert.equal(ev.type, 'message');
  assert.equal(ev.self_chat, false);
  assert.equal(ev.from_me, false);
});

test('conversa "Voce": from_me no proprio chat vira message com self_chat', () => {
  const ev = triageUpsert(msg({ remoteJid: SELF, fromMe: true }), 'notify', opts());
  assert.equal(ev.type, 'message');
  assert.equal(ev.from_me, true);
  assert.equal(ev.self_chat, true);
  assert.equal(ev.sender_phone, '+5511999990000');
});

test('o dono escrevendo a um terceiro continua from_me sem self_chat', () => {
  const ev = triageUpsert(msg({ remoteJid: PEER, fromMe: true }), 'notify', opts());
  assert.equal(ev.from_me, true);
  assert.equal(ev.self_chat, false);
});

test('anti-eco: o que a propria ponte enviou nunca volta como entrada', () => {
  const sentIds = new Set(['M1']);
  assert.equal(triageUpsert(msg({ remoteJid: SELF, fromMe: true }), 'notify', opts({ sentIds })), null);
  assert.equal(triageUpsert(msg({ remoteJid: SELF, fromMe: true }), 'append', opts({ sentIds })), null);
});

test('append da propria conta (eco local de um envio) some sem evento', () => {
  assert.equal(triageUpsert(msg({ remoteJid: SELF, fromMe: true }), 'append', opts()), null);
  assert.equal(triageUpsert(msg({ remoteJid: PEER, fromMe: true }), 'append', opts()), null);
});

test('append recente de terceiro (chegou durante o restart) vira message', () => {
  const ev = triageUpsert(msg({ remoteJid: PEER }, { conversation: 'oi' }, { messageTimestamp: NOW - 120 }), 'append', opts());
  assert.equal(ev.type, 'message');
  assert.equal(ev.text, 'oi');
});

test('append antigo de terceiro vira dropped offline_expired, sem texto', () => {
  const velho = NOW - OFFLINE_REPLY_WINDOW_SECS - 1;
  const ev = triageUpsert(msg({ remoteJid: PEER }, { conversation: 'segredo' }, { messageTimestamp: velho }), 'append', opts());
  assert.deepEqual(ev, { type: 'dropped', reason: 'offline_expired', chat_last4: '0000', is_group: false, from_me: false });
});

test('timestamp Long do protobuf tambem conta a idade', () => {
  const longTs = { toNumber: () => NOW - 30 };
  assert.equal(triageUpsert(msg({ remoteJid: PEER }, undefined, { messageTimestamp: longTs }), 'append', opts()).type, 'message');
});

test('falha de decriptacao (stub CIPHERTEXT) vira dropped decrypt_failed', () => {
  const stub = { key: { id: 'X', remoteJid: PEER, fromMe: false }, messageStubType: 2, messageStubParameters: ['Bad MAC'], messageTimestamp: NOW };
  const ev = triageUpsert(stub, 'notify', opts());
  assert.equal(ev.type, 'dropped');
  assert.equal(ev.reason, 'decrypt_failed');
  assert.ok(!JSON.stringify(ev).includes('Bad MAC'), 'o erro do Baileys nao vaza');
});

test('conteudo sem texto e sem midia: enquete avisa, reacao e sincronizacao somem', () => {
  assert.equal(triageUpsert(msg({ remoteJid: PEER }, { pollCreationMessage: { name: 'x' } }), 'notify', opts()).reason, 'unsupported_content');
  assert.equal(triageUpsert(msg({ remoteJid: PEER }, { reactionMessage: { text: '👍' } }), 'notify', opts()), null);
  assert.equal(triageUpsert(msg({ remoteJid: PEER }, { protocolMessage: { type: 0 } }), 'notify', opts()), null);
  assert.equal(triageUpsert(msg({ remoteJid: PEER, fromMe: true }, { pollCreationMessage: { name: 'x' } }), 'notify', opts()), null);
});

test('status e canal continuam fora, sem evento', () => {
  assert.equal(triageUpsert(msg({ remoteJid: 'status@broadcast' }), 'notify', opts()), null);
  assert.equal(triageUpsert(msg({ remoteJid: '1203630@newsletter' }), 'notify', opts()), null);
});

test('dropped nunca carrega numero inteiro', () => {
  const ev = triageUpsert(msg({ remoteJid: PEER }, { pollCreationMessage: {} }), 'notify', opts());
  assert.ok(!JSON.stringify(ev).includes('5511888880000'));
  assert.equal(ev.chat_last4, '0000');
});

test('tipo desconhecido de upsert nao entrega nada', () => {
  assert.equal(triageUpsert(msg({ remoteJid: PEER }), 'last', opts()), null);
});

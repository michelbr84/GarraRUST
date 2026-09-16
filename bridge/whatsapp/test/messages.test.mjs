// Traducao Baileys -> evento `message`.
import test from 'node:test';
import assert from 'node:assert/strict';
import { toMessageEvent, unwrapMessage } from '../bridge.mjs';

const OWN = '5511999990000:1@s.whatsapp.net';
const PEER = '5511888880000@s.whatsapp.net';

const base = (overrides = {}) => ({
  key: { id: 'ABC123', remoteJid: PEER, fromMe: false },
  message: { conversation: 'oi' },
  messageTimestamp: 1780000000,
  pushName: 'Fulano',
  ...overrides,
});

test('mensagem de texto direta', () => {
  assert.deepEqual(toMessageEvent(base(), OWN), {
    type: 'message',
    id: 'ABC123',
    chat_jid: PEER,
    sender_jid: PEER,
    sender_phone: '+5511888880000',
    text: 'oi',
    media_kind: null,
    timestamp: 1780000000,
    is_group: false,
    from_me: false,
    push_name: 'Fulano',
  });
});

test('extendedTextMessage tambem e texto', () => {
  const ev = toMessageEvent(base({ message: { extendedTextMessage: { text: 'com link' } } }), OWN);
  assert.equal(ev.text, 'com link');
});

test('grupo: o remetente e o participant, nao o chat', () => {
  const ev = toMessageEvent(
    base({ key: { id: 'G1', remoteJid: '120363000000000000@g.us', participant: PEER } }),
    OWN,
  );
  assert.equal(ev.is_group, true);
  assert.equal(ev.sender_jid, PEER);
  assert.equal(ev.chat_jid, '120363000000000000@g.us');
});

test('midia: text null + media_kind (v1 nao entrega legenda)', () => {
  const ev = toMessageEvent(base({ message: { imageMessage: { caption: 'olha' } } }), OWN);
  assert.equal(ev.text, null);
  assert.equal(ev.media_kind, 'image');
});

test('status@broadcast e newsletter sao descartados', () => {
  assert.equal(toMessageEvent(base({ key: { id: 'S', remoteJid: 'status@broadcast' } }), OWN), null);
  assert.equal(toMessageEvent(base({ key: { id: 'N', remoteJid: 'abc@newsletter' } }), OWN), null);
});

test('mensagem sem texto nem midia (reacao, protocolo) nao vira evento', () => {
  assert.equal(toMessageEvent(base({ message: { protocolMessage: {} } }), OWN), null);
  assert.equal(toMessageEvent(base({ message: null }), OWN), null);
  assert.equal(toMessageEvent({ key: { id: 'x' } }, OWN), null);
});

test('remetente @lid entrega sender_phone null, sem inventar mapeamento', () => {
  const ev = toMessageEvent(
    base({ key: { id: 'L', remoteJid: '120363000000000000@g.us', participant: '87654321098765@lid' } }),
    OWN,
  );
  assert.equal(ev.sender_jid, '87654321098765@lid');
  assert.equal(ev.sender_phone, null);
});

test('fromMe em conversa direta atribui o proprio JID', () => {
  const ev = toMessageEvent(base({ key: { id: 'M', remoteJid: PEER, fromMe: true } }), OWN);
  assert.equal(ev.from_me, true);
  assert.equal(ev.sender_jid, OWN);
});

test('timestamp Long do protobuf vira number', () => {
  const ev = toMessageEvent(base({ messageTimestamp: { toNumber: () => 1780000042 } }), OWN);
  assert.equal(ev.timestamp, 1780000042);
});

test('unwrapMessage desce pelos containers do WhatsApp', () => {
  const inner = { conversation: 'escondido' };
  assert.deepEqual(unwrapMessage({ ephemeralMessage: { message: inner } }), inner);
  assert.deepEqual(
    unwrapMessage({ viewOnceMessageV2: { message: { ephemeralMessage: { message: inner } } } }),
    inner,
  );
  assert.deepEqual(unwrapMessage(inner), inner);
});

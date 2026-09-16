// Redacao: nenhum evento `log` pode carregar telefone inteiro nem JID cru.
import test from 'node:test';
import assert from 'node:assert/strict';
import { redact, redactJid, describeError, phoneLast4, senderPhone } from '../bridge.mjs';

test('redactJid mantem so os 4 ultimos digitos e o dominio', () => {
  assert.equal(redactJid('5511999990000@s.whatsapp.net'), '***0000@s.whatsapp.net');
  // O sufixo de device (`:1`) nao pode deslocar a janela dos 4 digitos.
  assert.equal(redactJid('5511999990000:1@s.whatsapp.net'), '***0000@s.whatsapp.net');
  assert.equal(redactJid('120363000000000000@g.us'), '***0000@g.us');
  assert.equal(redactJid('87654321098765@lid'), '***8765@lid');
  assert.equal(redactJid('sem-arroba'), '***');
  assert.equal(redactJid(undefined), '***');
});

test('redact mascara JIDs e telefones soltos dentro de texto livre', () => {
  assert.equal(
    redact('falha ao enviar para 5511999990000@s.whatsapp.net'),
    'falha ao enviar para ***0000@s.whatsapp.net',
  );
  assert.equal(redact('numero 5511988887777 bloqueado'), 'numero ***7777 bloqueado');
  assert.equal(redact('codigo 515 e retry 30'), 'codigo 515 e retry 30', 'numeros curtos ficam');
});

test('describeError nunca devolve o objeto de erro, so nome e mensagem redigida', () => {
  const boom = new Error('recipient 5511999990000@s.whatsapp.net rejected');
  boom.data = { creds: 'SEGREDO' };
  const described = describeError(boom);
  assert.equal(described, 'Error: recipient ***0000@s.whatsapp.net rejected');
  assert.ok(!described.includes('SEGREDO'));
});

test('phoneLast4 ignora o sufixo de device', () => {
  assert.equal(phoneLast4('5511999990000:1@s.whatsapp.net'), '0000');
  assert.equal(phoneLast4('5511999991234@s.whatsapp.net'), '1234');
  assert.equal(phoneLast4(null), null);
});

test('senderPhone resolve E.164 so para @s.whatsapp.net; @lid fica null', () => {
  assert.equal(senderPhone('5511999990000@s.whatsapp.net'), '+5511999990000');
  assert.equal(senderPhone('5511999990000:3@s.whatsapp.net'), '+5511999990000');
  // v1 nao mapeia LID -> telefone: o Rust recebe null e decide o que fazer.
  assert.equal(senderPhone('87654321098765@lid'), null);
  assert.equal(senderPhone('120363000000000000@g.us'), null);
});

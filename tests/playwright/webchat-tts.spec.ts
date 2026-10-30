/**
 * #1754 (PR I): o TTS da Web volta a tocar audio, interrompe a fala anterior e
 * libera a blob URL.
 *
 * Antes do fix o `playTTS` fazia `await r.json()` e lia `j.audio` em base64; no
 * sucesso o gateway devolve os bytes WAV crus (`Content-Type: audio/wav`),
 * entao o `r.json()` quebrava sobre o binario e o erro morria num `catch {}`
 * vazio — o TTS nunca tocava.
 *
 * O script do webchat vive numa IIFE, entao o spec chama `playTTS` pelo seam
 * `window.__garraTts` (congelado), exposto so para teste.
 *
 * O spec nao precisa do `--with-voice`: o `page.route` intercepta `/api/tts`
 * no browser e devolve bytes `audio/wav`. Ele troca `HTMLMediaElement.play`
 * por um stub que resolve (nenhum audio real toca no CI) e conta as chamadas a
 * `URL.revokeObjectURL` para provar a interrupcao/limpeza.
 */

import { test, expect, Page } from '@playwright/test';

/** Bytes crus que fingem um WAV; o cliente so olha o content-type. */
const WAV_BYTES = Buffer.from([0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00]); // "RIFF..."

/**
 * Abre o console com TTS ligado e instala sondas:
 *  - `HTMLMediaElement.prototype.play` vira um stub que registra `this.src` e
 *    resolve (nenhum audio real toca).
 *  - `URL.revokeObjectURL` registra cada URL revogada antes de chamar a real.
 */
async function abrirComSonda(page: Page) {
  await page.addInitScript(() => {
    window.localStorage.setItem('garraia.tts_enabled', 'true');
    // @ts-expect-error — sonda injetada so para o teste.
    window.__ttsProbe = { playCalls: [], revoked: [] };
    const playOriginal = HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      // @ts-expect-error — sonda.
      window.__ttsProbe.playCalls.push(this.src);
      return Promise.resolve();
    };
    void playOriginal;
    const revokeOriginal = URL.revokeObjectURL.bind(URL);
    URL.revokeObjectURL = function (u: string) {
      // @ts-expect-error — sonda.
      window.__ttsProbe.revoked.push(u);
      return revokeOriginal(u);
    };
  });

  await page.route('**/api/tts', (route) =>
    route.fulfill({ status: 200, contentType: 'audio/wav', body: WAV_BYTES }),
  );

  await page.goto('/');
  await page.waitForFunction(() => typeof (window as unknown as { __garraTts?: { playTTS?: unknown } }).__garraTts?.playTTS === 'function', null, {
    timeout: 10_000,
  });
  // Zera o que o boot possa ter registrado antes de medir.
  await page.evaluate(() => {
    // @ts-expect-error — sonda.
    window.__ttsProbe.playCalls = [];
    // @ts-expect-error — sonda.
    window.__ttsProbe.revoked = [];
  });
}

test.describe('#1754 — TTS da Web toca o blob e interrompe a fala anterior', () => {
  test('sucesso audio/wav toca um <audio> com src blob:', async ({ page }) => {
    await abrirComSonda(page);

    await page.evaluate(() => (window as unknown as { __garraTts: { playTTS: (t: string) => Promise<void> } }).__garraTts.playTTS('oi'));

    const playCalls = await page.evaluate(() => (window as unknown as { __ttsProbe: { playCalls: string[] } }).__ttsProbe.playCalls);
    expect(playCalls.length, 'playTTS deveria ter chamado play() uma vez').toBe(1);
    expect(playCalls[0], 'o src do audio deveria ser uma blob URL').toMatch(/^blob:/);
  });

  test('uma segunda fala interrompe a primeira e revoga a blob URL anterior', async ({ page }) => {
    await abrirComSonda(page);

    await page.evaluate(() => (window as unknown as { __garraTts: { playTTS: (t: string) => Promise<void> } }).__garraTts.playTTS('oi'));
    const primeiraUrl = (await page.evaluate(() => (window as unknown as { __ttsProbe: { playCalls: string[] } }).__ttsProbe.playCalls))[0];
    expect(primeiraUrl).toMatch(/^blob:/);

    await page.evaluate(() => (window as unknown as { __garraTts: { playTTS: (t: string) => Promise<void> } }).__garraTts.playTTS('tchau'));

    const playCalls = await page.evaluate(() => (window as unknown as { __ttsProbe: { playCalls: string[] } }).__ttsProbe.playCalls);
    expect(playCalls.length, 'as duas falas deveriam ter tocado').toBe(2);

    const revoked = await page.evaluate(() => (window as unknown as { __ttsProbe: { revoked: string[] } }).__ttsProbe.revoked);
    expect(revoked, 'a blob URL da primeira fala deveria ter sido revogada').toContain(primeiraUrl);
  });
});

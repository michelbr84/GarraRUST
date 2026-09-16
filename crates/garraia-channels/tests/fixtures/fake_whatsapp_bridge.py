#!/usr/bin/env python3
"""Ponte WhatsApp falsa: fala o protocolo NDJSON v1 sem Node, sem Baileys e
sem telefone.

Existe para que os testes Rust de `garraia-channels` exercitem o supervisor da
ponte (pareamento, expiracao de QR, logout, queda, reconexao, eco) de forma
deterministica e em milissegundos. O contrato e o mesmo de
`bridge/whatsapp/bridge.mjs`; os codigos de saida sao espelhados:

    0 = fim normal   1 = erro fatal   2 = logged_out   3 = erro de protocolo

Somente stdlib. Todo evento sai em stdout como UMA linha JSON.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import queue
import sys
import threading
import time

PROTOCOL_VERSION = 1
BRIDGE_VERSION = "fake-1.0.0"
MAX_LINE_BYTES = 256 * 1024

EXIT_OK = 0
EXIT_FATAL = 1
EXIT_LOGGED_OUT = 2
EXIT_PROTOCOL = 3

# Identidade fixa: os testes Rust dependem destes valores exatos.
OWN_JID = "5511999990000:1@s.whatsapp.net"
PEER_JID = "5511888880000@s.whatsapp.net"
PUSHNAME = "Garra Tester"
BASE_TIMESTAMP = 1780000000

SCENARIOS = (
    "pair-ok",
    "pair-expire-then-ok",
    "logged-out",
    "crash-after-qr",
    "network-flap",
    "hang",
    "serve-echo",
    # --- acrescentados pelo lado Rust (branch feat/whatsapp-linked-cli) ------
    # Os quatro abaixo nao existem no bridge real como MODO: sao formas de
    # quebrar o contrato que o supervisor Rust precisa tratar sem travar e sem
    # persistir nada. Ficam aqui, e nao num segundo arquivo, porque sao o mesmo
    # dublê e o mesmo enquadramento.
    #
    # session-ok      sessao carregada vale: conecta SEM emitir QR (e o que o
    #                 bridge real faz; `pair-ok` sempre emite QR)
    # bad-protocol    started com protocol != 1
    # garbage         texto livre no stdout (proibido: stdout e so NDJSON)
    # oversized       linha acima do teto de 256 KiB
    # connect-then-hang
    #                 conecta, entrega a sessao e emudece SEM fechar o stdout
    "session-ok",
    "bad-protocol",
    "garbage",
    "oversized",
    "connect-then-hang",
)


def emit(event: dict) -> None:
    sys.stdout.write(json.dumps(event, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def session_blob() -> str:
    """Snapshot deterministico — mesmo bytes em toda execucao."""
    payload = {"creds": {"me": {"id": OWN_JID}}, "keys": {}}
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode("utf-8")
    return base64.b64encode(raw).decode("ascii")


class Bridge:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.seq = 0
        self.qr_attempt = 0
        self.sent = 0
        self.commands: "queue.Queue[dict]" = queue.Queue()
        self.eof = threading.Event()

    # -- saida -------------------------------------------------------------
    def status(self, state: str, detail: str = "") -> None:
        emit({"type": "status", "state": state, "detail": detail})

    def qr(self) -> None:
        self.qr_attempt += 1
        emit(
            {
                "type": "qr",
                "data": f"2@FAKEQR/{self.qr_attempt}/garraia,fakekey,fakemac",
                "expires_in_secs": int(max(1, round(self.args.qr_expires))),
                "attempt": self.qr_attempt,
            }
        )
        self.status("waiting_scan", f"qr attempt {self.qr_attempt}")

    def session_update(self) -> None:
        self.seq += 1
        emit({"type": "session_update", "session": session_blob(), "seq": self.seq})

    def connected(self) -> None:
        self.status("connected")
        emit(
            {
                "type": "connected",
                "jid": OWN_JID,
                "phone_last4": OWN_JID.split("@")[0].split(":")[0][-4:],
                "pushname": PUSHNAME,
            }
        )

    def disconnected(self, code: int, reason: str, will_retry: bool, retry_in_ms: int) -> None:
        emit(
            {
                "type": "disconnected",
                "reason_code": code,
                "reason": reason,
                "will_retry": will_retry,
                "retry_in_ms": retry_in_ms,
            }
        )
        self.status("reconnecting" if will_retry else "disconnected", reason)

    def message(self, text: str, index: int) -> None:
        emit(
            {
                "type": "message",
                "id": f"FAKEMSG{index:04d}",
                "chat_jid": PEER_JID,
                "sender_jid": PEER_JID,
                "sender_phone": "+" + PEER_JID.split("@")[0],
                "text": text,
                "media_kind": None,
                "timestamp": BASE_TIMESTAMP + index,
                "is_group": False,
                "from_me": False,
                "push_name": PUSHNAME,
            }
        )

    # -- entrada -----------------------------------------------------------
    def reader(self) -> None:
        """Le stdin num thread proprio: nenhum cenario pode travar por falta
        de comando (em CI a fixture roda com stdin em /dev/null)."""
        try:
            for raw in sys.stdin.buffer:
                line = raw.rstrip(b"\r\n")
                if len(line) > MAX_LINE_BYTES:
                    emit({"type": "error", "code": "protocol", "message": "line too long"})
                    self._hard_exit(EXIT_PROTOCOL)
                if not line.strip():
                    continue
                try:
                    command = json.loads(line.decode("utf-8"))
                except (ValueError, UnicodeDecodeError):
                    emit({"type": "error", "code": "protocol", "message": "line is not valid JSON"})
                    self._hard_exit(EXIT_PROTOCOL)
                    return
                if not isinstance(command, dict) or not isinstance(command.get("type"), str):
                    emit({"type": "error", "code": "protocol", "message": 'missing "type" field'})
                    self._hard_exit(EXIT_PROTOCOL)
                    return
                self.commands.put(command)
        finally:
            self.eof.set()

    @staticmethod
    def _hard_exit(code: int) -> None:
        sys.stdout.flush()
        # os._exit e proposital: o thread leitor nao pode esperar o main.
        os._exit(code)

    def take(self, wanted: str, timeout: float) -> dict | None:
        """Espera um comando especifico; devolve None em EOF/timeout."""
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            try:
                command = self.commands.get(timeout=min(remaining, 0.05))
            except queue.Empty:
                if self.eof.is_set() and self.commands.empty():
                    return None
                continue
            if command["type"] == wanted:
                return command
            emit(
                {
                    "type": "error",
                    "code": "bad_request",
                    "message": f'expected "{wanted}", got "{command["type"]}"',
                }
            )

    # -- cenarios ----------------------------------------------------------
    def run(self) -> int:
        if self.args.scenario == "garbage":
            # stdout com texto livre ANTES do handshake.
            sys.stdout.write("Debugger listening on ws://127.0.0.1:9229\n")
            sys.stdout.flush()
            time.sleep(self.args.hang_secs)
            return EXIT_OK

        emit(
            {
                "type": "started",
                "protocol": 99 if self.args.scenario == "bad-protocol" else PROTOCOL_VERSION,
                "bridge_version": BRIDGE_VERSION,
                "baileys_version": "fake",
                "node_version": "fake",
            }
        )
        if self.args.scenario == "bad-protocol":
            time.sleep(self.args.hang_secs)
            return EXIT_OK
        if self.args.scenario == "oversized":
            sys.stdout.write("x" * (300 * 1024) + "\n")
            sys.stdout.flush()
            time.sleep(self.args.hang_secs)
            return EXIT_OK

        threading.Thread(target=self.reader, daemon=True).start()

        loaded = self.take("session_load", self.args.handshake_timeout)
        start = self.take("start", self.args.handshake_timeout)
        mode = start.get("mode") if isinstance(start, dict) else None
        if mode not in ("pair", "serve"):
            mode = "serve" if self.args.scenario == "serve-echo" else "pair"

        self.status("connecting")
        scenario = self.args.scenario

        if scenario == "session-ok":
            # Sessao valida: o bridge real NUNCA emite QR neste caminho.
            if not (isinstance(loaded, dict) and loaded.get("session")):
                sys.stderr.write("session-ok exige session_load com sessao\n")
                return EXIT_FATAL
            self.connected()
            self.session_update()
            return EXIT_OK if mode == "pair" else self.serve()

        if scenario == "logged-out":
            self.disconnected(401, "logged_out", False, 0)
            emit({"type": "logged_out"})
            return EXIT_LOGGED_OUT

        if scenario == "hang":
            self.qr()
            self.eof.wait(self.args.hang_secs)
            return EXIT_OK

        self.qr()
        time.sleep(self.args.qr_expires)

        if scenario == "crash-after-qr":
            # Morte abrupta: nenhum evento de despedida, exatamente como um
            # processo que some no meio do fluxo.
            sys.stderr.write("fake bridge: simulated crash\n")
            return EXIT_FATAL

        if scenario == "pair-expire-then-ok":
            self.disconnected(408, "timeout", True, 1000)
            self.qr()
            time.sleep(self.args.qr_expires)

        emit({"type": "authenticated"})
        self.status("authenticated")
        # 515 logo apos o pareamento e o comportamento real do WhatsApp.
        self.disconnected(515, "restart_required", True, 0)
        self.connected()
        self.session_update()

        if scenario == "network-flap":
            self.disconnected(428, "network", True, 1000)
            self.connected()
            self.session_update()

        if scenario == "network-flap" and mode == "serve":
            # A ponte MORRE aqui, e e isso que obriga o driver a reconectar.
            # Sem esta saida o processo vivia ate o `shutdown` e as duas
            # `connected` acima saiam da MESMA execucao: o teste do lado Rust
            # contava reconexao sem que nenhuma tivesse acontecido.
            return EXIT_OK

        if scenario == "connect-then-hang":
            # Conectou, entregou a sessao e emudeceu sem fechar o stdout. O
            # `pair` para de contar silencio ao conectar (ele espera o
            # `session_update` final), entao sem o teto de flush final do
            # driver este processo pendura o terminal para sempre.
            self.eof.wait(self.args.hang_secs)
            return EXIT_OK

        if mode == "pair":
            self.session_update()
            return EXIT_OK

        return self.serve()

    def serve(self) -> int:
        while True:
            try:
                command = self.commands.get(timeout=0.05)
            except queue.Empty:
                if self.eof.is_set() and self.commands.empty():
                    self.session_update()
                    return EXIT_OK
                continue

            kind = command["type"]
            if kind == "send":
                self.sent += 1
                emit(
                    {
                        "type": "sent",
                        "request_id": command.get("request_id"),
                        "id": f"FAKESENT{self.sent:04d}",
                    }
                )
                if self.args.scenario == "serve-echo":
                    self.message(f"echo: {command.get('text', '')}", self.sent)
            elif kind in ("read", "typing"):
                pass  # best-effort, sem resposta — igual a ponte real
            elif kind == "logout":
                emit({"type": "logged_out"})
                return EXIT_OK
            elif kind == "shutdown":
                self.session_update()
                return EXIT_OK
            else:
                emit(
                    {
                        "type": "error",
                        "code": "bad_request",
                        "message": f'unknown command "{kind}"',
                    }
                )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Fake WhatsApp bridge (protocolo NDJSON v1)")
    parser.add_argument("--scenario", choices=SCENARIOS, default="pair-ok")
    parser.add_argument("--qr-expires", type=float, default=20.0, help="segundos ate o QR expirar")
    parser.add_argument("--hang-secs", type=float, default=3600.0, help="duracao do cenario hang")
    parser.add_argument(
        "--handshake-timeout",
        type=float,
        default=0.5,
        help="espera por session_load/start antes de assumir os defaults",
    )
    args = parser.parse_args(argv)
    return Bridge(args).run()


if __name__ == "__main__":
    _code = main(sys.argv[1:])
    sys.stdout.flush()
    sys.stderr.flush()
    # `os._exit` e nao `sys.exit`: o thread leitor e daemon e fica bloqueado
    # num `read` de stdin. Quando o pai fecha o stdin no mesmo instante em que
    # o interpretador finaliza, esse thread acorda DEPOIS do `Py_Finalize` e
    # o CPython aborta com SIGABRT — e um processo morto por sinal nao tem
    # codigo de saida, entao o supervisor Rust perde justamente o bit que
    # distingue "sessao morta" (2) de "encerramento normal" (0).
    os._exit(_code)

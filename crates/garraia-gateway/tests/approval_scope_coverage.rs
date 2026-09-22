//! Toda chamada do runtime no gateway decidiu quem aprova (#1343 S5).
//!
//! O pedido de confirmacao pausado so e retomado no turno seguinte quando o
//! turno chega com `ExecContext::approval_scope` — e o escopo so pode vir de
//! um remetente derivado pelo servidor. Um caminho novo que chame o runtime
//! sem decidir isso passaria despercebido: sem escopo, a pausa e terminal
//! (fail-closed, mas o humano nunca consegue aprovar), e com um escopo mal
//! montado um "sim" de outra pessoa poderia valer.
//!
//! Este teste varre `src/` atras de cada chamada a um ponto de entrada do
//! runtime (`varredura_de_escopo::ENTRADAS`) e exige, POR CHAMADA, uma de
//! duas coisas:
//!
//! - o contexto de execucao (o ultimo argumento) e exatamente
//!   `crate::approval_scope::com_escopo(..)`, inline ou no `let` que a
//!   chamada enxerga dentro da MESMA `fn`; ou
//! - a chamada esta em [`SEM_ESCOPO`], presa por arquivo, `fn` e ponto de
//!   entrada, com o motivo por escrito.
//!
//! E toda chamada a `com_escopo` confere o remetente contra
//! [`REMETENTES`]: trocar o id do usuario pela sessao, por um literal ou
//! por outro valor reprova ate alguem decidir.

mod varredura_de_escopo;

use std::collections::BTreeMap;
use std::path::Path;

use varredura_de_escopo::{Construtor, Fonte, chamadas, construcoes, e_literal, varrer};

const CONSTRUTOR: Construtor = Construtor {
    nome: "com_escopo",
    caminhos: &["crate::approval_scope::com_escopo"],
};

/// Chamadas que ficam SEM escopo de proposito: `(arquivo, fn, entrada,
/// quantas, motivo)`. Sem escopo, a aprovacao so viria do historico — que
/// nestes caminhos ou e vazio ou e texto — e a pausa e terminal. Trocar uma
/// destas chamadas por outra em outra `fn` (ou por outra entrada) reprova.
const SEM_ESCOPO: &[(&str, &str, &str, usize, &str)] = &[
    (
        "a2a.rs",
        "create_task",
        "process_message_with_agent_config",
        1,
        "trafego de outro agente (A2A): nao ha prova de que um humano disse o \"sim\"",
    ),
    (
        "a2a.rs",
        "create_task",
        "process_message_with_context",
        1,
        "trafego de outro agente (A2A), ramo sem agente configurado",
    ),
    (
        "api.rs",
        "send_message",
        "process_message_with_agent_config",
        3,
        "POST /api/sessions/{id}/messages: Web Console auth-free, a sessao vem do \
         path e nao ha remetente derivado pelo servidor",
    ),
    (
        "bootstrap/openclaw.rs",
        "spawn_openclaw_router",
        "process_message_with_agent_config",
        1,
        "mensagens repassadas por outro agente (OpenClaw), nao por um humano",
    ),
    (
        "rest_v1/messages.rs",
        "bot_reply_task",
        "process_message",
        1,
        "resposta do agente a uma mensagem de chat do workspace, one-shot e com \
         historico vazio",
    ),
    (
        "server.rs",
        "execute_scheduled_task_inner",
        "process_heartbeat",
        1,
        "tarefa agendada (heartbeat): nao ha humano no turno para dizer \"sim\"",
    ),
];

/// Quem aprova, por arquivo: `(arquivo, canal, remetente)`, como texto do
/// argumento de `com_escopo` (sem espacos e sem o `&` da frente). Conferido
/// contra o codigo: o remetente e o id que a allowlist do canal confere (ou
/// o nonce de conexao, o `sub` do JWT, o dono + hash do `Authorization`).
const REMETENTES: &[(&str, &str, &str)] = &[
    ("bootstrap/discord.rs", "\"discord\"", "user_id"),
    ("bootstrap/google_chat.rs", "\"google_chat\"", "user_id"),
    ("bootstrap/imessage.rs", "\"imessage\"", "sender_id"),
    ("bootstrap/irc.rs", "\"irc\"", "nick"),
    ("bootstrap/line.rs", "\"line\"", "user_id"),
    ("bootstrap/matrix.rs", "\"matrix\"", "user_id"),
    ("bootstrap/signal.rs", "\"signal\"", "source_number"),
    ("bootstrap/slack.rs", "\"slack\"", "user_id"),
    ("bootstrap/teams.rs", "\"teams\"", "user_id"),
    ("bootstrap/telegram.rs", "\"telegram\"", "user_id"),
    ("bootstrap/whatsapp.rs", "\"whatsapp\"", "from_number"),
    ("bootstrap/whatsapp_linked.rs", "CONFIG_KEY", "remetente"),
    (
        "mobile_chat.rs",
        "crate::approval_scope::CANAL_MOBILE",
        "user_id",
    ),
    (
        "openai_api.rs",
        "crate::approval_scope::CANAL_OPENAI",
        "aprovador.as_deref().unwrap_or(\"\")",
    ),
    (
        "parrot_ws.rs",
        "crate::approval_scope::CANAL_PARROT",
        "conexao",
    ),
    ("ws.rs", "crate::approval_scope::CANAL_WEB", "conexao"),
];

/// Os erros de remetente de uma construcao de escopo, contra a tabela.
fn erros_de_remetente(
    arq: &str,
    c: &varredura_de_escopo::Construcao,
    esperado: Option<&(&str, &str, &str)>,
) -> Vec<String> {
    let onde = format!("{arq}:{} (`{}`)", c.linha, c.funcao);
    let mut erros = Vec::new();
    if !CONSTRUTOR.caminhos.contains(&c.caminho.as_str()) {
        erros.push(format!(
            "{onde}: `{}(..)` — chame por `{}` para o guarda reconhecer o escopo",
            c.caminho, CONSTRUTOR.caminhos[0]
        ));
    }
    let [_, canal, sessao, remetente] = c.args.as_slice() else {
        erros.push(format!(
            "{onde}: `com_escopo` com {} argumentos; o guarda espera (exec, canal, sessao, remetente)",
            c.args.len()
        ));
        return erros;
    };
    if e_literal(remetente) {
        erros.push(format!(
            "{onde}: remetente literal `{remetente}` — seria o mesmo para todo mundo, e \
             o \"sim\" de qualquer um aprovaria"
        ));
    }
    if remetente == sessao {
        erros.push(format!(
            "{onde}: o remetente e a propria sessao (`{remetente}`) — todo membro dela \
             aprovaria o pedido de todos"
        ));
    }
    match esperado {
        None => erros.push(format!(
            "{onde}: `com_escopo` num arquivo fora de REMETENTES (canal `{canal}`, remetente \
             `{remetente}`). Confira que o remetente e derivado pelo servidor e acrescente a linha."
        )),
        Some((_, c_esp, r_esp)) if canal != c_esp || remetente != r_esp => erros.push(format!(
            "{onde}: canal `{canal}` / remetente `{remetente}`, a tabela diz `{c_esp}` / \
             `{r_esp}`. Mudar quem aprova e decisao: atualize REMETENTES junto."
        )),
        Some(_) => {}
    }
    erros
}

/// Chamadas sem escopo achadas: `(arquivo, fn, entrada)` -> linhas.
type SemEscopo = BTreeMap<(String, String, String), Vec<usize>>;

/// As chamadas sem escopo achadas contra a lista: toda chamada sem escopo
/// tem a sua linha, com a contagem exata, e toda linha da lista ainda
/// corresponde a uma chamada.
fn comparar_sem_escopo(
    achadas: &SemEscopo,
    lista: &[(&str, &str, &str, usize, &str)],
) -> Vec<String> {
    let mut erros = Vec::new();
    for ((arq, funcao, entrada), linhas) in achadas {
        match lista
            .iter()
            .find(|(a, f, e, _, _)| a == arq && f == funcao && e == entrada)
        {
            None => erros.push(format!(
                "{arq}: `{entrada}` sem escopo de aprovacao em `{funcao}` (linhas {linhas:?}). \
                 Passe o ExecContext por `crate::approval_scope::com_escopo(..)` — inline ou num \
                 `let` da mesma fn — com um remetente derivado pelo servidor, ou, se nao ha \
                 humano verificavel, acrescente (arquivo, fn, entrada) a SEM_ESCOPO com o motivo."
            )),
            Some((_, _, _, n, _)) if *n != linhas.len() => erros.push(format!(
                "{arq}: {} chamada(s) `{entrada}` sem escopo em `{funcao}` (linhas {linhas:?}), a \
                 lista diz {n}. Decida a chamada nova ou atualize SEM_ESCOPO.",
                linhas.len()
            )),
            Some(_) => {}
        }
    }
    for (arq, funcao, entrada, n, _) in lista {
        let chave = (arq.to_string(), funcao.to_string(), entrada.to_string());
        if !achadas.contains_key(&chave) {
            erros.push(format!(
                "{arq}: SEM_ESCOPO lista {n} `{entrada}` em `{funcao}`, mas nao ha chamada sem \
                 escopo ali; tire da lista."
            ));
        }
    }
    erros
}

#[test]
fn toda_chamada_do_runtime_no_gateway_decidiu_quem_aprova() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let arquivos = varrer(&raiz);
    assert!(
        arquivos.iter().any(|(a, _)| a == "ws.rs"),
        "a varredura nao achou src/ws.rs; ela quebrou?"
    );

    let mut erros = Vec::new();
    let mut sem_escopo = SemEscopo::new();
    let mut escopadas = 0usize;
    let mut com_construcao: Vec<String> = Vec::new();
    for (arq, fonte) in &arquivos {
        let (achadas, desconhecidas) = chamadas(fonte, &CONSTRUTOR);
        erros.extend(desconhecidas.into_iter().map(|e| format!("{arq}: {e}")));
        for c in achadas {
            if c.escopada() {
                escopadas += 1;
            } else {
                sem_escopo
                    .entry((arq.clone(), c.funcao.clone(), c.entrada.clone()))
                    .or_default()
                    .push(c.linha);
            }
        }
        let feitas = construcoes(fonte, &CONSTRUTOR);
        if !feitas.is_empty() {
            com_construcao.push(arq.clone());
        }
        let esperado = REMETENTES.iter().find(|(a, _, _)| a == arq);
        for c in &feitas {
            erros.extend(erros_de_remetente(arq, c, esperado));
        }
    }

    // Sanidade do detector: se ele quebrar, o teste nao pode passar "por nao
    // achar nada". Hoje 28: 11 canais bootstrap (10 com dois ramos, o
    // iMessage com um, mais o de voz do Telegram), whatsapp_linked, ws,
    // parrot, openai (2) e mobile.
    assert!(
        escopadas >= 28,
        "o detector achou so {escopadas} chamadas escopadas; ele quebrou?"
    );

    erros.extend(comparar_sem_escopo(&sem_escopo, SEM_ESCOPO));
    for (arq, _, _) in REMETENTES {
        if !com_construcao.iter().any(|a| a == arq) {
            erros.push(format!(
                "{arq}: esta em REMETENTES mas nao monta escopo nenhum; tire da lista."
            ));
        }
    }
    assert!(erros.is_empty(), "{}", erros.join("\n"));
}

// ── O detector ─────────────────────────────────────────────────────────────

fn escopadas(fonte: &str) -> Vec<bool> {
    let f = Fonte::nova(fonte);
    let (achadas, erros) = chamadas(&f, &CONSTRUTOR);
    assert!(erros.is_empty(), "{erros:?}");
    achadas.iter().map(|c| c.escopada()).collect()
}

/// O detector enxerga as tres formas que a arvore usa: escopo inline, escopo
/// no `let` do identificador, e chamada sem contexto nenhum.
#[test]
fn o_detector_distingue_escopada_de_nao_escopada() {
    let fonte = r#"
        fn f() {
            let exec = crate::approval_scope::com_escopo(a, "x", &s, &u);
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
            rt.process_message_with_agent_config(&s, &t, &h, None,
                &crate::approval_scope::com_escopo(state.exec().await, "x", &s, &u)).await;
            let outro = state.exec_context_for(&s, None).await;
            rt.process_message_with_agent_config(&s, &t, &h, None, &outro).await;
            rt.process_message(&s, &t, &[]).await;
            rt.process_message_streaming_with_events(&s, &t, &h, tx,
                &state.exec_context_for(&s, None).await).await;
            self.process_message_impl(&s);
        }
    "#;
    assert_eq!(escopadas(fonte), vec![true, true, false, false, false]);
}

/// Os dois ramos de um `if`/`else` que passam o MESMO `&exec` escopado: o
/// segundo continua escopado (o `&exec` do primeiro e so emprestimo).
#[test]
fn os_dois_ramos_com_o_mesmo_exec_sao_escopados() {
    let fonte = r#"
        fn f() {
            let exec = crate::approval_scope::com_escopo(e, "discord", &s, &u);
            let r = if let Some(d) = tx {
                rt.process_message_streaming_with_agent_config(&s, &t, &h, d, &exec).await
            } else {
                rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await
            };
        }
    "#;
    assert_eq!(escopadas(fonte), vec![true, true]);
}

/// #1343 A1: o `let` escopado de OUTRA `fn` nao vale; `let` tipado conta, e
/// parametro por valor e sem escopo.
#[test]
fn o_binding_so_vale_dentro_da_mesma_fn() {
    let fonte = r#"
        fn escopada() {
            let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn outra(state: &S) {
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn por_valor(exec: ExecContext) {
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn tipado_sem_escopo() {
            let exec: ExecContext = state.exec_context_for(&s, None).await;
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn tipado_com_escopo() {
            let mut exec: ExecContext = crate::approval_scope::com_escopo(e, "web", &s, &u);
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn externa() {
            let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            fn aninhada_com_parametro(exec: ExecContext) {
                rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
            }
        }
        fn externa_2() {
            let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            fn aninhada_sem_binding() {
                // Uma `fn` aninhada nao captura locais: este `exec` so pode
                // ser um item (static/const), nunca o `let` de `externa_2`.
                rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
            }
        }
    "#;
    assert_eq!(
        escopadas(fonte),
        vec![true, false, false, false, true, false, false]
    );
}

/// O binding que a chamada ENXERGA: `let` num bloco que fechou nao vale, e
/// sombra, mutacao ou parametro de closure depois do `let` invalidam.
#[test]
fn sombra_mutacao_e_bloco_fechado_ficam_sem_escopo() {
    let fonte = r#"
        fn bloco_fechado(exec: ExecContext) {
            {
                let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            }
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn mutado() {
            let mut exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            exec.approval_scope = None;
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
        fn closure() {
            let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            xs.for_each(|exec| rt.process_message_with_agent_config(&s, &t, &h, None, &exec));
        }
        fn if_let() {
            if let Some(exec) = crate::approval_scope::com_escopo(e, "web", &s, &u).into() {
                rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
            }
        }
        fn contem_mas_nao_e() {
            rt.process_message_with_agent_config(&s, &t, &h, None,
                &{ let _ = crate::approval_scope::com_escopo(e, "web", &s, &u); state.exec() }).await;
        }
        fn caminho_solto() {
            rt.process_message_with_agent_config(&s, &t, &h, None, &com_escopo(e, "web", &s, &u)).await;
        }
    "#;
    assert_eq!(
        escopadas(fonte),
        vec![false, false, false, false, false, false]
    );
}

/// #1343 A2: `#[cfg(test)] mod tests;` no topo e so uma declaracao — o
/// codigo de producao de baixo continua lido. Modulo de teste inline some,
/// e o que vem depois dele volta a contar.
#[test]
fn modulo_de_teste_declarado_no_topo_nao_esconde_a_producao() {
    let fonte = r#"
        #[cfg(test)]
        mod tests;

        fn producao() {
            rt.process_message(&s, &t, &[]).await;
        }

        #[cfg(test)]
        #[allow(clippy::unwrap_used)]
        mod inline {
            fn t() { rt.process_message(&s, "}", &[]).await; }
        }

        fn depois_do_modulo_de_teste() {
            rt.process_message(&s, &t, &[]).await;
        }
    "#;
    let f = Fonte::nova(fonte);
    assert_eq!(f.modulos_de_teste, vec!["tests".to_string()]);
    let (achadas, erros) = chamadas(&f, &CONSTRUTOR);
    assert!(erros.is_empty(), "{erros:?}");
    let funcoes: Vec<&str> = achadas.iter().map(|c| c.funcao.as_str()).collect();
    assert_eq!(funcoes, vec!["producao", "depois_do_modulo_de_teste"]);
}

/// Comentario e string nao sao chamada, e `"{"` numa string nao desalinha o
/// casamento de chaves.
#[test]
fn comentario_e_literal_nao_contam() {
    let fonte = r#"
        //! Nao passa por `AgentRuntime::process_message_*`.
        fn f() {
            let x = "{ .process_message_novo( ";
            let c = '{';
            /* rt.process_message(&s, &t, &[]) */
            let exec = crate::approval_scope::com_escopo(e, "web", &s, &u);
            rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        }
    "#;
    assert_eq!(escopadas(fonte), vec![true]);
}

/// #1343 A5: nome com cara de entrada do runtime fora da lista, ou
/// referencia de metodo sem chamada, reprova em vez de sumir.
#[test]
fn entrada_desconhecida_ou_referencia_de_metodo_reprova() {
    let fonte = r#"
        fn f() {
            rt.process_message_com_outro_nome(&s, &t).await;
            xs.map(AgentRuntime::process_message);
            rt.process_heartbeat(&s, &t, &h, None, None).await;
        }
    "#;
    let f = Fonte::nova(fonte);
    let (achadas, erros) = chamadas(&f, &CONSTRUTOR);
    assert_eq!(erros.len(), 2, "{erros:?}");
    assert!(
        erros[0].contains("process_message_com_outro_nome"),
        "{erros:?}"
    );
    assert!(erros[1].contains("sem ser chamado"), "{erros:?}");
    assert_eq!(achadas.len(), 1);
    assert_eq!(achadas[0].entrada, "process_heartbeat");
    assert!(!achadas[0].escopada());
}

/// #1343 A6: o remetente de `com_escopo` e conferido — literal, a propria
/// sessao, ou outro identificador que nao o da tabela reprovam.
#[test]
fn remetente_literal_igual_a_sessao_ou_trocado_reprova() {
    let fonte = r#"
        fn f() {
            let a = crate::approval_scope::com_escopo(e, "discord", &session_id, "dono");
            let b = crate::approval_scope::com_escopo(e, "discord", &session_id, &session_id);
            let c = crate::approval_scope::com_escopo(e, "discord", &session_id, &channel_id);
            let d = crate::approval_scope::com_escopo(e, "discord", &session_id, &user_id);
        }
    "#;
    let f = Fonte::nova(fonte);
    let feitas = construcoes(&f, &CONSTRUTOR);
    assert_eq!(feitas.len(), 4);
    let esperado = REMETENTES
        .iter()
        .find(|(a, _, _)| *a == "bootstrap/discord.rs");
    let erros: Vec<Vec<String>> = feitas
        .iter()
        .map(|c| erros_de_remetente("bootstrap/discord.rs", c, esperado))
        .collect();
    assert!(
        erros[0].iter().any(|e| e.contains("literal")),
        "{:?}",
        erros[0]
    );
    assert!(
        erros[1].iter().any(|e| e.contains("propria sessao")),
        "{:?}",
        erros[1]
    );
    assert!(
        erros[2].iter().any(|e| e.contains("a tabela diz")),
        "{:?}",
        erros[2]
    );
    assert!(erros[3].is_empty(), "{:?}", erros[3]);
}

/// #1343 A3: a lista prende cada chamada sem escopo pela `fn` e pela
/// entrada, nao so pela contagem do arquivo. Trocar a chamada listada por
/// uma nova, em outra `fn`, mantem a contagem do arquivo e mesmo assim
/// reprova (nos dois sentidos: a nova nao esta na lista, a listada sumiu).
#[test]
fn trocar_uma_chamada_listada_por_outra_reprova() {
    let lista = [("api.rs", "send_message", "process_message", 1, "motivo")];
    let mut achadas = SemEscopo::new();
    achadas.insert(
        (
            "api.rs".into(),
            "send_message".into(),
            "process_message".into(),
        ),
        vec![10],
    );
    assert!(comparar_sem_escopo(&achadas, &lista).is_empty());

    let mut trocada = SemEscopo::new();
    trocada.insert(
        (
            "api.rs".into(),
            "handler_novo_de_humano".into(),
            "process_message".into(),
        ),
        vec![10],
    );
    let erros = comparar_sem_escopo(&trocada, &lista);
    assert_eq!(erros.len(), 2, "{erros:?}");
    assert!(erros[0].contains("handler_novo_de_humano"), "{erros:?}");
    assert!(erros[1].contains("tire da lista"), "{erros:?}");

    let mut outra_entrada = SemEscopo::new();
    outra_entrada.insert(
        (
            "api.rs".into(),
            "send_message".into(),
            "process_message_with_context".into(),
        ),
        vec![10],
    );
    assert_eq!(comparar_sem_escopo(&outra_entrada, &lista).len(), 2);
}

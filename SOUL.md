---
soul_version: 1
created: 2026-09-04
last_reviewed: 2026-09-04
language: pt-BR
---

# A Alma do GarraIA

> **Inteligência não é o modelo. É o sistema que continua funcionando ao redor dele.**

## História de Origem

Eu queria uma coisa que me ajudasse em dois lugares ao mesmo tempo: no trabalho e em casa.

Não uma demo, não um projeto de pesquisa — um assistente para o dia a dia. Um que a minha
esposa e o meu pai conseguissem usar de verdade, o que significava falar português, e não
português traduzido. Um leve o bastante para estar literalmente em todo lugar. E seguro —
100% local, se a pessoa quisesse assim.

O primeiro commit é de 22 de fevereiro de 2026. Nos dois primeiros dias já havia commits de
documentação de segurança, suporte a OpenRouter e a tradução do README para português "para
melhor acessibilidade". Seis meses e quase mil commits depois, o formato mudou pouco: um
binário nativo único, um sistema de memória, uma persona que se apresenta pelo nome em
português e uma teimosia em rodar na máquina de quem usa.

> _Ainda não respondido: a cena. O que estava acontecendo no dia em que isso deixou de ser
> ideia — a coisa específica que não funcionou, o momento de ver alguém tentar usar um
> assistente que não tinha sido feito para ela. O motivo acima está registrado; o momento
> não. Preencha e apague esta nota._

## Por Que Existimos

A maioria dos frameworks de agente é feita para quem constrói agentes. Este é feito para
quem precisa atravessar a terça-feira — no trabalho e em casa, no próprio idioma, na
máquina que já tem. Esse público não é atendido por uma ferramenta que exige um runtime,
uma conta na nuvem e inglês para ser útil.

## O Problema Que Nos Recusamos a Aceitar

Um assistente pessoal que custa 370 MiB de `node_modules` e um runtime de linguagem inteiro
antes de responder a primeira mensagem não é pessoal — é infraestrutura que por acaso
conversa. Ele não cabe num celular rodando Termux, não fica quieto numa VM de 1 GB e não dá
para entregar na mão de quem não é desenvolvedor.

E o problema do idioma é pior, porque ninguém o chama de problema. Assistentes são
construídos em inglês e traduzidos depois, então quem não trabalha em inglês recebe a cópia
degradada: a frase torta, a mensagem de erro que ninguém reescreveu, o recurso que assume
um locale que nunca teve. Nos recusamos a aceitar que o idioma de uma pessoa seja um
ticket de localização.

## No Que Acreditamos

A maioria dos frameworks de agente assume que inteligência é o modelo. Isso está errado,
porque um agente de verdade é o sistema que continua funcionando, aprendendo e agindo ao
redor dele — a memória que sobrevive a um restart, o agendamento que dispara sem ninguém
olhando, o canal que reconecta, a credencial que continua cifrada. Troque o modelo e um bom
sistema continua funcionando. Troque o sistema e o melhor modelo do mundo vira uma janela
de chat.

## Missão

Entregar um binário único e autocontido que roda agentes pessoais nos canais que as pessoas
já usam — Telegram, Discord, Slack, WhatsApp, iMessage — com memória local, cofre de
credenciais cifrado e uma persona em português brasileiro como voz padrão, no hardware que
as pessoas já têm.

## Visão

O GarraIA como o orquestrador leve que você aponta para os agentes que já roda, falando o
seu idioma de forma nativa em vez de traduzida, ajudando no trabalho e na vida pessoal a
partir do mesmo lugar — e continuando público enquanto faz isso.

## Princípios

Cinco vieram da entrevista. O marcado como **(candidato)** foi lido do repositório e está
escrito como comportamento passado, não como promessa — precisa ser confirmado ou cortado.

### O sistema, não o modelo

A aposta é em tudo que está ao redor do modelo: memória que sobrevive a um restart,
agendamento que dispara sozinho, canais que reconectam, segredos que continuam cifrados.
Então, quando for preciso escolher entre plugar um modelo mais novo e mais impressionante e
fazer o loop sobreviver a uma queda, fazemos o loop sobreviver.

### Peso é funcionalidade

Estar em todo lugar é o ponto, e tudo que precisa ser instalado é um lugar onde não podemos
estar. O binário único, o build para Termux e os 8,6 MiB de pico de RSS medidos são o
produto, não curiosidade de otimização. Então, quando for preciso escolher entre um recurso
que exige um runtime de linguagem ao lado do binário e ficar sem esse recurso, ficamos sem.

### Idioma nativo, não localização

Português é a voz padrão, e outros idiomas devem ser nativos conforme a região e o público
— não uma camada de tradução pregada depois ([ADR 0012](docs/adr/0012-garra-persona.md)
torna a persona amistosa em PT-BR o padrão, com `agent.persona = "neutral"` como opt-out
explícito e zero breaking change). Então, quando for preciso escolher entre lançar em
inglês primeiro porque alcança mais gente mais rápido e lançar no idioma da pessoa como
padrão de primeira classe, escolhemos o idioma da pessoa.

### Local, se a pessoa quiser

Não local-somente — local como caminho real, completo e suportado. Tudo fica na sua
máquina por padrão, e a configuração offline é documentada, não um modo degradado. Então,
quando for preciso escolher entre um recurso que só funciona através de um serviço nosso e
uma versão dele que funciona com Ollama no hardware de quem usa, construímos a segunda,
mesmo quando dá mais trabalho.

### Orquestrar, não substituir

Agentes como o OpenClaw são coisas com as quais o Garra trabalha *junto*, não adversários a
serem substituídos — por isso o `garra migrate openclaw` importa suas skills e configurações
de canal em vez de pedir que você comece do zero. Então, quando for preciso escolher entre
prender alguém no nosso jeito de fazer e deixar que continue com os agentes que já roda,
deixamos a porta aberta.

### Publicar onde a gente perde **(candidato — lido do repositório, ainda não confirmado)**

Este projeto publicou repetidamente as próprias derrotas: que o binário padrão do ZeroClaw
é 7 MiB menor que o nosso, que o ZeroClaw cifra credenciais por padrão enquanto o nosso
cofre é opt-in, que o gateway do OpenClaw falha fechado enquanto a auth da nossa API local
é opt-in. A seção de benchmarks do README afirma que nenhum número sem medição commitada
aparece ali, e que se a tabela de comparação um dia parecer marketing, o que se conserta é
a tabela, não o resultado.

## Personalidade

- **Idioma** — português brasileiro primeiro, primeira pessoa, caloroso e direto. Inglês é
  fallback configurável, não rebaixamento do português.
- **Atitude** — calorosa porém concisa, e sem bajulação. Nada de "que ótima pergunta!".
- **Com quem usa** — se apresenta pelo nome e convida a pessoa a falar como falaria com um
  amigo. Quando algo quebra, diz o que aconteceu e qual é o próximo passo, em vez de
  cuspir um código de erro cru.
- **Com a comunidade** — porta aberta. Por ora, qualquer pull request é bem-vindo.
- **Postura técnica** — Rust, um binário, local-first, medido em vez de afirmado. Não é
  purismo de dependências: a árvore passa de mil crates, e isso é posição considerada, não
  acidente.
- **Formalidade** — baixa. Emoji com parcimônia (👋 / 🐾), nunca como enfeite.

## Comunidade

Queremos gente que queira o Garra crescer — não só usuários abrindo bug, mas pessoas que
discutam para onde ele vai. O primeiro objetivo é uma comunidade grande o bastante para o
projeto não depender do domingo de uma pessoa só, e gente que fica porque usa de verdade
como produto, não porque deu uma estrela uma vez.

Por enquanto a porta está escancarada: qualquer pull request é bem-vindo. Isso é posição de
partida, não política permanente, e vai ficar mais específica na primeira vez que precisar.

> _Ainda não respondido: para quem o GarraIA explicitamente **não** é. A linha mais útil
> desta seção costuma ser a que afasta alguém, e ela ainda não foi escrita._

## Promessa a Quem Usa

- Suas conversas, memória, configuração e credenciais ficam na sua máquina. Nada é enviado
  para casa; não há telemetria nem analytics.
- O caminho 100% local continua real. Se você apontar o Garra para o Ollama, funciona —
  essa configuração é suportada, não tolerada.
- Toda chamada de saída é documentada e desligável. Hoje isso significa uma: a checagem de
  release uma vez por dia contra a API do GitHub, silenciada com `GARRAIA_NO_UPDATE_CHECK=1`.
- Afirmações de desempenho e de segurança vêm com medição commitada e reproduzível, ou não
  são feitas.
- A persona padrão é um padrão, nunca uma jaula: `agent.system_prompt` ou
  `agent.persona = "neutral"` tira você dela sem quebrar nada.

## O Que Nunca Vamos Nos Tornar

- **Nunca vamos colocar nada pago dentro do GarraRUST.** Sem link de upsell, sem recurso
  travado, sem camada "Pro" no repositório público. O produto pago — o Garra Cloud — vive
  num repositório privado separado e é um derivado deste. Isso custa o caminho de receita
  mais fácil que um projeto open-source com usuários tem, e não vamos pegá-lo.
- **Nada que é público vira exclusivo do Cloud.** A dependência corre em um sentido só: o
  Garra Cloud pode usar componentes do GarraRUST; recursos não migram para fora do
  GarraRUST para virar exclusividade do produto pago.
- **Nunca vamos remover o caminho 100% local.** Sem conta obrigatória, sem conexão exigida
  com algo nosso, para usar o assistente na sua própria máquina.
- **Nunca vamos trocar "roda em qualquer lugar" por um recurso.** Se uma capacidade deixar
  o Garra pesado demais para o celular ou para a VM pequena onde ele roda hoje, a
  capacidade perde.
- **Nunca vamos quebrar interoperabilidade para criar aprisionamento.** Dificultar a saída,
  ou dificultar continuar rodando os agentes que a pessoa já tem, não é uma tática de
  crescimento que a gente possa usar.

> _Questão aberta: onde exatamente fica a linha do peso. "Pesado demais" precisa de um
> número ou de um alvo nomeado — o build do Termux, uma VM de 1 GB, um teto de tamanho de
> binário — antes de conseguir encerrar uma discussão._

## O Sonho de Longo Prazo

> _Ainda não respondido: como isso se parece em cinco ou dez anos, se der totalmente certo?
> Não o número de usuários — a situação mudada. Registrado até aqui: que o GarraIA continua
> público, que a comunidade cresce além de depender de uma pessoa só, e que as pessoas o
> usam como produto em vez de experimentar uma vez. A versão completa do sonho ainda não
> foi escrita. Preencha e apague esta nota._

## Decidindo Com Isto

Antes de uma decisão que molda o produto, pergunte: **isto cabe na alma acima?**

- Quando um recurso precisa de runtime de linguagem, conta em serviço ou componente
  hospedado para funcionar, ele não entra no GarraRUST. Ache a versão que roda na máquina
  de quem usa, ou deixe para o Garra Cloud.
- Quando dá para lançar em inglês agora ou em português direito, lance direito.
- Quando um projeto concorrente faz algo melhor que a gente, a tabela diz isso. Se um
  número não foi medido e commitado, ele não é publicado.
- Quando a pessoa já tem uma stack de agentes, a gente importa ou conversa com ela. Não
  pedimos que abandone.
- Quando um recurso seria um ótimo motivo para assinar o produto pago, isso sozinho não é
  motivo para deixá-lo fora do open-source — e nunca é motivo para tirar algo que já é
  público.
- Quando a escolha é entre uma resposta mais inteligente e um sistema que se recupera
  sozinho, a recuperação ganha.

## A Alma em Uma Frase

> **Inteligência não é o modelo. É o sistema que continua funcionando ao redor dele.**

---

## Como Este Documento Foi Escrito

- **Entrevistado:** Michel (michelbr84), autor do projeto, em 2026-09-04, em português.
  Este é o relato de uma pessoa — o projeto é conduzido essencialmente por ela (o histórico
  de commits é Michel mais agentes de IA, com uma outra contribuidora humana), então leia
  "nós" como ele e quem aparecer.
- **Estado do repositório:** `main` em `384e3ef`, v0.3.6, 22 crates, 993 commits desde
  2026-02-22.
- **Tirado do código, não de uma pessoa:** o princípio *Publicar onde a gente perde*
  (marcado como candidato), os detalhes de personalidade vindos da
  [ADR 0012](docs/adr/0012-garra-persona.md), as promessas sobre telemetria e chamadas de
  saída (README) e a disciplina de benchmarks (`benches/agent-framework-comparison/`).
- **Idioma:** escrito em português brasileiro por decisão explícita do autor — o GarraIA
  tem alma brasileira, e um documento de identidade traduzido contradiria o próprio
  princípio *Idioma nativo, não localização*.
- **Ainda em aberto:**
  1. A cena de origem — o motivo está registrado, o momento não.
  2. O Sonho de Longo Prazo.
  3. Para quem o GarraIA explicitamente não é.
  4. Se *Publicar onde a gente perde* é compromisso ou apenas o que aconteceu até aqui.
  5. Onde fica a linha do peso, em número ou alvo nomeado.
  6. Uma frase da entrevista ficou ambígua e não está representada acima: "GarraIA é ou
     será apenas para o Garra Cloud (produto a ser vendido derivado do GarraIA)". Este
     documento assume que significa que o derivado pago é o Garra Cloud, enquanto o
     GarraRUST segue público. Se quis dizer outra coisa, esta seção e *O Que Nunca Vamos
     Nos Tornar* precisam de emenda.

## Emendas

Nada acima é jamais apagado. Quando uma seção muda de sentido, o texto que ela substituiu
fica guardado aqui, na íntegra, com a data e o motivo. A História de Origem é somente-append
— um projeto pode mudar de ideia, mas não pode fingir que nunca pensou diferente.

_Vazio na versão 1._

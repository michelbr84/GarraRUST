use garraia_common::{Error, Result};

/// Busca semântica de skills — **não implementada** (#964).
///
/// A doc anterior dizia "Returns empty list until then" e o corpo devolvia
/// `Err`. Duas frases sobre a mesma função, discordando: quem lesse o
/// comentário escreveria `retrieve(q)?.is_empty()` e levaria um erro em
/// produção. O comportamento que ficou é o `Err`, e agora a doc diz isso.
///
/// **Devolver `Ok(vec![])` seria pior.** Uma lista vazia é indistinguível de
/// "procurei e não achei nada", então o chamador concluiria que não existe
/// skill relevante — e seguiria em frente com um recall que nunca rodou. O
/// erro é a única resposta que não mente.
///
/// A dependência que a doc antiga citava (`garraia-embeddings`) é o crate
/// órfão do #949, que não é o caminho em uso. Quando isto for implementado, o
/// que precisa ser usado é o par que já funciona na memória do agente:
/// `garraia_agents::embeddings::EmbeddingProvider` para gerar o vetor e
/// `garraia_db::vector_store` (sqlite-vec) para o KNN — os mesmos que o
/// `garra memory search` exercita.
///
/// Hoje não há nenhum chamador no workspace.
pub fn retrieve(_query: &str) -> Result<Vec<crate::Skill>> {
    Err(Error::Other(
        "busca semantica de skills nao implementada (#964): quando for, use \
         garraia_agents::embeddings + garraia_db::vector_store, o mesmo par da \
         memoria do agente — nao o crate orfao garraia-embeddings"
            .into(),
    ))
}

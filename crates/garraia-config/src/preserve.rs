//! Edicao cirurgica do `config.yml` (#1825).
//!
//! `ConfigLoader::save` relia o arquivo para um `AppConfig` e reserializava
//! tudo: ordem das secoes, comentarios e chaves que o `AppConfig` nao conhece
//! se perdiam a cada campo gravado. Aqui o texto original e mantido e so os
//! trechos cujo valor mudou sao reescritos.
//!
//! Estrategia: percorre os blocos de mapeamento por indentacao; chave igual
//! fica intacta, mapeamento que mudou desce um nivel, qualquer outra mudanca
//! substitui so o trecho daquela chave. Chaves que a config conhecida nao
//! produz (desconhecidas) nunca sao tocadas. Se o resultado nao reparsear para
//! exatamente a config nova, [`patch`] devolve `None` e o chamador regrava o
//! arquivo inteiro — a preservacao e um favor, a correcao do valor e o contrato.

use serde_norway::{Mapping, Value};

/// Texto novo com so os campos alterados reescritos, ou `None` quando nao foi
/// possivel editar com seguranca.
pub(crate) fn patch(old_text: &str, new: &Value, old_canon: &Value) -> Option<String> {
    let old_raw: Value = serde_norway::from_str(old_text).ok()?;
    let lines: Vec<String> = old_text.lines().map(str::to_string).collect();
    let (Value::Mapping(raw), Value::Mapping(canon), Value::Mapping(new_map)) =
        (&old_raw, old_canon, new)
    else {
        return None;
    };
    let lines = patch_block(&lines, 0, raw, canon, new_map)?;
    let mut out = lines.join("\n");
    if old_text.ends_with('\n') || old_text.is_empty() {
        out.push('\n');
    }
    // Rede de seguranca: o que sai tem de significar exatamente a config nova.
    let reparsed: Value = serde_norway::from_str(&out).ok()?;
    let Value::Mapping(n) = new else {
        return None;
    };
    let canon_back: crate::model::AppConfig = serde_norway::from_value(reparsed.clone()).ok()?;
    let canon_back = serde_norway::to_value(&canon_back).ok()?;
    (canon_back == Value::Mapping(n.clone())).then_some(out)
}

fn indent_of(line: &str) -> Option<usize> {
    let t = line.trim_start_matches(' ');
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    Some(line.len() - t.len())
}

fn key_name(line: &str, indent: usize) -> Option<String> {
    let rest = &line[indent..];
    if rest.starts_with("- ") || rest.starts_with('-') && rest.len() == 1 {
        return None;
    }
    let rest = rest.trim_end();
    let (k, _) = if let Some(r) = rest.strip_prefix('"') {
        let e = r.find('"')?;
        (&r[..e], &r[e + 1..])
    } else if let Some(r) = rest.strip_prefix('\'') {
        let e = r.find('\'')?;
        (&r[..e], &r[e + 1..])
    } else {
        let e = rest.find(':')?;
        (&rest[..e], &rest[e..])
    };
    Some(k.to_string())
}

/// Chaves diretas do bloco `[start, end)` na indentacao `indent`:
/// `(nome, inicio, fim)`; o inicio inclui comentarios/linhas em branco que
/// vem logo antes da chave, para que continuem com ela.
/// Chaves diretas do bloco na indentacao `indent`: `(nome, linha da chave,
/// fim exclusivo)`. Comentarios/brancos colados na proxima chave ficam com ela.
fn keys_of(lines: &[String], indent: usize) -> Option<Vec<(String, usize, usize)>> {
    let mut keys: Vec<(String, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        match indent_of(line) {
            None => {}
            Some(n) if n == indent => {
                if let Some(name) = key_name(line, n) {
                    keys.push((name, i));
                }
                // `- item` na mesma indentacao continua a chave anterior
            }
            Some(n) if n < indent => return None,
            Some(_) => {}
        }
    }
    let mut out = Vec::new();
    for (idx, (name, kline)) in keys.iter().enumerate() {
        let mut e = keys.get(idx + 1).map_or(lines.len(), |(_, nk)| *nk);
        while e > kline + 1 && indent_of(&lines[e - 1]).is_none() {
            e -= 1;
        }
        out.push((name.clone(), *kline, e));
    }
    Some(out)
}

fn render(key: &Value, val: &Value, indent: usize) -> Option<Vec<String>> {
    let mut m = Mapping::new();
    m.insert(key.clone(), val.clone());
    let s = serde_norway::to_string(&Value::Mapping(m)).ok()?;
    let pad = " ".repeat(indent);
    Some(s.lines().map(|l| format!("{pad}{l}")).collect())
}

/// Edita um bloco de mapeamento (as linhas dele, sem a chave-pai) e devolve as
/// linhas novas. `raw` e o que o texto diz, `canon` e o que a config conhecida
/// produzia antes, `new` e o que ela produz agora.
fn patch_block(
    lines: &[String],
    indent: usize,
    raw: &Mapping,
    canon: &Mapping,
    new: &Mapping,
) -> Option<Vec<String>> {
    let keys = keys_of(lines, indent)?;
    // Sanidade: o que achamos no texto tem de bater com o que o parser viu.
    if keys.len() != raw.len() || keys.iter().any(|(n, ..)| raw.get(n.as_str()).is_none()) {
        return None;
    }
    // (de, ate, novas linhas) sobre `lines`
    let mut ops: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut appended: Vec<String> = Vec::new();
    let find = |name: &str| keys.iter().find(|(n, ..)| n == name);

    for (k, nv) in new {
        let name = k.as_str()?;
        if canon.get(k) == Some(nv) {
            continue;
        }
        match find(name) {
            Some((_, kl, ke)) => {
                let mut op = None;
                if let (Some(Value::Mapping(cm)), Value::Mapping(nm), Some(Value::Mapping(rm))) =
                    (canon.get(k), nv, raw.get(k))
                    && lines[*kl].trim_end().ends_with(':')
                    && let Some(ci) = (*kl + 1..*ke).find_map(|i| indent_of(&lines[i]))
                    && ci > indent
                    && let Some(child) = patch_block(&lines[*kl + 1..*ke], ci, rm, cm, nm)
                {
                    op = Some((*kl + 1, *ke, child));
                }
                ops.push(op.map_or_else(|| render(k, nv, indent).map(|r| (*kl, *ke, r)), Some)?);
            }
            None => appended.extend(render(k, nv, indent)?),
        }
    }
    // Chaves conhecidas que sumiram da config nova: remover. Desconhecidas ficam.
    for (k, _) in canon {
        if new.get(k).is_none()
            && let Some((_, kl, ke)) = find(k.as_str()?)
        {
            ops.push((*kl, *ke, Vec::new()));
        }
    }
    let mut out = lines.to_vec();
    ops.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (de, ate, novas) in ops {
        out.splice(de..ate, novas);
    }
    if !appended.is_empty() {
        let mut at = out.len();
        while at > 0 && indent_of(&out[at - 1]).is_none() {
            at -= 1;
        }
        out.splice(at..at, appended);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use crate::loader::ConfigLoader;
    use std::fs;

    const ORIGINAL: &str = "# minha config\nzzz_desconhecida:\n  x: 1\n# gateway\ngateway:\n  host: 0.0.0.0 # aberto\n  port: 3888\nchannels:\n  telegram:\n    type: telegram\n    enabled: true\n    bot_token: abc\n  # comentario do discord\n  discord:\n    type: discord\n    enabled: false\nagent:\n  system_prompt: antigo\n";

    fn setup(label: &str) -> (ConfigLoader, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("garra-preserve-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join("config.yml"), ORIGINAL).expect("escreve");
        (ConfigLoader::with_dir(&dir), dir)
    }

    #[test]
    fn set_agent_system_prompt_preserva_o_resto_byte_a_byte() {
        let (loader, dir) = setup("prompt");
        assert!(loader.set_agent_system_prompt("novo").expect("grava"));
        let depois = fs::read_to_string(dir.join("config.yml")).expect("le");
        assert_eq!(depois, ORIGINAL.replace("antigo", "novo"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn set_channel_enabled_preserva_comentarios_ordem_e_desconhecidas() {
        let (loader, dir) = setup("canal");
        assert!(loader.set_channel_enabled("discord", true).expect("grava"));
        let depois = fs::read_to_string(dir.join("config.yml")).expect("le");
        let esperado = ORIGINAL.replace(
            "    type: discord\n    enabled: false",
            "    type: discord\n    enabled: true",
        );
        assert_eq!(depois, esperado);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn canal_novo_entra_sem_mexer_no_resto() {
        let (loader, dir) = setup("novo");
        assert!(loader.set_channel_enabled("slack", true).expect("grava"));
        let depois = fs::read_to_string(dir.join("config.yml")).expect("le");
        assert!(depois.starts_with("# minha config\nzzz_desconhecida:\n  x: 1\n# gateway\n"));
        assert!(depois.contains("# aberto") && depois.contains("# comentario do discord"));
        assert!(depois.contains("slack"));
        let cfg = loader.load_sem_env().expect("recarrega");
        assert_eq!(
            cfg.channels.get("slack").and_then(|c| c.enabled),
            Some(true)
        );
        assert!(cfg.channels.contains_key("telegram") && cfg.channels.contains_key("discord"));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn mantem_0600() {
        use std::os::unix::fs::PermissionsExt;
        let (loader, dir) = setup("modo");
        loader.set_agent_system_prompt("x").expect("grava");
        let modo = fs::metadata(dir.join("config.yml"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(modo & 0o777, 0o600);
        let _ = fs::remove_dir_all(dir);
    }
}

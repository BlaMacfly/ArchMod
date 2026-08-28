//! Parseur minimal mais tolérant du format KeyValues de Valve (`.vdf`, `.acf`).
//!
//! Il n'existe pas de spécification officielle : ce parseur suit le comportement
//! observé de Steam (clés/valeurs entre guillemets, tokens nus tolérés,
//! commentaires `//`, échappements `\"` `\\` `\n` `\t`, conditionnels `[$X]`
//! ignorés). Les recherches de clés sont insensibles à la casse car Steam
//! alterne entre `Valve`/`valve` ou `AppState`/`appstate` selon les versions.

use std::path::Path;

use crate::error::{Result, TuxError};

#[derive(Debug, Clone, PartialEq)]
pub enum VdfValue {
    Str(String),
    Obj(Vec<(String, VdfValue)>),
}

impl VdfValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            VdfValue::Str(s) => Some(s.as_str()),
            VdfValue::Obj(_) => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, VdfValue)]> {
        match self {
            VdfValue::Obj(entries) => Some(entries),
            VdfValue::Str(_) => None,
        }
    }

    /// Première valeur associée à `key` (comparaison insensible à la casse).
    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        self.as_obj()?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// Descend une suite de clés : `root.path(&["Software", "Valve"])`.
    pub fn path(&self, keys: &[&str]) -> Option<&VdfValue> {
        keys.iter().try_fold(self, |node, key| node.get(key))
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get_str(key)?.trim().parse().ok()
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        self.get_str(key)?.trim().parse().ok()
    }

    /// Itère sur les entrées d'un objet (vide si la valeur est une chaîne).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &VdfValue)> {
        self.as_obj()
            .unwrap_or(&[])
            .iter()
            .map(|(k, v)| (k.as_str(), v))
    }
}

/// Lit et parse un fichier KeyValues depuis le disque.
///
/// Les fichiers Steam ne sont pas toujours en UTF-8 strict (noms de jeux
/// exotiques) : on repasse donc en lossy plutôt que d'échouer.
pub fn parse_file(path: impl AsRef<Path>) -> Result<VdfValue> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| TuxError::io(path, e))?;
    let text = String::from_utf8_lossy(&bytes);
    parse(&text).map_err(|detail| TuxError::vdf(path, detail))
}

/// Parse un document KeyValues. Le document racine est un objet implicite.
pub fn parse(input: &str) -> std::result::Result<VdfValue, String> {
    let mut parser = Parser {
        chars: input.as_bytes(),
        pos: 0,
    };
    let entries = parser.parse_entries(0)?;
    parser.skip_trivia();
    if parser.pos < parser.chars.len() {
        return Err(format!(
            "caractère inattendu « {} » à l'offset {}",
            parser.chars[parser.pos] as char, parser.pos
        ));
    }
    Ok(VdfValue::Obj(entries))
}

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
}

const MAX_DEPTH: usize = 64;

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.chars.get(self.pos).copied()
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some(b'/') if self.chars.get(self.pos + 1) == Some(&b'/') => {
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                // Conditionnels de plateforme : [$WIN32], [!$LINUX], ...
                Some(b'[') => {
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b']' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn parse_entries(
        &mut self,
        depth: usize,
    ) -> std::result::Result<Vec<(String, VdfValue)>, String> {
        if depth > MAX_DEPTH {
            return Err("imbrication trop profonde".into());
        }
        let mut entries = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                None | Some(b'}') => return Ok(entries),
                _ => {}
            }
            let key = self.parse_token()?;
            self.skip_trivia();
            match self.peek() {
                Some(b'{') => {
                    self.pos += 1;
                    let children = self.parse_entries(depth + 1)?;
                    self.skip_trivia();
                    if self.peek() != Some(b'}') {
                        return Err(format!("accolade fermante manquante pour « {key} »"));
                    }
                    self.pos += 1;
                    entries.push((key, VdfValue::Obj(children)));
                }
                Some(_) => {
                    let value = self.parse_token()?;
                    entries.push((key, VdfValue::Str(value)));
                }
                None => return Err(format!("valeur manquante pour la clé « {key} »")),
            }
        }
    }

    fn parse_token(&mut self) -> std::result::Result<String, String> {
        match self.peek() {
            Some(b'"') => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    let c = self
                        .peek()
                        .ok_or_else(|| "guillemet fermant manquant".to_string())?;
                    self.pos += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let escaped = self
                                .peek()
                                .ok_or_else(|| "échappement en fin de fichier".to_string())?;
                            self.pos += 1;
                            out.push(match escaped {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                other => other,
                            });
                        }
                        other => out.push(other),
                    }
                }
                Ok(String::from_utf8_lossy(&out).into_owned())
            }
            Some(_) => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_ascii_whitespace() || c == b'{' || c == b'}' || c == b'"' {
                        break;
                    }
                    self.pos += 1;
                }
                if start == self.pos {
                    return Err(format!("token vide à l'offset {start}"));
                }
                Ok(String::from_utf8_lossy(&self.chars[start..self.pos]).into_owned())
            }
            None => Err("fin de fichier inattendue".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let src = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "/home/user/.local/share/Steam"
        "apps" { "440" "1234" }
    }
    // un commentaire
    "1"
    {
        "path"      "/mnt/games/SteamLibrary"
    }
}
"#;
        let root = parse(src).expect("parse ok");
        let folders = root.get("libraryfolders").expect("racine");
        let paths: Vec<_> = folders
            .entries()
            .filter_map(|(_, v)| v.get_str("path"))
            .collect();
        assert_eq!(
            paths,
            vec!["/home/user/.local/share/Steam", "/mnt/games/SteamLibrary"]
        );
    }

    #[test]
    fn parses_appmanifest_with_escapes_and_case() {
        let src = r#"
"AppState"
{
    "appid"     "292030"
    "name"      "The \"Witcher\" 3"
    "installdir"    "The Witcher 3"
    "SizeOnDisk"    "51000000000"
}
"#;
        let root = parse(src).expect("parse ok");
        let state = root.get("appstate").expect("insensible à la casse");
        assert_eq!(state.get_u32("AppID"), Some(292030));
        assert_eq!(state.get_str("name"), Some("The \"Witcher\" 3"));
        assert_eq!(state.get_u64("sizeondisk"), Some(51_000_000_000));
    }

    #[test]
    fn rejects_unbalanced_braces() {
        assert!(parse("\"a\" {\n \"b\" \"c\"\n").is_err());
    }

    #[test]
    fn tolerates_bare_tokens_and_conditionals() {
        let root = parse("key value\n\"other\" \"x\" [$LINUX]\n").expect("parse ok");
        assert_eq!(root.get_str("key"), Some("value"));
        assert_eq!(root.get_str("other"), Some("x"));
    }
}

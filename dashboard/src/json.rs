//! JSON minimal : écriture d'objets et lecture d'un objet plat (clés texte, valeurs texte ou nombre).

/// Échappe une chaîne pour JSON (sans les guillemets).
pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Constructeur d'objet JSON (ordre d'insertion).
#[derive(Default)]
pub struct Obj {
    parts: Vec<String>,
}

impl Obj {
    pub fn new() -> Obj {
        Obj::default()
    }
    pub fn raw(mut self, k: &str, raw: &str) -> Obj {
        self.parts.push(format!("\"{}\":{}", escape(k), raw));
        self
    }
    pub fn str(self, k: &str, v: &str) -> Obj {
        let raw = format!("\"{}\"", escape(v));
        self.raw(k, &raw)
    }
    pub fn int(self, k: &str, v: i64) -> Obj {
        self.raw(k, &v.to_string())
    }
    pub fn num(self, k: &str, v: f64) -> Obj {
        // JSON n'a ni NaN ni infini.
        let raw = if v.is_finite() {
            format!("{v:.3}")
        } else {
            "null".to_string()
        };
        self.raw(k, &raw)
    }
    pub fn bool(self, k: &str, v: bool) -> Obj {
        self.raw(k, if v { "true" } else { "false" })
    }
    pub fn build(self) -> String {
        format!("{{{}}}", self.parts.join(","))
    }
}

/// Tableau JSON à partir d'éléments déjà sérialisés.
pub fn array(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Str(String),
    Num(f64),
}

/// Lit un objet plat `{"k":"v","n":12}`. Refuse imbrication, tableaux, booléens, texte superflu.
pub fn parse_flat(s: &str) -> Result<Vec<(String, Val)>, String> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    skip_ws(&b, &mut i);
    expect(&b, &mut i, '{')?;
    skip_ws(&b, &mut i);
    if b.get(i) == Some(&'}') {
        i += 1;
    } else {
        loop {
            skip_ws(&b, &mut i);
            let k = parse_str(&b, &mut i)?;
            skip_ws(&b, &mut i);
            expect(&b, &mut i, ':')?;
            skip_ws(&b, &mut i);
            let v = if b.get(i) == Some(&'"') {
                Val::Str(parse_str(&b, &mut i)?)
            } else {
                let st = i;
                while i < b.len() && (b[i].is_ascii_digit() || "+-.eE".contains(b[i])) {
                    i += 1;
                }
                let t: String = b[st..i].iter().collect();
                Val::Num(
                    t.parse::<f64>()
                        .map_err(|_| "valeur invalide".to_string())?,
                )
            };
            out.push((k, v));
            skip_ws(&b, &mut i);
            match b.get(i) {
                Some(',') => i += 1,
                Some('}') => {
                    i += 1;
                    break;
                }
                _ => return Err("',' ou '}' attendu".into()),
            }
        }
    }
    skip_ws(&b, &mut i);
    if i != b.len() {
        return Err("texte après l'objet".into());
    }
    Ok(out)
}

pub fn get_str<'a>(o: &'a [(String, Val)], k: &str) -> Option<&'a str> {
    o.iter().find(|(kk, _)| kk == k).and_then(|(_, v)| match v {
        Val::Str(s) => Some(s.as_str()),
        _ => None,
    })
}

pub fn get_num(o: &[(String, Val)], k: &str) -> Option<f64> {
    o.iter().find(|(kk, _)| kk == k).and_then(|(_, v)| match v {
        Val::Num(n) => Some(*n),
        _ => None,
    })
}

fn skip_ws(b: &[char], i: &mut usize) {
    while *i < b.len() && b[*i].is_whitespace() {
        *i += 1;
    }
}

fn expect(b: &[char], i: &mut usize, c: char) -> Result<(), String> {
    if b.get(*i) == Some(&c) {
        *i += 1;
        Ok(())
    } else {
        Err(format!("'{c}' attendu"))
    }
}

fn parse_str(b: &[char], i: &mut usize) -> Result<String, String> {
    expect(b, i, '"')?;
    let mut s = String::new();
    loop {
        let c = *b.get(*i).ok_or("chaîne non terminée")?;
        *i += 1;
        match c {
            '"' => return Ok(s),
            '\\' => {
                let e = *b.get(*i).ok_or("échappement tronqué")?;
                *i += 1;
                s.push(match e {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    _ => return Err("échappement non géré".into()),
                });
            }
            c => s.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialise_objet() {
        let s = Obj::new()
            .str("mode", "Nominal")
            .int("t", 5)
            .num("x", 1.5)
            .bool("ok", true)
            .raw("a", &array(&["1".into(), "2".into()]))
            .build();
        assert_eq!(
            s,
            r#"{"mode":"Nominal","t":5,"x":1.500,"ok":true,"a":[1,2]}"#
        );
    }

    #[test]
    fn echappement_et_non_fini() {
        assert_eq!(escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
        assert_eq!(Obj::new().num("x", f64::NAN).build(), r#"{"x":null}"#);
    }

    #[test]
    fn lecture_objet_plat() {
        let o = parse_flat(r#" {"cmd":"SetMode", "arg": 1} "#).unwrap();
        assert_eq!(get_str(&o, "cmd"), Some("SetMode"));
        assert_eq!(get_num(&o, "arg"), Some(1.0));
        assert!(parse_flat("{}").unwrap().is_empty());
    }

    #[test]
    fn lecture_refuse_le_malforme() {
        for bad in ["", "{", "[1]", r#"{"a":}"#, r#"{"a":1} x"#, r#"{"a" 1}"#] {
            assert!(parse_flat(bad).is_err(), "{bad}");
        }
    }
}

//! Écriture JSON minimale à la main (pas de dépendance externe). L'ordre des clés est celui de
//! l'insertion : la sortie est déterministe.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn obj() -> Value {
        Value::Obj(Vec::new())
    }

    /// Ajoute une clé à un objet (panique si `self` n'en est pas un : erreur de programmation).
    pub fn with(mut self, key: &str, v: impl Into<Value>) -> Value {
        match &mut self {
            Value::Obj(o) => o.push((key.to_string(), v.into())),
            _ => panic!("Value::with sur un non-objet"),
        }
        self
    }

    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }

    fn write(&self, out: &mut String, ind: usize) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Uint(u) => out.push_str(&u.to_string()),
            Value::Num(x) => {
                if x.is_finite() {
                    out.push_str(&format!("{x}"));
                } else {
                    out.push_str("null");
                }
            }
            Value::Str(s) => escape(s, out),
            Value::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, v) in a.iter().enumerate() {
                    pad(out, ind + 1);
                    v.write(out, ind + 1);
                    if i + 1 < a.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, ind);
                out.push(']');
            }
            Value::Obj(o) => {
                if o.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    pad(out, ind + 1);
                    escape(k, out);
                    out.push_str(": ");
                    v.write(out, ind + 1);
                    if i + 1 < o.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, ind);
                out.push('}');
            }
        }
    }
}

fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}
impl From<u64> for Value {
    fn from(u: u64) -> Value {
        Value::Uint(u)
    }
}
impl From<usize> for Value {
    fn from(u: usize) -> Value {
        Value::Uint(u as u64)
    }
}
impl From<u32> for Value {
    fn from(u: u32) -> Value {
        Value::Uint(u as u64)
    }
}
impl From<f64> for Value {
    fn from(x: f64) -> Value {
        Value::Num(x)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s)
    }
}
impl From<Vec<Value>> for Value {
    fn from(a: Vec<Value>) -> Value {
        Value::Arr(a)
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(o: Option<T>) -> Value {
        o.map_or(Value::Null, Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_order() {
        let v = Value::obj()
            .with("b", 1u32)
            .with("a", "x\"y\n")
            .with("n", f64::NAN)
            .with("l", vec![Value::Bool(true), Value::Null]);
        let s = v.to_pretty();
        assert!(s.find("\"b\"").unwrap() < s.find("\"a\"").unwrap());
        assert!(s.contains("\"x\\\"y\\n\""));
        assert!(s.contains("\"n\": null"));
        assert!(s.contains("true,\n"));
    }
}

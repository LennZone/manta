//! A JSON/TOML/YAML value that keeps the order of object keys (scripts are
//! listed in file order, like the Python plugin's dicts did), with the
//! small accessors detection needs.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<V>),
    /// Keys in file order; a repeated key replaces the earlier value.
    Map(Vec<(String, V)>),
}

impl V {
    pub fn get(&self, key: &str) -> Option<&V> {
        match self {
            V::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `get(key)` if it is a string.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(V::str)
    }

    pub fn map(&self) -> Option<&[(String, V)]> {
        match self {
            V::Map(m) => Some(m),
            _ => None,
        }
    }

    /// `get(key)` if it is a map (Python: `isinstance(x.get(k), dict)`).
    pub fn get_map(&self, key: &str) -> Option<&V> {
        self.get(key).filter(|v| matches!(v, V::Map(_)))
    }

    pub fn list(&self) -> Option<&[V]> {
        match self {
            V::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn is_map(&self) -> bool {
        matches!(self, V::Map(_))
    }

    /// Python's `str()` of a scalar (for script bodies).
    pub fn text(&self) -> String {
        match self {
            V::Null => "None".into(),
            V::Bool(true) => "True".into(),
            V::Bool(false) => "False".into(),
            V::Int(i) => i.to_string(),
            V::Float(f) => f.to_string(),
            V::Str(s) => s.clone(),
            V::List(l) => format!(
                "[{}]",
                l.iter()
                    .map(|v| format!("'{}'", v.text()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            V::Map(_) => "{…}".into(),
        }
    }

    /// From a parsed YAML document.
    pub fn from_yaml(y: &yaml_rust2::Yaml) -> V {
        use yaml_rust2::Yaml;
        match y {
            Yaml::Real(s) => s
                .parse()
                .map(V::Float)
                .unwrap_or_else(|_| V::Str(s.clone())),
            Yaml::Integer(i) => V::Int(*i),
            Yaml::String(s) => V::Str(s.clone()),
            Yaml::Boolean(b) => V::Bool(*b),
            Yaml::Array(a) => V::List(a.iter().map(V::from_yaml).collect()),
            Yaml::Hash(h) => {
                let mut m: Vec<(String, V)> = Vec::new();
                for (k, v) in h {
                    let key = match k {
                        Yaml::String(s) | Yaml::Real(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => continue,
                    };
                    insert(&mut m, key, V::from_yaml(v));
                }
                V::Map(m)
            }
            _ => V::Null,
        }
    }
}

fn insert(m: &mut Vec<(String, V)>, k: String, v: V) {
    match m.iter_mut().find(|(x, _)| *x == k) {
        Some(slot) => slot.1 = v,
        None => m.push((k, v)),
    }
}

impl<'de> Deserialize<'de> for V {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<V, D::Error> {
        struct Vis;
        impl<'de> Visitor<'de> for Vis {
            type Value = V;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_bool<E>(self, b: bool) -> Result<V, E> {
                Ok(V::Bool(b))
            }
            fn visit_i64<E>(self, i: i64) -> Result<V, E> {
                Ok(V::Int(i))
            }
            fn visit_u64<E>(self, u: u64) -> Result<V, E> {
                Ok(i64::try_from(u).map(V::Int).unwrap_or(V::Float(u as f64)))
            }
            fn visit_f64<E>(self, f: f64) -> Result<V, E> {
                Ok(V::Float(f))
            }
            fn visit_str<E>(self, s: &str) -> Result<V, E> {
                Ok(V::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<V, E> {
                Ok(V::Str(s))
            }
            fn visit_none<E>(self) -> Result<V, E> {
                Ok(V::Null)
            }
            fn visit_unit<E>(self) -> Result<V, E> {
                Ok(V::Null)
            }
            fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<V, D2::Error> {
                V::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<V, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(V::List(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<V, A::Error> {
                let mut m = Vec::new();
                while let Some((k, v)) = a.next_entry::<String, V>()? {
                    insert(&mut m, k, v);
                }
                Ok(V::Map(m))
            }
        }
        d.deserialize_any(Vis)
    }
}

/// Parse JSON keeping key order.
pub fn json(text: &str) -> Result<V, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// Parse TOML keeping key order; errors as "line N: message".
pub fn toml(text: &str) -> Result<V, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    toml::from_str::<V>(text).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
        let msg = e.message().trim().to_string();
        match line {
            Some(l) => format!("line {l}: {msg}"),
            None => msg,
        }
    })
}

/// Parse the first YAML document.
pub fn yaml(text: &str) -> Result<V, String> {
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    Ok(docs.first().map(V::from_yaml).unwrap_or(V::Null))
}

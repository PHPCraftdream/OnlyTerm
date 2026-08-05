use crate::color::Palette;
use anyhow::Context;
use std::convert::TryInto;
use std::fs;
use std::path::Path;
use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

#[derive(Debug, Default, Clone, Eq, PartialEq, FromDynamic, ToDynamic)]
pub struct ColorSchemeMetaData {
    pub name: Option<String>,
    pub author: Option<String>,
    pub origin_url: Option<String>,
    pub wezterm_version: Option<String>,
    #[dynamic(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct ColorSchemeFile {
    /// The color palette
    pub colors: Palette,
    /// Info about the scheme
    #[dynamic(default)]
    pub metadata: ColorSchemeMetaData,
}

fn dynamic_to_toml(value: Value) -> anyhow::Result<toml::Value> {
    Ok(match value {
        Value::Null => anyhow::bail!("cannot map Null to toml"),
        Value::Bool(b) => toml::Value::Boolean(b),
        Value::String(s) => toml::Value::String(s),
        Value::Array(a) => {
            let mut arr = vec![];
            for v in a {
                arr.push(dynamic_to_toml(v)?);
            }
            toml::Value::Array(arr)
        }
        Value::Object(o) => {
            let mut map = toml::map::Map::new();
            for (k, v) in o {
                let k = match k {
                    Value::String(s) => s,
                    Value::U64(u) => u.to_string(),
                    Value::I64(u) => u.to_string(),
                    Value::F64(u) => u.to_string(),
                    _ => anyhow::bail!("toml keys must be strings {k:?}"),
                };
                let v = match v {
                    Value::Null => continue,
                    other => dynamic_to_toml(other)?,
                };
                map.insert(k, v);
            }
            toml::Value::Table(map)
        }
        Value::U64(i) => toml::Value::Integer(i.try_into()?),
        Value::I64(i) => toml::Value::Integer(i),
        Value::F64(f) => toml::Value::Float(*f),
    })
}

impl ColorSchemeFile {
    pub fn from_toml_value(value: &toml::Value) -> anyhow::Result<Self> {
        let scheme = Self::from_dynamic(&crate::toml_to_dynamic(value), Default::default())
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        anyhow::ensure!(
            scheme.colors.ansi.is_some(),
            "scheme is missing ANSI colors"
        );

        Ok(scheme)
    }

    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        let scheme: toml::Value = toml::from_str(s)?;
        Self::from_toml_value(&scheme)
    }

    pub fn to_toml_value(&self) -> anyhow::Result<toml::Value> {
        let value = self.to_dynamic();
        dynamic_to_toml(value)
    }

    pub fn from_json_value(value: &serde_json::Value) -> anyhow::Result<Self> {
        Self::from_dynamic(&crate::json_to_dynamic(value), Default::default())
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let value = self.to_toml_value()?;
        let text = toml::to_string_pretty(&value)?;
        fs::write(&path, text)
            .with_context(|| format!("writing toml to {}", path.as_ref().display()))
    }
}

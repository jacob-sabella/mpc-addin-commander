//! The text MPC shows for a stock plugin's saved value, from `assets/stock_params.toml`.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// How one parameter's 0 to 1 value reads.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    min: Option<f32>,
    max: Option<f32>,
    #[serde(default)]
    decimals: usize,
    unit: Option<String>,
    options: Option<Vec<String>>,
}

type Table = HashMap<String, HashMap<String, Spec>>;

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    toml::from_str(include_str!("../assets/stock_params.toml")).expect("stock_params.toml")
});

/// The text for `param` (its saved name, or `Parameter N`) of stock plugin `plugin` at `value`,
/// when the table knows it.
pub fn text(plugin: &str, param: &str, value: f32) -> Option<String> {
    let spec = TABLE.get(plugin)?.get(param)?;
    let v = value.clamp(0.0, 1.0);
    if let Some(options) = spec.options.as_ref().filter(|o| !o.is_empty()) {
        let i = (v * (options.len() - 1) as f32).round() as usize;
        return Some(options[i.min(options.len() - 1)].clone());
    }
    let (min, max) = (spec.min?, spec.max?);
    let n = min + v * (max - min);
    let mut s = format!("{n:.*}", spec.decimals);
    // Rounding a small negative number must not show "-0".
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    if let Some(unit) = &spec.unit {
        s.push(' ');
        s.push_str(unit);
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses() {
        assert!(!TABLE.is_empty());
        for (plugin, params) in TABLE.iter() {
            for (name, spec) in params {
                let numeric = spec.min.is_some() && spec.max.is_some();
                assert!(numeric || spec.options.is_some(), "{plugin} {name}");
            }
        }
    }

    #[test]
    fn bus_compressor_reads_as_the_device_showed() {
        let t = |p, v| text("Bus Compressor", p, v).unwrap();
        assert_eq!(t("Output", 0.1333), "-2");
        assert_eq!(t("Thresh", 0.84), "-8");
        assert_eq!(t("Ratio", 0.0), "1");
        assert_eq!(t("Oldskool", 0.0), "Off");
        assert_eq!(t("Oldskool", 1.0), "On");
        assert_eq!(t("DryWet", 1.0), "100");
        assert_eq!(t("Attack", 0.5), "50");
        assert_eq!(t("Output", 0.2), "0");
        assert_eq!(text("Bus Compressor", "Nope", 0.5), None);
        assert_eq!(text("Nope", "Output", 0.5), None);
    }
}

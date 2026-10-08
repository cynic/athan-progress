//! Configuration loading for athan-progress.
//!
//! The config file is JSON. It is searched for at (in order):
//!
//!   1. `--config <path>` on the command line
//!   2. `$ATHAN_PROGRESS_CONFIG`
//!   3. `$XDG_CONFIG_HOME/athan-progress/config.json`
//!   4. `~/.config/athan-progress/config.json`
//!
//! A missing file at the *default* locations means the built-in defaults are
//! used silently. An explicitly requested file that is missing or malformed
//! is an error, so typos do not silently fall back to defaults.

use std::path::{Path, PathBuf};

use salah::{Madhab, Method};
use serde::Deserialize;

/// Built-in default coordinates: Makhanda, South Africa.
pub const DEFAULT_LATITUDE: f64 = -33.2688;
pub const DEFAULT_LONGITUDE: f64 = 24.4356;

/// Fully resolved settings after merging file values over built-in defaults.
#[derive(Debug)]
pub struct Settings {
    pub latitude: f64,
    pub longitude: f64,
    pub method: Method,
    pub madhab: Madhab,
    pub tolerances: Tolerances,
    pub labels: Labels,
    pub bar_width: u8,
    /// Use Nerd Font glyphs (`FontAwesome` ●/○) for the bar instead of █/░.
    pub nerd_font: bool,
    /// Minutes after a tolerance deadline during which the module reports
    /// "EXPIRED" before switching to the upcoming-prayer countdown.
    pub expired_minutes: i64,
}

/// Per-prayer tolerance windows, in minutes.
#[derive(Debug)]
pub struct Tolerances {
    pub fajr: i64,
    pub dhuhr: i64,
    pub asr: i64,
    pub maghrib: i64,
    pub isha: i64,
}

/// Per-prayer display labels.
#[derive(Debug)]
pub struct Labels {
    pub fajr: String,
    pub dhuhr: String,
    pub asr: String,
    pub maghrib: String,
    pub isha: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            latitude: DEFAULT_LATITUDE,
            longitude: DEFAULT_LONGITUDE,
            method: Method::MuslimWorldLeague,
            madhab: Madhab::Shafi,
            tolerances: Tolerances {
                fajr: 45,
                dhuhr: 90,
                asr: 75,
                maghrib: 25,
                isha: 35,
            },
            // Nominative case, full tashkeel.
            labels: Labels {
                fajr: "الْفَجْرُ".to_string(),
                dhuhr: "الْظُهْرُ".to_string(),
                asr: "الْعَصْرُ".to_string(),
                maghrib: "الْمَغْرِبُ".to_string(),
                isha: "الْعِشَاءُ".to_string(),
            },
            bar_width: 10,
            nerd_font: false,
            expired_minutes: 10,
        }
    }
}

/// Raw file representation: every key optional, missing keys keep defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    latitude: Option<f64>,
    longitude: Option<f64>,
    method: Option<String>,
    madhab: Option<String>,
    tolerances: FileTolerances,
    labels: FileLabels,
    bar_width: Option<u8>,
    nerd_font: Option<bool>,
    expired_minutes: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileTolerances {
    fajr: Option<i64>,
    dhuhr: Option<i64>,
    asr: Option<i64>,
    maghrib: Option<i64>,
    isha: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileLabels {
    fajr: Option<String>,
    dhuhr: Option<String>,
    asr: Option<String>,
    maghrib: Option<String>,
    isha: Option<String>,
}

impl FileConfig {
    fn resolve(self) -> Result<Settings, String> {
        let mut s = Settings::default();

        if let Some(lat) = self.latitude {
            if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
                return Err(format!("latitude must be between -90 and 90, got {lat}"));
            }
            s.latitude = lat;
        }
        if let Some(lon) = self.longitude {
            if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
                return Err(format!("longitude must be between -180 and 180, got {lon}"));
            }
            s.longitude = lon;
        }
        if let Some(m) = self.method {
            s.method = parse_method(&m)?;
        }
        if let Some(m) = self.madhab {
            s.madhab = parse_madhab(&m)?;
        }

        let t = self.tolerances;
        s.tolerances.fajr = check_tolerance("fajr", t.fajr, s.tolerances.fajr)?;
        s.tolerances.dhuhr = check_tolerance("dhuhr", t.dhuhr, s.tolerances.dhuhr)?;
        s.tolerances.asr = check_tolerance("asr", t.asr, s.tolerances.asr)?;
        s.tolerances.maghrib = check_tolerance("maghrib", t.maghrib, s.tolerances.maghrib)?;
        s.tolerances.isha = check_tolerance("isha", t.isha, s.tolerances.isha)?;

        let l = self.labels;
        if let Some(v) = l.fajr {
            s.labels.fajr = v;
        }
        if let Some(v) = l.dhuhr {
            s.labels.dhuhr = v;
        }
        if let Some(v) = l.asr {
            s.labels.asr = v;
        }
        if let Some(v) = l.maghrib {
            s.labels.maghrib = v;
        }
        if let Some(v) = l.isha {
            s.labels.isha = v;
        }

        if let Some(w) = self.bar_width {
            if w == 0 {
                return Err("bar_width must be at least 1".to_string());
            }
            s.bar_width = w;
        }
        if let Some(n) = self.nerd_font {
            s.nerd_font = n;
        }
        if let Some(e) = self.expired_minutes {
            if e <= 0 {
                return Err(format!("expired_minutes must be positive, got {e}"));
            }
            s.expired_minutes = e;
        }

        Ok(s)
    }
}

fn check_tolerance(name: &str, value: Option<i64>, default: i64) -> Result<i64, String> {
    match value {
        None => Ok(default),
        Some(v) if v > 0 => Ok(v),
        Some(v) => Err(format!("tolerance for {name} must be positive, got {v}")),
    }
}

/// Load settings, honouring the search order documented at the top of this module.
pub fn load(explicit: Option<PathBuf>) -> Result<Settings, String> {
    if let Some(path) = explicit {
        return load_file(&path);
    }
    if let Some(path) = std::env::var_os("ATHAN_PROGRESS_CONFIG").map(PathBuf::from) {
        return load_file(&path);
    }
    if let Some(path) = default_config_path() {
        if path.is_file() {
            return load_file(&path);
        }
    }
    Ok(Settings::default())
}

fn load_file(path: &Path) -> Result<Settings, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let file: FileConfig = serde_json::from_str(&text)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    file.resolve()
}

fn default_config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("athan-progress").join("config.json"))
}

/// Normalise a method/madhab string: lowercase, ignoring spaces, dashes,
/// underscores and apostrophes (`"Muslim World League"` == `"muslim_world_league"`).
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '\''))
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_method(s: &str) -> Result<Method, String> {
    Ok(match normalize(s).as_str() {
        "dubai" => Method::Dubai,
        "egyptian" => Method::Egyptian,
        "karachi" => Method::Karachi,
        "kuwait" => Method::Kuwait,
        "moonsightingcommittee" => Method::MoonsightingCommittee,
        "muslimworldleague" | "mwl" => Method::MuslimWorldLeague,
        "northamerica" | "isna" => Method::NorthAmerica,
        "other" => Method::Other,
        "qatar" => Method::Qatar,
        "singapore" => Method::Singapore,
        "tehran" => Method::Tehran,
        "turkey" => Method::Turkey,
        "ummalqura" => Method::UmmAlQura,
        _ => {
            return Err(format!(
                "unknown method '{s}' (expected one of: Dubai, Egyptian, Karachi, Kuwait, \
                 MoonsightingCommittee, MuslimWorldLeague, NorthAmerica, Other, Qatar, \
                 Singapore, Tehran, Turkey, UmmAlQura)"
            ))
        }
    })
}

fn parse_madhab(s: &str) -> Result<Madhab, String> {
    Ok(match normalize(s).as_str() {
        "shafi" | "shafii" => Madhab::Shafi,
        "hanafi" => Madhab::Hanafi,
        _ => {
            return Err(format!(
                "unknown madhab '{s}' (expected Shafi or Hanafi)"
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_documented_values() {
        let s = Settings::default();
        assert_eq!(s.latitude, DEFAULT_LATITUDE);
        assert_eq!(s.longitude, DEFAULT_LONGITUDE);
        assert_eq!(s.tolerances.fajr, 45);
        assert_eq!(s.tolerances.dhuhr, 90);
        assert_eq!(s.tolerances.asr, 75);
        assert_eq!(s.tolerances.maghrib, 25);
        assert_eq!(s.tolerances.isha, 35);
        assert_eq!(s.labels.fajr, "الْفَجْرُ");
        assert_eq!(s.labels.dhuhr, "الْظُهْرُ");
        assert_eq!(s.labels.asr, "الْعَصْرُ");
        assert_eq!(s.labels.maghrib, "الْمَغْرِبُ");
        assert_eq!(s.labels.isha, "الْعِشَاءُ");
        assert_eq!(s.bar_width, 10);
        assert!(!s.nerd_font);
        assert_eq!(s.expired_minutes, 10);
    }

    #[test]
    fn partial_config_keeps_defaults_for_missing_keys() {
        let f: FileConfig =
            serde_json::from_str(r#"{ "tolerances": { "fajr": 20 } }"#).expect("valid json");
        let s = f.resolve().expect("valid config");
        assert_eq!(s.tolerances.fajr, 20);
        assert_eq!(s.tolerances.dhuhr, 90);
        assert_eq!(s.latitude, DEFAULT_LATITUDE);
        assert_eq!(s.bar_width, 10);
        assert_eq!(s.expired_minutes, 10);
    }

    #[test]
    fn nerd_font_and_expired_minutes_are_respected() {
        let f: FileConfig = serde_json::from_str(
            r#"{ "nerd_font": true, "expired_minutes": 25 }"#,
        )
        .expect("valid json");
        let s = f.resolve().expect("valid config");
        assert!(s.nerd_font);
        assert_eq!(s.expired_minutes, 25);
    }

    #[test]
    fn method_and_madhab_parsing_is_forgiving() {
        assert!(matches!(
            parse_method("Muslim World League"),
            Ok(Method::MuslimWorldLeague)
        ));
        assert!(matches!(parse_method("mwl"), Ok(Method::MuslimWorldLeague)));
        assert!(matches!(
            parse_method("umm_al_qura"),
            Ok(Method::UmmAlQura)
        ));
        assert!(matches!(parse_madhab("Hanafi"), Ok(Madhab::Hanafi)));
        assert!(matches!(parse_madhab("shafii"), Ok(Madhab::Shafi)));
        assert!(parse_method("Nonsense").is_err());
        assert!(parse_madhab("Maliki").is_err());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(serde_json::from_str::<FileConfig>(r#"{ "tollerances": {} }"#).is_err());
        assert!(serde_json::from_str::<FileConfig>(r#"{ "lattitude": 1.0 }"#).is_err());
    }

    #[test]
    fn validation_catches_bad_values() {
        // Rejected either at parse time or during resolution.
        let bad = |json: &str| {
            serde_json::from_str::<FileConfig>(json)
                .ok()
                .and_then(|f| f.resolve().ok())
                .is_none()
        };
        assert!(bad(r#"{ "latitude": 95 }"#));
        assert!(bad(r#"{ "latitude": 1e999 }"#));
        assert!(bad(r#"{ "longitude": -200 }"#));
        assert!(bad(r#"{ "tolerances": { "fajr": 0 } }"#));
        assert!(bad(r#"{ "tolerances": { "isha": -5 } }"#));
        assert!(bad(r#"{ "method": "Nonsense" }"#));
        assert!(bad(r#"{ "madhab": "Maliki" }"#));
        assert!(bad(r#"{ "bar_width": 0 }"#));
        assert!(bad(r#"{ "expired_minutes": 0 }"#));
        assert!(bad(r#"{ "expired_minutes": -3 }"#));
        assert!(bad(r#"{ "nerd_font": "yes" }"#));
    }
}

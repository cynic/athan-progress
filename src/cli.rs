//! Command-line interface.
//!
//! Every configuration option that can come from the config file can also be
//! given as a flag; flags take precedence over the file, which takes
//! precedence over the built-in defaults.

use std::path::PathBuf;

use clap::Parser;

use crate::config::FileConfig;

#[derive(Parser, Debug)]
#[command(
    name = "athan-progress",
    about = "Waybar progress bar for the window following each athan",
    version
)]
pub struct Cli {
    /// Path to a JSON config file (default: XDG config location)
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Latitude in degrees (overrides config file)
    #[arg(long, allow_hyphen_values = true)]
    pub latitude: Option<f64>,

    /// Longitude in degrees (overrides config file)
    #[arg(long, allow_hyphen_values = true)]
    pub longitude: Option<f64>,

    /// Calculation method, e.g. `MuslimWorldLeague` or mwl (overrides config file)
    #[arg(long)]
    pub method: Option<String>,

    /// Madhab: Shafi or Hanafi (overrides config file)
    #[arg(long)]
    pub madhab: Option<String>,

    /// Per-prayer tolerance in minutes; repeatable, e.g. `--tolerance fajr=45`
    #[arg(long = "tolerance", value_name = "PRAYER=MINUTES")]
    pub tolerances: Vec<String>,

    /// Per-prayer display label; repeatable, e.g. `--label fajr=الفجر`
    #[arg(long = "label", value_name = "PRAYER=TEXT")]
    pub labels: Vec<String>,

    /// Number of bar characters (overrides config file)
    #[arg(long)]
    pub bar_width: Option<u8>,

    /// Use Nerd Font circle glyphs for the bar
    #[arg(long, overrides_with = "no_nerd_font")]
    pub nerd_font: bool,

    /// Use plain block characters for the bar
    #[arg(long = "no-nerd-font", overrides_with = "nerd_font")]
    pub no_nerd_font: bool,

    /// Minutes to report EXPIRED after a window closes (overrides config file)
    #[arg(long)]
    pub expired_minutes: Option<i64>,
}

impl Cli {
    /// Convert the flags into a [`FileConfig`] overlay: only flags actually
    /// given produce `Some(..)`, so everything else falls through to the
    /// config file and then the built-in defaults.
    pub fn to_file_config(&self) -> Result<FileConfig, String> {
        let nerd_font = if self.nerd_font {
            Some(true)
        } else if self.no_nerd_font {
            Some(false)
        } else {
            None
        };
        let mut over = FileConfig {
            latitude: self.latitude,
            longitude: self.longitude,
            method: self.method.clone(),
            madhab: self.madhab.clone(),
            bar_width: self.bar_width,
            expired_minutes: self.expired_minutes,
            nerd_font,
            ..Default::default()
        };
        for entry in &self.tolerances {
            let (prayer, minutes) = split_entry(entry, "--tolerance")?;
            let minutes: i64 = minutes
                .parse()
                .map_err(|_| format!("--tolerance: '{minutes}' is not a number of minutes"))?;
            over.tolerances.set(prayer, minutes)?;
        }
        for entry in &self.labels {
            let (prayer, label) = split_entry(entry, "--label")?;
            over.labels.set(prayer, label.to_string())?;
        }
        Ok(over)
    }
}

fn split_entry<'a>(entry: &'a str, flag: &str) -> Result<(&'a str, &'a str), String> {
    entry
        .split_once('=')
        .ok_or_else(|| format!("{flag} expects PRAYER=VALUE, got '{entry}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("athan-progress").chain(args.iter().copied()))
    }

    #[test]
    fn parses_tolerance_and_label_entries() {
        let cli = parse(&[
            "--tolerance",
            "fajr=45",
            "--tolerance",
            "zuhr=120",
            "--label",
            "fajr=الفجر",
        ])
        .expect("parses");
        let over = cli.to_file_config().expect("converts");
        assert_eq!(over.tolerances.fajr, Some(45));
        assert_eq!(over.tolerances.dhuhr, Some(120)); // zuhr alias
        assert_eq!(over.labels.fajr.as_deref(), Some("الفجر"));
        // Untouched prayers stay None so the file/defaults survive.
        assert_eq!(over.tolerances.asr, None);
    }

    #[test]
    fn nerd_font_is_tristate() {
        let none = parse(&[]).expect("parses").to_file_config().expect("converts");
        assert_eq!(none.nerd_font, None);
        let on = parse(&["--nerd-font"])
            .expect("parses")
            .to_file_config()
            .expect("converts");
        assert_eq!(on.nerd_font, Some(true));
        let off = parse(&["--no-nerd-font"])
            .expect("parses")
            .to_file_config()
            .expect("converts");
        assert_eq!(off.nerd_font, Some(false));
        // Last one wins.
        let both = parse(&["--nerd-font", "--no-nerd-font"])
            .expect("parses")
            .to_file_config()
            .expect("converts");
        assert_eq!(both.nerd_font, Some(false));
    }

    #[test]
    fn negative_longitudes_parse() {
        let cli = parse(&["--longitude", "-118.24"]).expect("parses");
        let over = cli.to_file_config().expect("converts");
        assert_eq!(over.longitude, Some(-118.24));
    }

    #[test]
    fn malformed_entries_are_errors() {
        // Missing '='.
        let err = parse(&["--tolerance", "fajr"])
            .expect("parses")
            .to_file_config()
            .expect_err("rejects");
        assert!(err.contains("--tolerance expects PRAYER=VALUE"), "{err}");
        // Non-numeric minutes.
        let err = parse(&["--tolerance", "fajr=soon"])
            .expect("parses")
            .to_file_config()
            .expect_err("rejects");
        assert!(err.contains("not a number of minutes"), "{err}");
        // Unknown prayer name.
        let err = parse(&["--label", "sunrise=X"])
            .expect("parses")
            .to_file_config()
            .expect_err("rejects");
        assert!(err.contains("unknown prayer"), "{err}");
    }

    #[test]
    fn unknown_flags_fail_with_usage() {
        assert!(parse(&["--latitute", "1"]).is_err());
    }
}

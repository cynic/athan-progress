# athan-progress

A [Waybar](https://github.com/Alexays/Waybar) module for Hyprland (or any
bar that speaks Waybar's custom-module protocol): a progress bar showing how
much of the allowed window after the most recent athan has elapsed, with a
different tolerance for each salaat.

```
الْفَجْرُ ██████░░░░ 62%
```

The module has three states, driven by the most recent athan and its
tolerance window:

1. **Active** — the bar fills as the tolerance window elapses
   (`normal` → `warning` → `critical`).
2. **Expired** — for `expired_minutes` (default 10) after the window closes,
   the text reads `الْظُهْرُ EXPIRED` with class `expired`.
3. **Upcoming** — thereafter the bar counts down to the *next* athan: the
   next prayer's label, a bar filling to the athan, and the minutes
   remaining (e.g. `الْعَصْرُ █████░ 47m`), class `upcoming`.

This means pre-dawn shows a countdown to today's Fajr (via yesterday's long-
closed Isha window), and after Isha it counts down to tomorrow's Fajr.

## How it works

For the current *local* date, prayer times are calculated with the
[`salah`](https://crates.io/crates/salah) crate (Adhan-based) for your
configured coordinates, for yesterday, today and tomorrow. The most recent
prayer whose athan has passed is the *anchor*; its tolerance window decides
the active/expired phases, and the next prayer time drives the countdown.

Default tolerances (minutes after the athan):

| Salaat  | Tolerance |
|---------|-----------|
| Fajr    | 45        |
| Zuhr    | 90        |
| Asr     | 75        |
| Maghrib | 25        |
| Isha    | 35        |

Bar colour classes (style them in your Waybar CSS):

| Class      | Meaning                                        |
|------------|------------------------------------------------|
| `normal`   | window open, less than 50% elapsed             |
| `warning`  | window open, 50–85% elapsed                    |
| `critical` | window open, more than 85% elapsed             |
| `expired`  | within `expired_minutes` of the window closing |
| `upcoming` | counting down to the next athan                |

## Install

```sh
cargo build --release
cp target/release/athan-progress ~/.local/bin/
mkdir -p ~/.config/athan-progress
cp config.example.json ~/.config/athan-progress/config.json
```

Then edit `~/.config/athan-progress/config.json` to your coordinates and
preferences (see below). Every key is optional — delete the file entirely and
the built-in defaults apply (Makhanda, South Africa; Muslim World League;
Shafi madhab; tolerances as in the table above).

## Waybar setup

Merge `waybar-config-snippet.json` into your Waybar config, adjusting `exec`
if your binary lives elsewhere:

```json
{
  "custom/athan": {
    "exec": "~/.local/bin/athan-progress",
    "return-type": "json",
    "format": "{}",
    "interval": 60,
    "tooltip": true
  }
}
```

Add colours to your Waybar CSS (colours below are Catppuccin Mocha):

```css
#custom-athan.normal  { color: #a6e3a1; }
#custom-athan.warning { color: #f9e2af; }
#custom-athan.critical { color: #f38ba8; }
#custom-athan.expired { color: #f38ba8; }
#custom-athan.upcoming { color: #89b4fa; }
```

The default labels are Arabic with full tashkeel, so your Waybar font stack
needs an Arabic-capable font. Either install one (e.g. `Noto Naskh Arabic`)
or append a fallback in your Waybar/CSS font declaration, e.g.
`font-family: "JetBrains Mono", "Noto Naskh Arabic";`.

With `"nerd_font": true` the bar uses FontAwesome circles `●`/`○` from the
Nerd Font private-use area — make sure a Nerd Font is actually in your
Waybar font stack, otherwise those glyphs render as boxes. The Arabic font
advice above applies identically.

## Configuration reference

`~/.config/athan-progress/config.json` (override the path with
`--config <path>` or the `ATHAN_PROGRESS_CONFIG` environment variable):

```json
{
  "latitude": -33.2688,
  "longitude": 24.4356,
  "method": "MuslimWorldLeague",
  "madhab": "Shafi",
  "tolerances": {
    "fajr": 45,
    "dhuhr": 90,
    "asr": 75,
    "maghrib": 25,
    "isha": 35
  },
  "labels": {
    "fajr": "الْفَجْرُ",
    "dhuhr": "الْظُهْرُ",
    "asr": "الْعَصْرُ",
    "maghrib": "الْمَغْرِبُ",
    "isha": "الْعِشَاءُ"
  },
  "bar_width": 10,
  "nerd_font": false,
  "expired_minutes": 10
}
```

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `latitude` | number | `-33.2688` | −90…90 |
| `longitude` | number | `24.4356` | −180…180 |
| `method` | string | `"MuslimWorldLeague"` | One of: `Dubai`, `Egyptian`, `Karachi`, `Kuwait`, `MoonsightingCommittee`, `MuslimWorldLeague`, `NorthAmerica`, `Other`, `Qatar`, `Singapore`, `Tehran`, `Turkey`, `UmmAlQura` |
| `madhab` | string | `"Shafi"` | `Shafi` or `Hanafi` (affects Asr) |
| `tolerances.<prayer>` | integer (minutes) | see table | Must be positive |
| `expired_minutes` | integer (minutes) | `10` | Grace period reporting `EXPIRED`; must be positive |
| `labels.<prayer>` | string | Arabic with tashkeel | Free-form; single words keep parsing robust |
| `bar_width` | integer | `10` | Bar characters; at least 1 |
| `nerd_font` | boolean | `false` | `true` uses Nerd Font circles `●`/`○` (FontAwesome U+F111/U+F10C) instead of `█`/`░` |

Unknown keys are rejected with an error rather than silently ignored, so
typos surface immediately. Method and madhab matching is case-insensitive and
ignores spaces/dashes/underscores (`"Muslim World League"` works).

## Command-line options

Every configuration option can also be given as a flag; **precedence is
flags > config file > built-in defaults**. Run `athan-progress --help` for
the full list.

```sh
# One-off location and method, ignoring the config file's values:
athan-progress --latitude 21.4225 --longitude 39.8262 --method umm-al-qura

# Temporary tolerance/label experiments (repeatable; zuhr and dhuhr are
# accepted for the same prayer):
athan-progress --tolerance fajr=20 --tolerance isha=60 --label fajr=الفجر

# Bar appearance and EXPIRED duration:
athan-progress --bar-width 12 --nerd-font --expired-minutes 15
```

Notes:

- `--tolerance PRAYER=MINUTES` and `--label PRAYER=TEXT` accept `fajr`,
  `zuhr`/`dhuhr`, `asr`, `maghrib`, `isha`/`esha` (case-insensitive).
- `--nerd-font` / `--no-nerd-font` force the bar style either way; if
  neither is given, the config file's `nerd_font` applies.
- Unknown flags exit with code 2 and a usage hint (courtesy of clap);
  invalid values (bad prayer names, non-numeric minutes, out-of-range
  coordinates) exit with code 1 and a specific message.
- The binary is built with `lto = true` and `codegen-units = 1` (see
  `[profile.release]`) to keep the added dependency cost small.

## Development

The repo ships a [devenv](https://devenv.sh) environment (`devenv shell`)
with the Rust toolchain. `cargo build --release`, `cargo run` (prints one
JSON line), and pipe through `jq` to inspect the output:

```sh
cargo run | jq .
```

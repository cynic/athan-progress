//! athan-progress — Waybar progress bar for the window following each athan.
//!
//! Prints a single line of Waybar JSON on stdout:
//!   {"text": "...", "tooltip": "...", "class": "normal|warning|critical|expired|upcoming"}
//!
//! Exit code is non-zero on any error (Waybar then shows the module as failed;
//! a wrapper can substitute a fallback glyph).

mod config;

use std::path::PathBuf;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use salah::{Configuration, Coordinates, Prayer, PrayerSchedule};
use serde::Serialize;

use config::Settings;

/// Invisible bidi control characters (UAX #9). Waybar/Pango renders text with
/// the Unicode bidirectional algorithm, whose paragraph direction is taken from
/// the first *strong* character. Our text starts with Arabic (RTL), which would
/// mirror the whole line — so we force an LTR paragraph with a leading LRM and
/// isolate each segment's directionality explicitly.
const LRM: char = '\u{200E}'; // LEFT-TO-RIGHT MARK: forces LTR paragraph direction
const LRI: char = '\u{2066}'; // LEFT-TO-RIGHT ISOLATE
const PDI: char = '\u{2069}'; // POP DIRECTIONAL ISOLATE

/// Wrap a segment so it renders exactly in its written order, left to right.
/// The leading LRM forces an LTR paragraph; the LRI isolate keeps the
/// segment's internal directionality self-contained, so parentheses in
/// "(Zuhr)" or a trailing "!" can never mirror. Arabic inside still renders
/// correctly right-to-left as a unit. All controls are zero-width/invisible.
fn ltr_segment(seg: &str) -> String {
    format!("{LRM}{LRI}{seg}{PDI}")
}

/// Join segments (each from [`ltr_segment`]) with spaces.
fn join_segments(segments: &[String]) -> String {
    segments.join(" ")
}

#[derive(Debug)]
struct Entry {
    /// Display label (Arabic with tashkeel by default).
    name: String,
    /// English name, used in the tooltip.
    name_en: &'static str,
    time: DateTime<Utc>,
    tolerance_min: i64,
}

fn calculate(date: NaiveDate, settings: &Settings) -> Result<Vec<Entry>, String> {
    let coords = Coordinates::new(settings.latitude, settings.longitude);
    let params = Configuration::with(settings.method, settings.madhab);

    let prayers = PrayerSchedule::new()
        .on(date)
        .for_location(coords)
        .with_configuration(params)
        .calculate()
        .map_err(|e| format!("prayer time calculation failed: {e}"))?;

    Ok(vec![
        Entry {
            name: settings.labels.fajr.clone(),
            name_en: "Fajr",
            time: prayers.time(Prayer::Fajr),
            tolerance_min: settings.tolerances.fajr,
        },
        Entry {
            name: settings.labels.dhuhr.clone(),
            name_en: "Zuhr",
            time: prayers.time(Prayer::Dhuhr),
            tolerance_min: settings.tolerances.dhuhr,
        },
        Entry {
            name: settings.labels.asr.clone(),
            name_en: "Asr",
            time: prayers.time(Prayer::Asr),
            tolerance_min: settings.tolerances.asr,
        },
        Entry {
            name: settings.labels.maghrib.clone(),
            name_en: "Maghrib",
            time: prayers.time(Prayer::Maghrib),
            tolerance_min: settings.tolerances.maghrib,
        },
        Entry {
            name: settings.labels.isha.clone(),
            name_en: "Isha",
            time: prayers.time(Prayer::Isha),
            tolerance_min: settings.tolerances.isha,
        },
    ])
}

/// What the bar should show at a given moment.
#[derive(Debug)]
enum State<'a> {
    /// The window after the most recent athan is still open.
    Active {
        anchor: &'a Entry,
        percent: u32,
        class: &'static str,
    },
    /// Within the grace period after a window closed.
    Expired {
        anchor: &'a Entry,
        next: &'a Entry,
    },
    /// Counting down to the next athan.
    Upcoming {
        next: &'a Entry,
        percent: u32,
        minutes: i64,
    },
}

/// Decide what the bar should show at `now`, given three consecutive days of
/// prayer entries (yesterday is needed before Fajr, tomorrow after Isha).
///
/// * `now <= deadline` (anchor time + tolerance) → [`State::Active`]
/// * `now <= deadline + expired_min` → [`State::Expired`]
/// * otherwise → [`State::Upcoming`], with the bar measuring progress from
///   the end of the grace period to the next athan.
///
/// Returns `None` only if no prayer has passed yet (needs a clock earlier
/// than yesterday's Isha, i.e. practically never).
fn compute_state<'a>(
    now: DateTime<Utc>,
    yesterday: &'a [Entry],
    today: &'a [Entry],
    tomorrow: &'a [Entry],
    expired_min: i64,
) -> Option<State<'a>> {
    // Anchor: the most recent athan that has passed — today's, or yesterday's
    // Isha before today's Fajr.
    let anchor = today
        .iter()
        .rev()
        .find(|e| e.time <= now)
        .or_else(|| yesterday.last().filter(|e| e.time <= now))?;

    // Next athan: later today, else tomorrow's Fajr.
    let next = today
        .iter()
        .find(|e| e.time > now)
        .or_else(|| tomorrow.first())?;

    let tolerance_sec = anchor.tolerance_min * 60;
    let deadline = anchor.time + Duration::seconds(tolerance_sec);

    if now <= deadline {
        let elapsed = (now - anchor.time).num_seconds().max(0);
        // Seconds-level values are tiny compared to f64's 52-bit mantissa,
        // so these casts are exact for any realistic tolerance window.
        #[allow(clippy::cast_precision_loss)]
        let ratio = (elapsed as f64 / tolerance_sec as f64).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let percent = (ratio * 100.0) as u32;
        let class = if ratio < 0.5 {
            "normal"
        } else if ratio < 0.85 {
            "warning"
        } else {
            "critical"
        };
        Some(State::Active {
            anchor,
            percent,
            class,
        })
    } else {
        let gap_start = deadline + Duration::seconds(expired_min * 60);
        if now <= gap_start {
            Some(State::Expired { anchor, next })
        } else {
            let total = (next.time - gap_start).num_seconds().max(1);
            let done = (now - gap_start).num_seconds().clamp(0, total);
            let percent = u32::try_from((done * 100) / total).unwrap_or(100);
            let minutes = ceil_min(next.time - now);
            Some(State::Upcoming {
                next,
                percent,
                minutes,
            })
        }
    }
}

/// Ceiling of a duration to whole minutes.
fn ceil_min(d: Duration) -> i64 {
    (d.num_seconds().max(0) + 59) / 60
}

/// Bar glyphs: Nerd Font (`FontAwesome`) circles, or plain block elements.
fn bar_chars(nerd_font: bool) -> (char, char) {
    if nerd_font {
        ('\u{F111}', '\u{F10C}') // ● ○
    } else {
        ('█', '░')
    }
}

fn draw_bar(percent: u32, width: u8, filled: char, empty: char) -> String {
    let width = u32::from(width.max(1));
    let filled_count = (percent.min(100) * width + 50) / 100;
    let mut bar = String::with_capacity(width as usize * filled.len_utf8());
    for i in 0..width {
        bar.push(if i < filled_count { filled } else { empty });
    }
    bar
}

#[derive(Serialize)]
struct WaybarOutput {
    text: String,
    tooltip: String,
    class: &'static str,
}

fn run() -> Result<(), String> {
    let mut explicit: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| "missing path after --config".to_string())?;
                explicit = Some(PathBuf::from(path));
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument '{other}' (try --help)")),
        }
    }

    let settings = config::load(explicit)?;

    // Select the schedule by *local* date: near midnight (and in far-east
    // timezones) the UTC date can differ from the date the user is living.
    let now_local = Local::now();
    let date = NaiveDate::from_ymd_opt(now_local.year(), now_local.month(), now_local.day())
        .ok_or_else(|| "could not construct today's date".to_string())?;

    let now_utc = Utc::now();
    let yesterday = calculate(date - Duration::days(1), &settings)?;
    let today = calculate(date, &settings)?;
    let tomorrow = calculate(date + Duration::days(1), &settings)?;

    let state = compute_state(now_utc, &yesterday, &today, &tomorrow, settings.expired_minutes)
        .ok_or_else(|| "no passed prayer found to anchor on".to_string())?;

    let (filled, empty) = bar_chars(settings.nerd_font);

    let (text, class, tooltip) = match &state {
        State::Active {
            anchor,
            percent,
            class,
        } => {
            let bar = draw_bar(*percent, settings.bar_width, filled, empty);
            let text = join_segments(&[
                ltr_segment(&anchor.name),
                ltr_segment(&format!("{bar} {percent}%")),
            ]);
            let tol_sec = anchor.tolerance_min * 60;
            let remaining_min =
                ((tol_sec - (now_utc - anchor.time).num_seconds()).max(0) + 59) / 60;
            let tooltip = join_lines(&[
                tooltip_title(anchor),
                ltr_segment(&format!(
                    "Athān: {}",
                    anchor.time.with_timezone(&Local).format("%H:%M")
                )),
                ltr_segment(&format!("Tolerance: {} min", anchor.tolerance_min)),
                ltr_segment(&format!("Remaining: {remaining_min} min")),
            ]);
            (text, *class, tooltip)
        }
        State::Expired { anchor, next } => {
            let text = join_segments(&[ltr_segment(&anchor.name), ltr_segment("EXPIRED")]);
            let deadline = anchor.time + Duration::seconds(anchor.tolerance_min * 60);
            let until_next = ceil_min(next.time - now_utc);
            let tooltip = join_lines(&[
                tooltip_title(anchor),
                ltr_segment(&format!(
                    "Window closed at {}",
                    deadline.with_timezone(&Local).format("%H:%M")
                )),
                ltr_segment(&format!("Next: {} — in {until_next} min", next.name)),
            ]);
            (text, "expired", tooltip)
        }
        State::Upcoming {
            next,
            percent,
            minutes,
        } => {
            let bar = draw_bar(*percent, settings.bar_width, filled, empty);
            let text = join_segments(&[
                ltr_segment(&next.name),
                ltr_segment(&format!("{bar} {minutes}m")),
            ]);
            let tooltip = join_lines(&[
                tooltip_title(next),
                ltr_segment(&format!(
                    "Athān at: {}",
                    next.time.with_timezone(&Local).format("%H:%M")
                )),
                ltr_segment(&format!("In: {minutes} min")),
            ]);
            (text, "upcoming", tooltip)
        }
    };

    let out = WaybarOutput {
        text,
        tooltip,
        class,
    };
    println!(
        "{}",
        serde_json::to_string(&out).map_err(|e| e.to_string())?
    );
    Ok(())
}

/// Join pre-built lines (each bidi-isolated via [`ltr_segment`]) with newlines.
fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Tooltip title line: `الْظُهْرُ (Zuhr)`.
fn tooltip_title(e: &Entry) -> String {
    join_segments(&[
        ltr_segment(&e.name),
        ltr_segment(&format!("({})", e.name_en)),
    ])
}

fn print_help() {
    println!(
        "athan-progress — Waybar progress bar for the window following each athan\n\
         \n\
         Usage: athan-progress [--config <path>]\n\
         \n\
         Options:\n\
         -c, --config <path>  Use this JSON config file\n\
         -h, --help           Print this help\n\
         \n\
         Config search order:\n\
         1. --config <path>\n\
         2. $ATHAN_PROGRESS_CONFIG\n\
         3. $XDG_CONFIG_HOME/athan-progress/config.json\n\
         4. ~/.config/athan-progress/config.json\n\
         \n\
         Output: one line of Waybar JSON (text, tooltip, class) on stdout."
    );
}

fn main() {
    if let Err(e) = run() {
        eprintln!("athan-progress: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salah::{Madhab, Method};

    fn makkah(method: Method, madhab: Madhab) -> Settings {
        Settings {
            latitude: 21.4225,
            longitude: 39.8262,
            method,
            madhab,
            ..Settings::default()
        }
    }

    fn test_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).expect("valid date")
    }

    #[test]
    fn method_changes_fajr_time() {
        let mwl = calculate(test_date(), &makkah(Method::MuslimWorldLeague, Madhab::Shafi))
            .expect("calculation");
        let isna =
            calculate(test_date(), &makkah(Method::NorthAmerica, Madhab::Shafi)).expect("calculation");
        // ISNA uses a 15° twilight angle, so its Fajr must be later than MWL's 18°.
        assert!(isna[0].time > mwl[0].time, "method had no effect on Fajr");
    }

    #[test]
    fn madhab_changes_asr_time() {
        let shafi = calculate(test_date(), &makkah(Method::MuslimWorldLeague, Madhab::Shafi))
            .expect("calculation");
        let hanafi = calculate(test_date(), &makkah(Method::MuslimWorldLeague, Madhab::Hanafi))
            .expect("calculation");
        // Hanafi Asr (shadow factor 2) is later than Shafi's (factor 1).
        assert!(hanafi[2].time > shafi[2].time, "madhab had no effect on Asr");
    }

    #[test]
    fn entries_carry_configured_labels_and_tolerances() {
        let mut s = Settings::default();
        s.labels.fajr = "TEST".to_string();
        s.tolerances.fajr = 11;
        let entries = calculate(test_date(), &s).expect("calculation");
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].name, "TEST");
        assert_eq!(entries[0].tolerance_min, 11);
        assert_eq!(entries[4].name, "الْعِشَاءُ");
        assert_eq!(entries[4].tolerance_min, 35);
    }

    #[test]
    fn draw_bar_shapes() {
        assert_eq!(draw_bar(0, 10, '█', '░'), "░░░░░░░░░░");
        assert_eq!(draw_bar(50, 10, '█', '░'), "█████░░░░░");
        assert_eq!(draw_bar(100, 10, '█', '░'), "██████████");
        assert_eq!(draw_bar(42, 5, '█', '░'), "██░░░");
    }

    #[test]
    fn nerd_font_bar_uses_circles() {
        assert_eq!(bar_chars(true), ('\u{F111}', '\u{F10C}'));
        assert_eq!(bar_chars(false), ('█', '░'));
        assert_eq!(draw_bar(50, 4, '\u{F111}', '\u{F10C}'), "\u{F111}\u{F111}\u{F10C}\u{F10C}");
        assert_eq!(draw_bar(0, 4, '\u{F111}', '\u{F10C}'), "\u{F10C}\u{F10C}\u{F10C}\u{F10C}");
    }
}

/// Verification that the bidi controls produce the intended visual order.
/// Uses the `unicode-bidi` crate, which implements UAX #9 — the same
/// algorithm Pango/FriBidi applies when Waybar renders the text.
#[cfg(test)]
mod bidi_tests {
    use super::*;
    use unicode_bidi::{BidiInfo, Level};

    /// Visual order as the renderer would display the line left to right,
    /// with the invisible control characters stripped.
    fn visual_order(text: &str) -> String {
        let bidi = BidiInfo::new(text, Some(Level::ltr()));
        let para = bidi.paragraphs.first().expect("at least one paragraph");
        let visual = bidi.reorder_line(para, para.range.clone()).to_string();
        visual
            .chars()
            .filter(|&c| !matches!(c, LRM | LRI | PDI))
            .collect()
    }

    /// How an Arabic label correctly renders on its own: right-to-left,
    /// i.e. character-reversed compared to the logical string. Whatever we
    /// compose, the label must appear exactly like this at the left end.
    fn rtl_rendered(label: &str) -> String {
        visual_order(label)
    }

    fn compose(label: &str, tail: &str) -> String {
        join_segments(&[ltr_segment(label), ltr_segment(tail)])
    }

    #[test]
    fn composed_output_displays_in_written_order() {
        // What the binary emits for the default label at 89%: the label
        // renders RTL (as Arabic must), then the bar, then "89%" — in
        // written order, left to right.
        let bar = draw_bar(89, 10, '█', '░');
        let visual = visual_order(&compose("الْظُهْرُ", &format!("{bar} 89%")));
        assert_eq!(visual, format!("{} {bar} 89%", rtl_rendered("الْظُهْرُ")));
        assert!(visual.ends_with("89%"), "percentage intact and rightmost");
    }

    #[test]
    fn latin_segments_render_ltr_with_parens_intact() {
        // Tooltip title: "(Zuhr)" must neither mirror nor detach.
        let bar = draw_bar(42, 10, '█', '░');
        let visual = visual_order(&join_segments(&[
            ltr_segment("الْظُهْرُ"),
            ltr_segment("(Zuhr)"),
            ltr_segment(&format!("{bar} 42%")),
        ]));
        assert_eq!(
            visual,
            format!("{} (Zuhr) {bar} 42%", rtl_rendered("الْظُهْرُ"))
        );
    }

    #[test]
    fn bracketed_labels_render_brackets_correctly() {
        // User label with brackets and trailing punctuation: brackets stay
        // in their LTR positions around the RTL-rendered core.
        let bar = draw_bar(12, 5, '█', '░');
        let visual = visual_order(&compose("[الْفَجْرُ!]", &format!("{bar} 12%")));
        assert_eq!(visual, format!("[{}!] {bar} 12%", rtl_rendered("الْفَجْرُ")));
    }

    #[test]
    fn unfixed_text_mirrors_segments() {
        // Documents the original bug: with no controls at all, the Arabic
        // first-strong character flips the paragraph to RTL and the
        // segments come out mirrored.
        let plain = "الْظُهْرُ ██████████░ 89%";
        let visual = visual_order(plain);
        assert_ne!(visual, plain, "expected the unfixed string to mirror");
        assert!(
            visual.find('█').unwrap() < visual.find('\u{0627}').unwrap(),
            "in the mirrored output the bar appears left of the Arabic"
        );
    }

    #[test]
    fn lrm_alone_reproduces_the_reported_bug() {
        // The earlier attempt — LRM at position 0 but no isolates —
        // reproduced in the algorithm: the digits jump to the far left, the
        // bar's blocks reverse, and the "%" detaches to the far right:
        // exactly the observed "percentage bar arabic percent-sign". The
        // isolates asserted in the tests above are what actually fix it.
        let visual = visual_order(&format!("{LRM}الْظُهْرُ ██████████░ 89%"));
        assert!(visual.starts_with("89 "), "digits at line start: {visual:?}");
        assert!(visual.ends_with('%'), "percent-sign detached right: {visual:?}");
        assert!(
            visual.find('░').unwrap() < visual.find('█').unwrap(),
            "bar blocks reversed: {visual:?}"
        );
    }
}

/// Tests for the three-state machine with synthetic prayer times.
#[cfg(test)]
mod state_tests {
    use super::*;

    /// October 2026; day 6 = yesterday, 7 = today, 8 = tomorrow.
    fn t(day: u32, h: u32, m: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2026, 10, day)
            .expect("date")
            .and_hms_opt(h, m, 0)
            .expect("time")
            .and_utc()
    }

    /// Synthetic schedule: Fajr 05:00 (45), Dhuhr 12:00 (90), Asr 15:00 (75),
    /// Maghrib 18:00 (25), Isha 19:30 (35).
    fn day(day: u32) -> Vec<Entry> {
        let e = |name: &'static str, h: u32, m: u32, tol: i64| Entry {
            name: name.to_string(),
            name_en: name,
            time: t(day, h, m),
            tolerance_min: tol,
        };
        vec![
            e("Fajr", 5, 0, 45),
            e("Dhuhr", 12, 0, 90),
            e("Asr", 15, 0, 75),
            e("Maghrib", 18, 0, 25),
            e("Isha", 19, 30, 35),
        ]
    }

    fn state(now: DateTime<Utc>) -> State<'static> {
        // The day vectors are leaked so `state` can return borrows freely;
        // tests run once, so this is harmless.
        let leak = |v: Vec<Entry>| -> &'static [Entry] { Box::leak(v.into_boxed_slice()) };
        compute_state(now, leak(day(6)), leak(day(7)), leak(day(8)), 10).expect("state")
    }

    #[test]
    fn active_phases_follow_thresholds() {
        // 10 min into Fajr's 45-min window: 22%, normal.
        match state(t(7, 5, 10)) {
            State::Active {
                anchor,
                percent,
                class,
            } => {
                assert_eq!(anchor.name, "Fajr");
                assert_eq!(percent, 22);
                assert_eq!(class, "normal");
            }
            other => panic!("expected Active, got {other:?}"),
        }
        // 35 min: 77%, warning. 42 min: 93%, critical.
        match state(t(7, 5, 35)) {
            State::Active { percent, class, .. } => {
                assert_eq!((percent, class), (77, "warning"));
            }
            other => panic!("expected Active, got {other:?}"),
        }
        match state(t(7, 5, 42)) {
            State::Active { percent, class, .. } => {
                assert_eq!((percent, class), (93, "critical"));
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }

    #[test]
    fn boundaries_between_states() {
        // Exactly at the deadline: still Active (full bar, critical).
        match state(t(7, 5, 45)) {
            State::Active { percent, class, .. } => assert_eq!((percent, class), (100, "critical")),
            other => panic!("expected Active, got {other:?}"),
        }
        // One second into the grace period: Expired.
        let now = t(7, 5, 45) + Duration::seconds(1);
        match state(now) {
            State::Expired { anchor, next } => {
                assert_eq!(anchor.name, "Fajr");
                assert_eq!(next.name, "Dhuhr");
            }
            other => panic!("expected Expired, got {other:?}"),
        }
        // Last second of the grace period: still Expired; one second later:
        // Upcoming, bar nearly empty, 365 min to Dhuhr.
        match state(t(7, 5, 55)) {
            State::Expired { .. } => {}
            other => panic!("expected Expired, got {other:?}"),
        }
        match state(t(7, 5, 55) + Duration::seconds(1)) {
            State::Upcoming {
                next,
                percent,
                minutes,
            } => {
                assert_eq!(next.name, "Dhuhr");
                assert_eq!(percent, 0);
                assert_eq!(minutes, 365);
            }
            other => panic!("expected Upcoming, got {other:?}"),
        }
    }

    #[test]
    fn upcoming_bar_fills_towards_next_athan() {
        // 09:00: 185 of 365 minutes past the grace period → 50%, 3 h to go.
        match state(t(7, 9, 0)) {
            State::Upcoming {
                next,
                percent,
                minutes,
            } => {
                assert_eq!(next.name, "Dhuhr");
                assert_eq!(percent, 50);
                assert_eq!(minutes, 180);
            }
            other => panic!("expected Upcoming, got {other:?}"),
        }
    }

    #[test]
    fn pre_fajr_counts_down_to_todays_fajr() {
        // 03:00: yesterday's Isha window (closed 20:05, +10 grace) is long
        // gone — the bar counts down to today's Fajr.
        match state(t(7, 3, 0)) {
            State::Upcoming {
                next,
                percent,
                minutes,
            } => {
                assert_eq!(next.name, "Fajr");
                assert_eq!(minutes, 120);
                assert_eq!(percent, 77);
            }
            other => panic!("expected Upcoming, got {other:?}"),
        }
    }

    #[test]
    fn after_isha_counts_down_to_tomorrows_fajr() {
        // 21:00: today's Isha window closed at 20:05 (+10 grace) — next is
        // tomorrow's Fajr, 8 hours away.
        match state(t(7, 21, 0)) {
            State::Upcoming {
                next,
                percent,
                minutes,
            } => {
                assert_eq!(next.name, "Fajr");
                assert_eq!(next.time, t(8, 5, 0));
                assert_eq!(minutes, 480);
                assert_eq!(percent, 8);
            }
            other => panic!("expected Upcoming, got {other:?}"),
        }
    }
}

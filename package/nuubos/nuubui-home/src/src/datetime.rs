/*
 * Manual Date & Time picker (General → DATE & TIME rows Date/Time and the
 * OOB Date & Time step, only while Automatic Time is off; user request
 * 2026-10-10). A full-screen page with five wheels: Day, Month, Year on top,
 * Hour : Minute below. Left/right move between wheels, up/down change the
 * focused one (wrapping; the day is clamped to the month), confirm sets
 * the clock through regionald SET_LOCAL_TIME, back cancels.
 *
 * Rust owns the values; datetime-picker in home.slint only draws the
 * fields it gets (value, the one above = +1, the one below = -1).
 */

use crate::{regional_command, tr, DateTimeField, HomeWindow};
use slint::{ModelRc, SharedString, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::thread;

const YEAR_MIN: i32 = 2020;
const YEAR_MAX: i32 = 2099;
/* i18n: 875.. months, 887.. weekdays (Monday first), 894.. field labels;
 * 1287.. months as written inside a date (case, genitive: "15 października",
 * "15 октября"), 1299 the date pattern ({0} day, {1} month, {2} year), 1300
 * the same with the weekday ({3}): each language orders its own date. */
const MONTHS: usize = 875;
const WEEKDAYS: usize = 887;
const LABELS: usize = 894;
const DATE_MONTHS: usize = 1287;
const DATE_PATTERN: usize = 1299;
const DATE_WEEKDAY_PATTERN: usize = 1300;

thread_local! {
    /* year, month, day, hour, minute */
    static VALUES: Cell<[i32; 5]> = const { Cell::new([2026, 1, 1, 0, 0]) };
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/* 0 = Monday (Sakamoto). */
fn weekday(year: i32, month: i32, day: i32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let sunday_first = (y + y / 4 - y / 100 + y / 400 + T[(month - 1) as usize] + day).rem_euclid(7);
    ((sunday_first + 6) % 7) as usize
}

fn month_name(ui: &HomeWindow, month: i32) -> String {
    tr(ui, MONTHS + (month - 1) as usize, "")
}

/* A full date in the UI language; weekday: Monday = 0. */
fn format_date(ui: &HomeWindow, y: i32, m: i32, d: i32, weekday: Option<usize>) -> String {
    let pattern = match weekday {
        Some(_) => tr(ui, DATE_WEEKDAY_PATTERN, "{3} {0} {1} {2}"),
        None => tr(ui, DATE_PATTERN, "{0} {1} {2}"),
    };
    pattern
        .replace("{0}", &d.to_string())
        .replace("{1}", &tr(ui, DATE_MONTHS + (m - 1) as usize, &month_name(ui, m)))
        .replace("{2}", &y.to_string())
        .replace("{3}", &weekday.map(|w| tr(ui, WEEKDAYS + w, "")).unwrap_or_default())
}

/* "10 October 2026" from regionald's local_date (YYYY-MM-DD); the raw
 * value when it does not parse (clock unavailable). */
pub(crate) fn date_label(ui: &HomeWindow, iso: &str) -> String {
    let parts: Vec<i32> = iso.split('-').filter_map(|p| p.parse().ok()).collect();
    match parts[..] {
        [y, m, d] if (1..=12).contains(&m) => format_date(ui, y, m, d, None),
        _ => iso.to_owned(),
    }
}

fn apply(ui: &HomeWindow) {
    let [y, mo, d, h, mi] = VALUES.with(Cell::get);
    let wrap = |v: i32, lo: i32, hi: i32| lo + (v - lo).rem_euclid(hi - lo + 1);
    let dim = days_in_month(y, mo);
    let num = |v: i32| SharedString::from(format!("{v:02}"));
    let field = |label: usize, above: SharedString, value: SharedString, below: SharedString| DateTimeField {
        label: tr(ui, LABELS + label, "").into(),
        above,
        value,
        below,
    };
    let fields = vec![
        field(0, num(wrap(d + 1, 1, dim)), num(d), num(wrap(d - 1, 1, dim))),
        field(
            1,
            month_name(ui, wrap(mo + 1, 1, 12)).into(),
            month_name(ui, mo).into(),
            month_name(ui, wrap(mo - 1, 1, 12)).into(),
        ),
        field(
            2,
            if y < YEAR_MAX { (y + 1).to_string().into() } else { "".into() },
            y.to_string().into(),
            if y > YEAR_MIN { (y - 1).to_string().into() } else { "".into() },
        ),
        field(3, num(wrap(h + 1, 0, 23)), num(h), num(wrap(h - 1, 0, 23))),
        field(4, num(wrap(mi + 1, 0, 59)), num(mi), num(wrap(mi - 1, 0, 59))),
    ];
    ui.set_datetime_fields(ModelRc::new(Rc::new(VecModel::from(fields))));
    ui.set_datetime_preview(
        format_date(ui, y, mo, d, Some(weekday(y, mo, d))).into(),
    );
}

/* field: 0 = Day (Date row), 3 = Hour (Time row). */
pub(crate) fn open(ui: &HomeWindow, field: i32) {
    let status = regional_command("STATUS").unwrap_or_default();
    let mut values = [2026, 1, 1, 0, 0];
    for line in status.lines() {
        if let Some(date) = line.strip_prefix("local_date=") {
            let p: Vec<i32> = date.split('-').filter_map(|x| x.parse().ok()).collect();
            if let [y, m, d] = p[..] {
                values[0] = y.clamp(YEAR_MIN, YEAR_MAX);
                values[1] = m.clamp(1, 12);
                values[2] = d.clamp(1, days_in_month(values[0], values[1]));
            }
        } else if let Some(time) = line.strip_prefix("local_time=") {
            let p: Vec<i32> = time.split(':').filter_map(|x| x.parse().ok()).collect();
            if let [h, m] = p[..] {
                values[3] = h.clamp(0, 23);
                values[4] = m.clamp(0, 59);
            }
        }
    }
    VALUES.with(|v| v.set(values));
    ui.set_datetime_field_index(field);
    apply(ui);
    ui.set_datetime_picker_open(true);
}

fn step(field: i32, delta: i32) {
    VALUES.with(|cell| {
        let mut v = cell.get();
        match field {
            0 => v[2] = 1 + (v[2] - 1 + delta).rem_euclid(days_in_month(v[0], v[1])),
            1 => v[1] = 1 + (v[1] - 1 + delta).rem_euclid(12),
            2 => v[0] = (v[0] + delta).clamp(YEAR_MIN, YEAR_MAX),
            3 => v[3] = (v[3] + delta).rem_euclid(24),
            _ => v[4] = (v[4] + delta).rem_euclid(60),
        }
        v[2] = v[2].min(days_in_month(v[0], v[1]));
        cell.set(v);
    });
}

/* The caller (handle_settings_action) already played the cue. */
pub(crate) fn action(ui: &HomeWindow, action: &str) {
    let field = ui.get_datetime_field_index();
    match action {
        "menu_left" => ui.set_datetime_field_index((field - 1).max(0)),
        "menu_right" => ui.set_datetime_field_index((field + 1).min(4)),
        "menu_up" | "menu_down" => {
            step(field, if action == "menu_up" { 1 } else { -1 });
            apply(ui);
        }
        "menu_confirm" => {
            let [y, mo, d, h, mi] = VALUES.with(Cell::get);
            ui.set_datetime_picker_open(false);
            let command = format!("SET_LOCAL_TIME\t{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}");
            thread::spawn(move || {
                let _ = regional_command(&command);
            });
        }
        "menu_back" => ui.set_datetime_picker_open(false),
        _ => {}
    }
}

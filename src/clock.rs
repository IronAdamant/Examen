//! Local calendar date, matching `datetime.date.today()` and `date.fromisoformat`.

pub fn today() -> String {
    let (y, m, d) = local_ymd();
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn valid_date(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return false;
    }
    let y: i32 = value[0..4].parse().unwrap_or(0);
    let m: u32 = value[5..7].parse().unwrap_or(0);
    let d: u32 = value[8..10].parse().unwrap_or(0);
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) {
        return false;
    }
    d >= 1 && d <= days_in_month(y, m)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn local_ymd() -> (i32, u32, u32) {
    #[cfg(windows)]
    {
        windows_ymd()
    }
    #[cfg(not(windows))]
    {
        unix_ymd()
    }
}

#[cfg(windows)]
fn windows_ymd() -> (i32, u32, u32) {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        dow: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }
    extern "system" {
        fn GetLocalTime(out: *mut SystemTime);
    }
    let mut st = SystemTime {
        year: 0,
        month: 0,
        dow: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        ms: 0,
    };
    unsafe { GetLocalTime(&mut st) }
    if st.year == 0 {
        return utc_fallback();
    }
    (st.year as i32, st.month as u32, st.day as u32)
}

#[cfg(not(windows))]
fn unix_ymd() -> (i32, u32, u32) {
    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        pad: [i64; 8],
    }
    extern "C" {
        fn time(tloc: *mut i64) -> i64;
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }
    unsafe {
        let mut secs: i64 = 0;
        if time(&mut secs) < 0 {
            return utc_fallback();
        }
        let mut tm = std::mem::zeroed::<Tm>();
        if localtime_r(&secs, &mut tm).is_null() || tm.tm_mday == 0 {
            return utc_fallback();
        }
        (tm.tm_year + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32)
    }
}

fn utc_fallback() -> (i32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    civil_from_days(days)
}

/// Howard Hinnant's civil_from_days, days since 1970-01-01.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Howard Hinnant's days_from_civil: days since 1970-01-01.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(if m <= 2 { y - 1 } else { y });
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Seconds since the Unix epoch for `YYYY-MM-DD` (midnight UTC) or
/// `YYYY-MM-DDTHH:MM:SSZ`. `None` for anything else.
pub fn parse_utc(text: &str) -> Option<i64> {
    let (date, time) = match text.len() {
        10 => (text, None),
        20 if text.as_bytes()[10] == b'T' && text.ends_with('Z') => (&text[..10], Some(&text[11..19])),
        _ => return None,
    };
    if !valid_date(date) {
        return None;
    }
    let y: i32 = date[0..4].parse().ok()?;
    let m: u32 = date[5..7].parse().ok()?;
    let d: u32 = date[8..10].parse().ok()?;
    let mut secs = days_from_civil(y, m, d) * 86_400;
    if let Some(time) = time {
        let b = time.as_bytes();
        if b[2] != b':' || b[5] != b':' {
            return None;
        }
        let part = |range: std::ops::Range<usize>| -> Option<i64> {
            let field = &time[range];
            field.bytes().all(|c| c.is_ascii_digit()).then(|| field.parse().ok())?
        };
        let (h, mi, s) = (part(0..2)?, part(3..5)?, part(6..8)?);
        if h > 23 || mi > 59 || s > 59 {
            return None;
        }
        secs += h * 3600 + mi * 60 + s;
    }
    Some(secs)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since the Unix epoch.
pub fn format_utc(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Whole seconds since the Unix epoch; earlier times count as 0.
pub fn epoch_secs(time: std::time::SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

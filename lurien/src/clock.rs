//! The wall clock a session's pages read.
//!
//! The shift lives in the engine, because the only place a clock can be moved
//! without the page being able to see it happen is inside the page's own
//! compartment before its first script runs. This module owns the wire format a
//! human types and the shape a face reports, nothing else.

use crate::control::Control;
use crate::error::Error;

/// Days in each month of a non-leap year, for civil date arithmetic.
const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Milliseconds in a day, hour, minute.
const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;
const SECOND_MS: i64 = 1_000;

/// What a session's clock reads and how far that is from the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The time the session serves, in milliseconds since the epoch.
    pub epoch_ms: i64,
    /// How far that runs from the host clock. Zero means the host's own time.
    pub shift_ms: i64,
}

impl Reading {
    /// Whether this session serves a time of its own.
    #[must_use]
    pub const fn is_shifted(&self) -> bool {
        self.shift_ms != 0
    }
}

/// Read the clock this session serves.
///
/// # Errors
///
/// [`Error::ControlUnavailable`] when the engine cannot be reached.
pub async fn read(control: &Control) -> Result<Reading, Error> {
    let shift_ms = control.clock_shift().await?.unwrap_or(0);
    Ok(Reading {
        epoch_ms: host_now_ms() + shift_ms,
        shift_ms,
    })
}

/// Milliseconds since the epoch on this host.
///
/// # Panics
///
/// Never in practice: a system clock before 1970 would be needed, and the
/// duration is taken in the direction that cannot fail on a sane clock.
#[must_use]
pub fn host_now_ms() -> i64 {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// Read a time a human typed.
///
/// Accepts milliseconds since the epoch, or a date and time in the shape
/// `2033-05-18T03:33:20Z`: `T` or a space between date and time, seconds and
/// fractional seconds optional, `Z` or `+HH:MM` for the offset, and local
/// meaning UTC when no offset is given, since a session's own timezone is the
/// one a page reads.
///
/// # Errors
///
/// [`Error::BadArgs`] naming the shape when the text is not one of those.
pub fn parse_time(text: &str) -> Result<i64, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(bad("a time is required"));
    }
    if let Ok(ms) = text.parse::<i64>() {
        return Ok(ms);
    }
    let (date, rest) = text
        .split_once(['T', 't', ' '])
        .ok_or_else(|| bad(&format!("{text:?} has no time part")))?;
    let (year, month, day) = parse_date(date)?;
    let (clock, offset_ms) = split_offset(rest)?;
    let (hour, minute, second, fraction) = parse_clock(clock)?;
    let days = days_from_civil(year, month, day);
    Ok(days * DAY_MS
        + hour * HOUR_MS
        + minute * MINUTE_MS
        + second * SECOND_MS
        + fraction
        - offset_ms)
}

/// Write a time back the way [`parse_time`] reads it, in UTC.
#[must_use]
pub fn format_time(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(DAY_MS);
    let mut rest = epoch_ms.rem_euclid(DAY_MS);
    let (year, month, day) = civil_from_days(days);
    let hour = rest / HOUR_MS;
    rest %= HOUR_MS;
    let minute = rest / MINUTE_MS;
    rest %= MINUTE_MS;
    let second = rest / SECOND_MS;
    let millis = rest % SECOND_MS;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// A refusal that names the shapes a time may take. The verb is the one that
/// takes a time; a tick takes a count of milliseconds and never lands here.
fn bad(detail: &str) -> Error {
    Error::BadArgs {
        verb: "clock-set".to_string(),
        detail: format!(
            "{detail}. Use milliseconds since the epoch or a time like 2033-05-18T03:33:20Z"
        ),
    }
}

fn parse_date(date: &str) -> Result<(i64, i64, i64), Error> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return Err(bad(&format!("{date:?} is not a date")));
    }
    let year: i64 = parts[0]
        .parse()
        .map_err(|_| bad(&format!("{:?} is not a year", parts[0])))?;
    let month: i64 = parts[1]
        .parse()
        .map_err(|_| bad(&format!("{:?} is not a month", parts[1])))?;
    let day: i64 = parts[2]
        .parse()
        .map_err(|_| bad(&format!("{:?} is not a day", parts[2])))?;
    if !(1..=12).contains(&month) {
        return Err(bad(&format!("month {month} is not 1 to 12")));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(bad(&format!(
            "day {day} is not 1 to {} in {year}-{month:02}",
            days_in_month(year, month)
        )));
    }
    Ok((year, month, day))
}

/// Split a trailing `Z` or `±HH:MM` off the clock part.
fn split_offset(rest: &str) -> Result<(&str, i64), Error> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Ok((clock, 0));
    }
    for (index, byte) in rest.bytes().enumerate().skip(1) {
        if byte != b'+' && byte != b'-' {
            continue;
        }
        let sign = if byte == b'-' { -1 } else { 1 };
        let offset = &rest[index + 1..];
        let (hours, minutes) = match offset.split_once(':') {
            Some((h, m)) => (h, m),
            None if offset.len() == 4 => (&offset[..2], &offset[2..]),
            None => return Err(bad(&format!("{offset:?} is not an offset"))),
        };
        let hours: i64 = hours
            .parse()
            .map_err(|_| bad(&format!("{hours:?} is not an offset hour")))?;
        let minutes: i64 = minutes
            .parse()
            .map_err(|_| bad(&format!("{minutes:?} is not an offset minute")))?;
        if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
            return Err(bad(&format!("offset {offset:?} is out of range")));
        }
        return Ok((&rest[..index], sign * (hours * HOUR_MS + minutes * MINUTE_MS)));
    }
    // No offset: read as UTC. A session's clock is a wall clock its own timezone
    // renders, so guessing the host's zone here would put the host back in.
    Ok((rest, 0))
}

fn parse_clock(clock: &str) -> Result<(i64, i64, i64, i64), Error> {
    let (whole, fraction) = match clock.split_once('.') {
        Some((whole, digits)) => {
            let mut millis = String::from(digits);
            millis.truncate(3);
            while millis.len() < 3 {
                millis.push('0');
            }
            let millis: i64 = millis
                .parse()
                .map_err(|_| bad(&format!("{digits:?} is not a fraction of a second")))?;
            (whole, millis)
        }
        None => (clock, 0),
    };
    let parts: Vec<&str> = whole.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(bad(&format!("{clock:?} is not a time of day")));
    }
    let hour: i64 = parts[0]
        .parse()
        .map_err(|_| bad(&format!("{:?} is not an hour", parts[0])))?;
    let minute: i64 = parts[1]
        .parse()
        .map_err(|_| bad(&format!("{:?} is not a minute", parts[1])))?;
    let second: i64 = match parts.get(2) {
        Some(text) => text
            .parse()
            .map_err(|_| bad(&format!("{text:?} is not a second")))?,
        None => 0,
    };
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return Err(bad(&format!("{clock:?} is out of range")));
    }
    Ok((hour, minute, second.min(59), fraction))
}

const fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    if month == 2 && is_leap(year) {
        29
    } else {
        MONTH_DAYS[(month - 1) as usize]
    }
}

/// Days from 1970-01-01 to a civil date, after Howard Hinnant's algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_read_in_every_shape_a_human_types() {
        // The epoch itself, so the civil arithmetic is anchored.
        assert_eq!(parse_time("1970-01-01T00:00:00Z").unwrap(), 0);
        // A round number of milliseconds passes through untouched.
        assert_eq!(parse_time("2000000000000").unwrap(), 2_000_000_000_000);
        // The same instant in four shapes.
        let target = 2_000_000_000_000;
        for text in [
            "2033-05-18T03:33:20Z",
            "2033-05-18 03:33:20Z",
            "2033-05-18T03:33:20.000Z",
            "2033-05-18T05:33:20+02:00",
        ] {
            assert_eq!(parse_time(text).unwrap(), target, "{text}");
        }
        // Seconds are optional, and a fraction is milliseconds.
        assert_eq!(
            parse_time("2033-05-18T03:33Z").unwrap(),
            target - 20 * SECOND_MS
        );
        assert_eq!(parse_time("2033-05-18T03:33:20.5Z").unwrap(), target + 500);
        // A negative offset moves the instant the other way.
        assert_eq!(
            parse_time("2033-05-17T21:33:20-06:00").unwrap(),
            target
        );
    }

    #[test]
    fn a_time_before_the_epoch_reads_as_a_negative_instant() {
        assert_eq!(parse_time("1969-12-31T23:59:59Z").unwrap(), -1_000);
        assert_eq!(parse_time("1900-01-01T00:00:00Z").unwrap(), -2_208_988_800_000);
    }

    #[test]
    fn a_leap_day_exists_only_in_a_leap_year() {
        assert_eq!(
            parse_time("2024-02-29T00:00:00Z").unwrap(),
            1_709_164_800_000
        );
        let refused = parse_time("2023-02-29T00:00:00Z").unwrap_err().to_string();
        assert!(refused.contains("day 29 is not 1 to 28"), "{refused}");
    }

    #[test]
    fn a_time_that_is_not_a_time_names_the_shape_that_works() {
        for text in [
            "",
            "tomorrow",
            "2033-05-18",
            "2033-13-01T00:00:00Z",
            "2033-05-18T25:00:00Z",
            "2033-05-18T03:33:20+99:00",
        ] {
            let refused = parse_time(text).unwrap_err().to_string();
            assert!(
                refused.contains("2033-05-18T03:33:20Z"),
                "{text:?} was refused with {refused:?}"
            );
        }
    }

    #[test]
    fn a_formatted_time_reads_back_as_itself() {
        for ms in [
            0_i64,
            -1,
            1,
            2_000_000_000_000,
            -2_208_988_800_000,
            1_709_164_800_123,
            4_102_444_800_000,
        ] {
            let text = format_time(ms);
            assert_eq!(parse_time(&text).unwrap(), ms, "{text}");
        }
    }

    #[test]
    fn a_civil_date_survives_the_round_trip_across_centuries() {
        // Every month boundary of four centuries, which is where the era
        // arithmetic breaks if a sign or a leap rule is wrong.
        for year in 1800..2200 {
            for month in 1..=12 {
                let days = days_from_civil(year, month, 1);
                assert_eq!(civil_from_days(days), (year, month, 1));
            }
        }
    }

    #[test]
    fn a_reading_says_whether_the_session_serves_its_own_time() {
        assert!(!Reading {
            epoch_ms: 1,
            shift_ms: 0
        }
        .is_shifted());
        assert!(Reading {
            epoch_ms: 1,
            shift_ms: -1
        }
        .is_shifted());
    }
}

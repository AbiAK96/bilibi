//! Inclusive calendar-day bounds, independent of timezone and HR employment dates.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDates;
impl fmt::Display for InvalidDates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected valid YYYY-MM-DD dates with effective_from <= effective_to")
    }
}
impl std::error::Error for InvalidDates {}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CalendarDate(String);
impl CalendarDate {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for CalendarDate {
    type Error = InvalidDates;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        {
            return Err(InvalidDates);
        }
        let year = value[..4].parse::<u16>().map_err(|_| InvalidDates)?;
        let month = value[5..7].parse::<u8>().map_err(|_| InvalidDates)?;
        let day = value[8..].parse::<u8>().map_err(|_| InvalidDates)?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(InvalidDates),
        };
        if year == 0 || day == 0 || day > days {
            return Err(InvalidDates);
        }
        Ok(Self(value))
    }
}
impl From<CalendarDate> for String {
    fn from(value: CalendarDate) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "DateBounds", into = "DateBounds")]
pub struct EffectiveDates {
    from: Option<CalendarDate>,
    to: Option<CalendarDate>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DateBounds {
    effective_from: Option<CalendarDate>,
    effective_to: Option<CalendarDate>,
}
impl EffectiveDates {
    pub fn new(from: Option<CalendarDate>, to: Option<CalendarDate>) -> Result<Self, InvalidDates> {
        if matches!((&from, &to), (Some(from), Some(to)) if from > to) {
            return Err(InvalidDates);
        }
        Ok(Self { from, to })
    }
    pub fn from(&self) -> Option<&CalendarDate> {
        self.from.as_ref()
    }
    pub fn to(&self) -> Option<&CalendarDate> {
        self.to.as_ref()
    }
}
impl TryFrom<DateBounds> for EffectiveDates {
    type Error = InvalidDates;
    fn try_from(value: DateBounds) -> Result<Self, Self::Error> {
        Self::new(value.effective_from, value.effective_to)
    }
}
impl From<EffectiveDates> for DateBounds {
    fn from(value: EffectiveDates) -> Self {
        Self {
            effective_from: value.from,
            effective_to: value.to,
        }
    }
}

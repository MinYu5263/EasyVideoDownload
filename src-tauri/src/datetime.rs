use chrono::{DateTime, NaiveDateTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Deserializer};

const FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const LEGACY_UTC_FORMAT: &str = "%Y-%m-%d %H:%M:%S%:z";

fn format_beijing(date: DateTime<Utc>) -> Option<String> {
    // Use a fixed UTC+8 offset, independently of the operating system's timezone.
    let value = date
        .naive_utc()
        .checked_add_signed(TimeDelta::hours(8))?
        .format(FORMAT)
        .to_string();
    is_valid(&value).then_some(value)
}

pub(crate) fn now() -> String {
    format_beijing(Utc::now()).expect("current Beijing time is representable")
}

pub(crate) fn is_valid(value: &str) -> bool {
    value.len() == 19
        && NaiveDateTime::parse_from_str(value, FORMAT)
            .is_ok_and(|date| date.nanosecond() == 0 && date.format(FORMAT).to_string() == value)
}

pub(crate) fn from_legacy_utc(value: &str) -> Option<String> {
    if value.len() != 25 {
        return None;
    }
    let date = DateTime::parse_from_str(value, LEGACY_UTC_FORMAT)
        .ok()?
        .with_timezone(&Utc);
    if date.nanosecond() != 0 || date.format(LEGACY_UTC_FORMAT).to_string() != value {
        return None;
    }
    format_beijing(date)
}

// Compatibility is limited to reading legacy tool JSON; all new writes use text.
pub(crate) fn deserialize_legacy<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum LegacyTime {
        Text(String),
        Seconds(u64),
    }
    let value = match LegacyTime::deserialize(deserializer)? {
        LegacyTime::Text(value) if is_valid(&value) => value,
        LegacyTime::Text(value) => from_legacy_utc(&value)
            .ok_or_else(|| serde::de::Error::custom("invalid legacy detection time"))?,
        LegacyTime::Seconds(seconds) => i64::try_from(seconds)
            .ok()
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .and_then(format_beijing)
            .ok_or_else(|| serde::de::Error::custom("invalid legacy detection time"))?,
    };
    if is_valid(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "expected Beijing time in YYYY-MM-DD HH:MM:SS format",
        ))
    }
}

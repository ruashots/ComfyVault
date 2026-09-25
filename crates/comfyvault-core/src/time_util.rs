//! Timestamps.
//!
//! The engine stores an instant as milliseconds since the Unix epoch, because
//! that sorts and compares without a parser. It renders an instant as RFC 3339
//! in UTC, because that is what the contract promises the interface.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// An instant, stored as milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub fn now() -> Self {
        let now = OffsetDateTime::now_utc();
        Self((now.unix_timestamp_nanos() / 1_000_000) as i64)
    }

    pub fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    pub fn as_millis(self) -> i64 {
        self.0
    }

    /// Renders as RFC 3339 in UTC, for example `2026-09-22T14:31:07.482Z`.
    pub fn to_rfc3339(self) -> String {
        let nanos = (self.0 as i128) * 1_000_000;
        match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
            Ok(dt) => dt.format(&Rfc3339).unwrap_or_else(|_| self.0.to_string()),
            // An out of range value is a corrupt record, not a crash.
            Err(_) => self.0.to_string(),
        }
    }

    pub fn parse_rfc3339(s: &str) -> Option<Self> {
        OffsetDateTime::parse(s, &Rfc3339)
            .ok()
            .map(|dt| Self((dt.unix_timestamp_nanos() / 1_000_000) as i64))
    }

    /// Reads the modification time of a file as whole nanoseconds.
    ///
    /// The scan cache compares this exactly, so it must not be rounded.
    pub fn mtime_nanos(meta: &std::fs::Metadata) -> i128 {
        match meta.modified() {
            Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_nanos() as i128,
                // A file dated before 1970 still needs a stable key.
                Err(e) => -(e.duration().as_nanos() as i128),
            },
            Err(_) => 0,
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Timestamp::parse_rfc3339(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("not an RFC 3339 timestamp: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_rfc3339_in_utc_with_a_z_suffix() {
        // `date -u -d "2026-09-22T14:31:07.482Z" +%s%3N`
        let t = Timestamp(1790087467482);
        let s = t.to_rfc3339();
        assert!(s.starts_with("2026-09-22T14:31:07"), "got {s}");
        assert!(s.ends_with('Z'), "must be UTC with a Z suffix, got {s}");
    }

    #[test]
    fn round_trips_through_json() {
        let t = Timestamp(1790087467482);
        let json = serde_json::to_string(&t).unwrap();
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn rejects_text_that_is_not_a_timestamp() {
        assert!(serde_json::from_str::<Timestamp>("\"yesterday\"").is_err());
    }

    #[test]
    fn an_out_of_range_value_renders_instead_of_panicking() {
        // A corrupt stored record must not take the process down.
        let t = Timestamp(i64::MAX);
        let _ = t.to_rfc3339();
    }

    #[test]
    fn mtime_nanos_is_stable_for_one_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        let a = Timestamp::mtime_nanos(&std::fs::metadata(&p).unwrap());
        let b = Timestamp::mtime_nanos(&std::fs::metadata(&p).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn mtime_nanos_changes_when_the_file_is_rewritten() {
        // The scan cache trusts this to notice an edit. If it stayed equal, the
        // engine would reuse a stale hash and move the wrong bytes.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"first").unwrap();
        let before = Timestamp::mtime_nanos(&std::fs::metadata(&p).unwrap());

        let f = std::fs::File::options().write(true).open(&p).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        f.set_modified(later).unwrap();
        drop(f);

        let after = Timestamp::mtime_nanos(&std::fs::metadata(&p).unwrap());
        assert_ne!(before, after);
    }
}

/// A nanosecond file time, written on the wire as a string.
///
/// JavaScript parses a JSON number into a double, which silently drops the
/// last digits of a nanosecond time. Measured: the engine sends
/// 1758240123456789012 and JavaScript reads 1758240123456789000. A string
/// keeps every digit.
///
/// It also lets one of these sit inside a flattened payload. Serde buffers a
/// flattened value through a type that has no 128 bit number, so a flattened
/// record holding one cannot be read back at all.
///
/// Reading accepts a plain number as well, so a vault written by an earlier
/// build still opens.
pub mod nanos_as_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &i128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WrittenAs {
            Text(String),
            Number(i64),
        }
        match WrittenAs::deserialize(d)? {
            WrittenAs::Text(s) => s.parse().map_err(serde::de::Error::custom),
            WrittenAs::Number(n) => Ok(n as i128),
        }
    }
}

/// [`nanos_as_string`] for a time that may be absent.
pub mod optional_nanos_as_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<i128>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => super::nanos_as_string::serialize(v, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i128>, D::Error> {
        #[derive(Deserialize)]
        struct Present(#[serde(with = "super::nanos_as_string")] i128);
        Ok(Option::<Present>::deserialize(d)?.map(|p| p.0))
    }
}

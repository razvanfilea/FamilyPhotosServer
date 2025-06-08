use exif::{Field, In, Tag, Value};
use mime_guess::MimeGuess;
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::str::from_utf8;
use std::sync::LazyLock;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};
use tracing::error;

pub fn get_timestamp_for_path<P: AsRef<Path>>(path: P) -> Option<OffsetDateTime> {
    get_json_timestamp(path.as_ref())
        .map_or_else(
            || get_exif_timestamp(path.as_ref()),
            |json_timestamp| OffsetDateTime::from_unix_timestamp(json_timestamp as i64).ok(),
        )
        .or_else(|| get_regex_timestamp(path))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GooglePhotoTimestamp {
    timestamp: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GooglePhotoJsonData {
    creation_time: Option<GooglePhotoTimestamp>,
    photo_taken_time: Option<GooglePhotoTimestamp>,
}

impl GooglePhotoJsonData {
    fn u64_creation_time(&self) -> Option<u64> {
        let time = self.creation_time.as_ref()?;
        time.timestamp.parse().ok()
    }

    fn u64_photo_taken_time(&self) -> Option<u64> {
        let time = self.photo_taken_time.as_ref()?;
        time.timestamp.parse().ok()
    }
}

fn get_json_timestamp(path: &Path) -> Option<u64> {
    let json_file_name = format!("{}.json", path.to_string_lossy());

    let file = fs::File::open(&json_file_name).ok()?;
    let reader = BufReader::new(file);
    let json = serde_json::from_reader::<_, GooglePhotoJsonData>(reader);

    match json {
        Ok(json_data) => json_data
            .u64_photo_taken_time()
            .or(json_data.u64_creation_time()),
        Err(e) => {
            error!("Failed parsing Json ({json_file_name}): {e}");
            None
        }
    }
}

fn parse_date_from_field(field: &Field) -> Option<OffsetDateTime> {
    let string = match &field.value {
        Value::Ascii(v) if v.len() == 1 => from_utf8(&v[0]).ok()?,
        Value::Ascii(v) if v.len() > 1 => {
            for t in &v[1..] {
                if !t.is_empty() {
                    return None;
                }
            }
            from_utf8(&v[0]).ok()?
        }
        _ => return None,
    };

    let format_simple = format_description!("[year]:[month]:[day] [hour]:[minute]:[second]");
    let format_timezone = format_description!(
        "[year]:[month]:[day] [hour]:[minute]:[second] [offset_hour]:[offset_minute]"
    );

    OffsetDateTime::parse(string, &format_timezone)
        .or_else(|_| {
            PrimitiveDateTime::parse(string, &format_simple).map(|date_time| date_time.assume_utc())
        })
        .ok()
}

fn get_exif_timestamp(path: &Path) -> Option<OffsetDateTime> {
    let mime = MimeGuess::from_ext(path.extension()?.to_str()?).first_or_octet_stream();
    if mime.type_() != "image" {
        return None;
    }

    let file = fs::File::open(path).ok()?;
    let mut bufreader = BufReader::new(&file);
    let reader = exif::Reader::new()
        .read_from_container(&mut bufreader)
        .ok()?;

    let time_tags = [
        Tag::OffsetTimeOriginal,
        Tag::OffsetTimeDigitized,
        Tag::DateTimeOriginal,
        Tag::DateTimeDigitized,
    ];

    for tag in time_tags {
        if let Some(date) = reader
            .get_field(tag, In::PRIMARY)
            .and_then(parse_date_from_field)
        {
            return Some(date);
        }
    }

    None
}

fn get_regex_timestamp<P: AsRef<Path>>(path: P) -> Option<OffsetDateTime> {
    static DATE_HOUR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d{4})\D*(\d{2})\D*(\d{2})\D*(\d{2})\D*(\d{2})\D*(\d{2})").unwrap()
    });
    static DATE_PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d{4})\D?(\d{2})\D?(\d{2})").unwrap());
    static MILLIS_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r".*(\d{13})").unwrap());

    let name = path.as_ref().file_stem()?.to_string_lossy().to_string();

    if let Some(capture) = DATE_HOUR_PATTERN.captures(&name) {
        let year = &capture[1];
        let month = &capture[2];
        let day = &capture[3];
        let hour = &capture[4];
        let minute = &capture[5];
        let second = &capture[6];

        let string = format!("{year}{month}{day}{hour}{minute}{second}");
        let result = PrimitiveDateTime::parse(
            string.as_str(),
            format_description!("[year][month][day][hour][minute][second]"),
        );

        if let Ok(parsed_time) = result {
            return Some(parsed_time.assume_utc());
        }
    }

    if let Some(capture) = DATE_PATTERN.captures(&name) {
        let year = &capture[1];
        let month = &capture[2];
        let day = &capture[3];
        if let Ok(parsed_time) = PrimitiveDateTime::parse(
            format!("{year}{month}{day} 000000").as_str(),
            format_description!("[year][month][day] [hour][minute][second]"),
        ) {
            return Some(parsed_time.assume_utc());
        }
    }

    if let Some(capture) = MILLIS_PATTERN.captures(&name) {
        let millis: i64 = capture[1].parse().ok()?;
        let seconds = millis / 1000;
        if let Ok(parsed_time) = OffsetDateTime::from_unix_timestamp(seconds) {
            return Some(parsed_time);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn test_regex_timestamp() {
        let expected_date = Some(datetime!(2016-09-22 16:04:30 UTC));
        assert_eq!(
            get_regex_timestamp("IMG_20160922_160430.jpg"),
            expected_date
        );

        assert_eq!(
            get_regex_timestamp("IMG_2016-09-22-16-04-30.jpg"),
            expected_date
        );

        assert_eq!(
            get_regex_timestamp("20160922.jpg"),
            Some(datetime!(2016-09-22 00:00:00 UTC))
        );

        assert_eq!(
            get_regex_timestamp("2016_09_22.jpg"),
            Some(datetime!(2016-09-22 00:00:00 UTC))
        );

        assert_eq!(get_regex_timestamp("2016__09_22.jpg"), None);

        assert_eq!(
            get_regex_timestamp("random-1474560270000.jpg"),
            expected_date
        );

        assert_eq!(
            get_regex_timestamp("random-IMG_2016-09-22-160430 (5).jpg"),
            expected_date
        );
    }

    #[test]
    fn json_parsing() {
        let json = r#"{
            "creationTime": {
                "timestamp": "1437327811",
                "formatted": "19 iul. 2015, 17:43:31 UTC"
            },
            "photoTakenTime": {
                "timestamp": "1435786122",
                "formatted": "1 iul. 2015, 21:28:42 UTC"
            }
        }"#;

        let data: GooglePhotoJsonData = serde_json::from_str(json).unwrap();
        assert_eq!(data.u64_creation_time(), Some(1437327811));
        assert_eq!(data.u64_photo_taken_time(), Some(1435786122));
    }
}

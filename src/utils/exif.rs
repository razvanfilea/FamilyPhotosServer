use crate::utils::media::{is_video, media_label};
use exif::{Field, In, Tag, Value};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

const FFPROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize)]
pub struct ExifField {
    tag: String,
    value: String,
}

pub fn read_exif<P: AsRef<Path>>(absolute_path: P) -> Option<Vec<ExifField>> {
    let file = std::fs::File::open(absolute_path).ok()?;
    let mut bufreader = std::io::BufReader::new(&file);
    let reader = exif::Reader::new()
        .read_from_container(&mut bufreader)
        .ok()?;

    let mut exif_data = vec![];

    for f in reader.fields() {
        if f.ifd_num == In::PRIMARY {
            exif_data.push(ExifField {
                tag: f.tag.to_string(),
                value: value_to_string(&f.value),
            });
        }
    }

    Some(exif_data)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Byte(vec) => format!("{vec:?}"),
        Value::Ascii(vec) => vec
            .iter()
            .map(|v| String::from_utf8_lossy(v))
            .collect::<Vec<_>>()
            .join(" "),
        Value::Short(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Long(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Rational(vec) => vec
            .iter()
            .map(|r| format!("{}/{}", r.num, r.denom))
            .collect::<Vec<_>>()
            .join(","),
        Value::SByte(vec) => format!("{vec:?}"),
        Value::Undefined(data, _) => format!("(undefined {} bytes)", data.len()),
        Value::SShort(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::SLong(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::SRational(vec) => vec
            .iter()
            .map(|r| format!("{}/{}", r.num, r.denom))
            .collect::<Vec<_>>()
            .join(","),
        Value::Float(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Double(vec) => vec
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Unknown(_type, count, offset) => {
            format!("(unknown {count} bytes at offset {offset})")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GpsLocation {
    pub lat: f64,
    pub lon: f64,
}

/// Structured, display-oriented details about a photo or video
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PhotoDetails {
    /// e.g. "Image (JPG)"
    pub media_label: String,
    /// Displayed width, i.e. after applying the EXIF orientation / video rotation
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub megapixels: Option<f32>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub f_number: Option<f32>,
    /// "1/250s" or "2s"
    pub exposure: Option<String>,
    pub focal_length_mm: Option<f32>,
    pub iso: Option<u32>,
    pub gps: Option<GpsLocation>,
    pub duration_secs: Option<f32>,
}

impl PhotoDetails {
    fn set_dimensions(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = Some(width);
        self.height = Some(height);
        let megapixels = width as f64 * height as f64 / 1_000_000.0;
        self.megapixels = Some(((megapixels * 10.0).round() / 10.0) as f32);
    }
}

/// Reads the details of the file at `path` (named `name` in the DB). Blocking: runs `ffprobe`
/// for videos and reads the file for images, so call it from `spawn_blocking`.
/// Anything that can't be read is left as `None`.
pub fn photo_details(path: &Path, name: &str) -> PhotoDetails {
    let mut details = PhotoDetails {
        media_label: media_label(name),
        ..Default::default()
    };

    if is_video(name) {
        if let Some(probe) = run_ffprobe(path) {
            apply_ffprobe_output(&mut details, &probe);
        }
        return details;
    }

    if let Some(exif) = std::fs::File::open(path).ok().and_then(|file| {
        exif::Reader::new()
            .read_from_container(&mut std::io::BufReader::new(file))
            .ok()
    }) {
        apply_exif_fields(&mut details, |tag| exif.get_field(tag, In::PRIMARY));
    }

    if details.width.is_none()
        && let Ok(size) = imagesize::size(path)
    {
        details.set_dimensions(size.width as u32, size.height as u32);
    }

    details
}

fn apply_exif_fields<'a>(details: &mut PhotoDetails, field: impl Fn(Tag) -> Option<&'a Field>) {
    let uint = |tag| field(tag).and_then(|f| f.value.get_uint(0));

    let width = uint(Tag::PixelXDimension).or_else(|| uint(Tag::ImageWidth));
    let height = uint(Tag::PixelYDimension).or_else(|| uint(Tag::ImageLength));
    if let (Some(width), Some(height)) = (width, height) {
        // Orientations 5-8 are rotated by 90 degrees
        if matches!(uint(Tag::Orientation), Some(5..=8)) {
            details.set_dimensions(height, width);
        } else {
            details.set_dimensions(width, height);
        }
    }

    details.camera_make = field(Tag::Make).and_then(ascii_value);
    details.camera_model = field(Tag::Model).and_then(ascii_value);
    details.lens_model = field(Tag::LensModel).and_then(ascii_value);
    details.f_number = field(Tag::FNumber)
        .and_then(rational_value)
        .map(|v| v as f32);
    details.exposure = field(Tag::ExposureTime)
        .and_then(rational_value)
        .and_then(format_exposure);
    details.focal_length_mm = field(Tag::FocalLength)
        .and_then(rational_value)
        .map(|v| v as f32);
    details.iso = uint(Tag::PhotographicSensitivity).filter(|iso| *iso > 0);

    let latitude =
        field(Tag::GPSLatitude).and_then(|f| gps_coordinate(f, field(Tag::GPSLatitudeRef)));
    let longitude =
        field(Tag::GPSLongitude).and_then(|f| gps_coordinate(f, field(Tag::GPSLongitudeRef)));
    if let (Some(lat), Some(lon)) = (latitude, longitude) {
        details.gps = Some(GpsLocation { lat, lon });
    }
}

fn ascii_value(field: &Field) -> Option<String> {
    let Value::Ascii(ref values) = field.value else {
        return None;
    };
    let text = values
        .iter()
        .map(|v| {
            String::from_utf8_lossy(v)
                .trim_matches(char::from(0))
                .trim()
                .to_owned()
        })
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

/// The first value of a (signed) rational field as a decimal
fn rational_value(field: &Field) -> Option<f64> {
    let value = match field.value {
        Value::Rational(ref v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        Value::SRational(ref v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        _ => None,
    }?;
    value.is_finite().then_some(value)
}

/// Degrees, minutes, seconds + a N/S/E/W reference → signed decimal degrees
fn gps_coordinate(dms: &Field, reference: Option<&Field>) -> Option<f64> {
    let Value::Rational(ref parts) = dms.value else {
        return None;
    };
    if parts.len() != 3 || parts.iter().any(|r| r.denom == 0) {
        return None;
    }

    let degrees = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
    let negative = reference
        .and_then(ascii_value)
        .is_some_and(|r| r.eq_ignore_ascii_case("S") || r.eq_ignore_ascii_case("W"));

    Some(if negative { -degrees } else { degrees })
}

/// Like Android: "1/250s" below one second, else whole seconds ("2s")
fn format_exposure(seconds: f64) -> Option<String> {
    if seconds <= 0.0 {
        return None;
    }
    Some(if seconds >= 1.0 {
        format!("{}s", seconds as u64)
    } else {
        format!("1/{}s", (1.0 / seconds).round() as u64)
    })
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeStream {
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    side_data_list: Vec<FfprobeSideData>,
    #[serde(default)]
    tags: FfprobeTags,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeSideData {
    rotation: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeTags {
    rotate: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

fn run_ffprobe(path: &Path) -> Option<FfprobeOutput> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:stream_side_data=rotation:stream_tags=rotate:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    match child.wait_timeout(FFPROBE_TIMEOUT) {
        Ok(Some(status)) if status.success() => {}
        Ok(Some(_)) => return None,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    }

    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    serde_json::from_str(&output).ok()
}

fn apply_ffprobe_output(details: &mut PhotoDetails, probe: &FfprobeOutput) {
    if let Some(stream) = probe.streams.first()
        && let (Some(width), Some(height)) = (stream.width, stream.height)
    {
        let rotation = stream
            .side_data_list
            .iter()
            .find_map(|s| s.rotation)
            .or_else(|| stream.tags.rotate.as_deref().and_then(|r| r.parse().ok()))
            .unwrap_or(0.0);
        if (rotation.abs() as u32) % 180 == 90 {
            details.set_dimensions(height, width);
        } else {
            details.set_dimensions(width, height);
        }
    }

    details.duration_secs = probe
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f32>().ok())
        .filter(|d| d.is_finite() && *d >= 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::{Rational, SRational};

    fn field(tag: Tag, value: Value) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        }
    }

    fn ascii(text: &str) -> Value {
        Value::Ascii(vec![text.as_bytes().to_vec()])
    }

    fn rational(num: u32, denom: u32) -> Value {
        Value::Rational(vec![Rational { num, denom }])
    }

    fn details_from(fields: &[Field]) -> PhotoDetails {
        let mut details = PhotoDetails::default();
        apply_exif_fields(&mut details, |tag| fields.iter().find(|f| f.tag == tag));
        details
    }

    #[test]
    fn test_rational_value() {
        assert_eq!(
            rational_value(&field(Tag::FNumber, rational(18, 10))),
            Some(1.8)
        );
        assert_eq!(rational_value(&field(Tag::FNumber, rational(1, 0))), None);
        assert_eq!(
            rational_value(&field(
                Tag::ExposureBiasValue,
                Value::SRational(vec![SRational { num: -1, denom: 3 }])
            )),
            Some(-1.0 / 3.0)
        );
        assert_eq!(rational_value(&field(Tag::FNumber, ascii("1.8"))), None);
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(1.0 / 250.0).as_deref(), Some("1/250s"));
        assert_eq!(format_exposure(0.0166).as_deref(), Some("1/60s"));
        assert_eq!(format_exposure(0.5).as_deref(), Some("1/2s"));
        assert_eq!(format_exposure(2.0).as_deref(), Some("2s"));
        assert_eq!(format_exposure(2.5).as_deref(), Some("2s"));
        assert_eq!(format_exposure(0.0), None);
    }

    #[test]
    fn test_gps_coordinate() {
        let dms = field(
            Tag::GPSLatitude,
            Value::Rational(vec![
                Rational { num: 44, denom: 1 },
                Rational { num: 25, denom: 1 },
                Rational {
                    num: 3000,
                    denom: 100,
                },
            ]),
        );
        let north = field(Tag::GPSLatitudeRef, ascii("N"));
        let south = field(Tag::GPSLatitudeRef, ascii("S"));
        let expected = 44.0 + 25.0 / 60.0 + 30.0 / 3600.0;

        assert_eq!(gps_coordinate(&dms, Some(&north)), Some(expected));
        assert_eq!(gps_coordinate(&dms, None), Some(expected));
        assert_eq!(gps_coordinate(&dms, Some(&south)), Some(-expected));

        let west = field(Tag::GPSLongitudeRef, ascii("W"));
        let lon = field(
            Tag::GPSLongitude,
            Value::Rational(vec![
                Rational { num: 26, denom: 1 },
                Rational { num: 6, denom: 1 },
                Rational { num: 0, denom: 1 },
            ]),
        );
        assert_eq!(gps_coordinate(&lon, Some(&west)), Some(-26.1));

        let invalid = field(Tag::GPSLatitude, rational(44, 1));
        assert_eq!(gps_coordinate(&invalid, Some(&north)), None);
    }

    #[test]
    fn test_apply_exif_fields() {
        let details = details_from(&[
            field(Tag::PixelXDimension, Value::Long(vec![4032])),
            field(Tag::PixelYDimension, Value::Long(vec![3024])),
            field(Tag::Orientation, Value::Short(vec![1])),
            field(Tag::Make, ascii("Google")),
            field(Tag::Model, ascii("Pixel 8\0")),
            field(Tag::LensModel, ascii("  ")),
            field(Tag::FNumber, rational(168, 100)),
            field(Tag::ExposureTime, rational(1, 250)),
            field(Tag::FocalLength, rational(690, 100)),
            field(Tag::PhotographicSensitivity, Value::Short(vec![100])),
            field(
                Tag::GPSLatitude,
                Value::Rational(vec![
                    Rational { num: 44, denom: 1 },
                    Rational { num: 30, denom: 1 },
                    Rational { num: 0, denom: 1 },
                ]),
            ),
            field(Tag::GPSLatitudeRef, ascii("N")),
            field(
                Tag::GPSLongitude,
                Value::Rational(vec![
                    Rational { num: 26, denom: 1 },
                    Rational { num: 15, denom: 1 },
                    Rational { num: 0, denom: 1 },
                ]),
            ),
            field(Tag::GPSLongitudeRef, ascii("E")),
        ]);

        assert_eq!(details.width, Some(4032));
        assert_eq!(details.height, Some(3024));
        assert_eq!(details.megapixels, Some(12.2));
        assert_eq!(details.camera_make.as_deref(), Some("Google"));
        assert_eq!(details.camera_model.as_deref(), Some("Pixel 8"));
        assert_eq!(details.lens_model, None);
        assert_eq!(details.f_number, Some(1.68));
        assert_eq!(details.exposure.as_deref(), Some("1/250s"));
        assert_eq!(details.focal_length_mm, Some(6.9));
        assert_eq!(details.iso, Some(100));
        assert_eq!(
            details.gps,
            Some(GpsLocation {
                lat: 44.5,
                lon: 26.25
            })
        );
    }

    #[test]
    fn test_orientation_swaps_dimensions() {
        let details = details_from(&[
            field(Tag::ImageWidth, Value::Long(vec![4000])),
            field(Tag::ImageLength, Value::Long(vec![3000])),
            field(Tag::Orientation, Value::Short(vec![6])),
        ]);
        assert_eq!((details.width, details.height), (Some(3000), Some(4000)));

        let details = details_from(&[
            field(Tag::PixelXDimension, Value::Short(vec![0])),
            field(Tag::PixelYDimension, Value::Short(vec![0])),
        ]);
        assert_eq!((details.width, details.height), (None, None));
    }

    #[test]
    fn test_apply_ffprobe_output() {
        let probe: FfprobeOutput = serde_json::from_str(
            r#"{"programs":[],"streams":[{"width":1920,"height":1080,"side_data_list":[{"rotation":-90}]}],"format":{"duration":"12.480000"}}"#,
        )
        .unwrap();
        let mut details = PhotoDetails::default();
        apply_ffprobe_output(&mut details, &probe);
        assert_eq!((details.width, details.height), (Some(1080), Some(1920)));
        assert_eq!(details.duration_secs, Some(12.48));

        let probe: FfprobeOutput = serde_json::from_str(
            r#"{"streams":[{"width":640,"height":480,"tags":{"rotate":"180"}}],"format":{}}"#,
        )
        .unwrap();
        let mut details = PhotoDetails::default();
        apply_ffprobe_output(&mut details, &probe);
        assert_eq!((details.width, details.height), (Some(640), Some(480)));
        assert_eq!(details.duration_secs, None);
    }

    #[test]
    fn test_photo_details_header_fallback() {
        // A PNG header without EXIF: the size comes from the IHDR chunk
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, png).unwrap();

        let details = photo_details(&path, "image.png");
        assert_eq!(details.media_label, "Image (PNG)");
        assert_eq!((details.width, details.height), (Some(640), Some(480)));
        assert_eq!(details.megapixels, Some(0.3));
        assert_eq!(details.camera_model, None);
    }

    #[test]
    fn test_photo_details_missing_file() {
        let details = photo_details(Path::new("/nonexistent/IMG_1.jpg"), "IMG_1.jpg");
        assert_eq!(
            details,
            PhotoDetails {
                media_label: "Image (JPG)".to_string(),
                ..Default::default()
            }
        );
        let details = photo_details(Path::new("/nonexistent/VID_1.mp4"), "VID_1.mp4");
        assert_eq!(details.media_label, "Video (MP4)");
        assert_eq!(details.width, None);
    }
}

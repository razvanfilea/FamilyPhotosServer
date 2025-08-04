use exif::{In, Value};
use serde::Serialize;
use std::path::Path;

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

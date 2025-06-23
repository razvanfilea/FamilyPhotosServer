use crate::model::exif_field::{ExifField, ExifFields};
use exif::In;

pub fn read_file_exif_from_bytes(file_content: Vec<u8>) -> Option<ExifFields> {
    let mut cursor = std::io::Cursor::new(file_content);
    let reader = exif::Reader::new().read_from_container(&mut cursor).ok()?;

    let fields = reader
        .fields()
        .filter(|field| field.ifd_num == In::PRIMARY)
        .map(|field| ExifField {
            tag: field.tag.to_string(),
            value: field.display_value().to_string(),
        })
        .collect();

    Some(fields)
}

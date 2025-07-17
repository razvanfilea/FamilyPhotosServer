use crate::file_scan::exif::read_file_exif_from_bytes;
use crate::http::AppStateRef;
use crate::model::exif_field::ExifFields;
use crate::model::photo::PhotoBase;
use crate::model::photo_extras::PhotoExtras;
use mime_guess::{MimeGuess, mime};
use rayon::prelude::*;
use sha2::Digest;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tracing::info;

pub async fn compute_photos_extras(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos = app_state
        .photo_extras_repo
        .get_photos_without_extras()
        .await?;

    if photos.is_empty() {
        return Ok(());
    }
    info!("Computing extras for {} photos", photos.len());

    let photos_hashes: Vec<_> = photos
        .into_par_iter()
        .filter_map(|photo| {
            let path = app_state.storage.resolve_photo(photo.partial_path());

            compute_extras(&path).ok().map(|(hash, exif)| PhotoExtras {
                id: photo.id,
                hash,
                exif_json: exif.map(Into::into),
            })
        })
        .collect();

    info!("Computed extras for {} photos", photos_hashes.len());

    for chunk in photos_hashes.chunks(1024) {
        app_state.photo_extras_repo.insert_hashes(chunk).await?;
    }

    Ok(())
}

fn compute_extras(path: &Path) -> std::io::Result<(Vec<u8>, Option<ExifFields>)> {
    let mut file_contents = Vec::new();
    File::open(path)?.read_to_end(&mut file_contents)?;

    let hash = compute_hash(&file_contents);
    if !is_image(path) {
        return Ok((hash, None));
    }

    let exif = read_file_exif_from_bytes(file_contents);

    Ok((hash, exif))
}

fn is_image(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    let mime = MimeGuess::from_ext(ext).first_or_octet_stream();

    mime.type_() == mime::IMAGE
}

fn compute_hash(bytes: &[u8]) -> Vec<u8> {
    let hash = sha2::Sha256::digest(bytes);
    let slice = hash.as_slice();
    slice[..slice.len() / 2].to_vec()
}

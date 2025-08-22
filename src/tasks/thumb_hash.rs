use crate::http::AppStateRef;
use crate::previews::{THUMB_HASH_IMAGE_SIZE, generate_thumb_hash_raw_image};
use fast_thumbhash::ThumbHashEncoder;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

pub async fn generate_thumb_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const CHUNK_SIZE: usize = 256;

    let photos = app_state
        .photos_repo
        .get_photos_without_thumb_hash()
        .await?;

    info!("Computing thumb hashes for {} photos", photos.len());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    spawn_blocking(move || {
        photos.par_chunks(CHUNK_SIZE).for_each(|chunk| {
            let Ok(mut encoder) =
                ThumbHashEncoder::new(THUMB_HASH_IMAGE_SIZE, THUMB_HASH_IMAGE_SIZE)
                    .inspect_err(|e| error!("Failed to create thumb hash encoder: {}", e))
            else {
                return;
            };

            let chunk = chunk
                .iter()
                .filter_map(|photo| {
                    let preview_path = app_state
                        .storage
                        .preview_folder
                        .join(photo.partial_preview_path());

                    if !preview_path.exists() {
                        return None;
                    }

                    match generate_thumb_image_hash(&mut encoder, &preview_path) {
                        Ok(thumb_hash) => Some((photo.id, thumb_hash)),
                        Err(e) => {
                            error!(
                                "Failed to generate thumb hash for photo {}: {}",
                                photo.full_name(),
                                e
                            );
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            if let Err(e) = tx.send(chunk) {
                error!("Failed to send thumb hashes over channel: {e}");
            }
        });

        drop(tx);
    });

    let mut count = 0;

    while let Some(chunk) = rx.recv().await {
        app_state.photos_repo.update_thumb_hashes(&chunk).await?;
        count += chunk.len();
    }

    info!("Updated {} thumb hashes", count);

    Ok(())
}

fn generate_thumb_image_hash(
    encoder: &mut ThumbHashEncoder,
    preview_path: &Path,
) -> Result<Vec<u8>, std::io::Error> {
    let thumb_image = generate_thumb_hash_raw_image(preview_path)?;
    encoder
        .encode_rgba(&thumb_image)
        .map_err(std::io::Error::other)
}

/*#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;

    #[test]
    fn test_generate_thumb_image_hash() {
        let mut encoder =
            ThumbHashEncoder::new(THUMB_HASH_IMAGE_SIZE, THUMB_HASH_IMAGE_SIZE).unwrap();
        let preview_path = Path::new("/home/razvan/Desktop/input.jpg");

        let thumb_hash = generate_thumb_image_hash(&mut encoder, preview_path).unwrap();
        let (width, height, rgba) = thumbhash::thumb_hash_to_rgba(&thumb_hash).unwrap();
        // assert_eq!(thumb_hash.len(), 16);

        // Create a new file to store the output image
        let output_file = "/home/razvan/Desktop/output.png";
        let file = std::fs::File::create(output_file).unwrap();

        // Initialize a PNG encoder and write the output image to the file
        let encoder = PngEncoder::new(file);
        encoder
            .write_image(
                &rgba,
                width as u32,
                height as u32,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap()
    }
}*/

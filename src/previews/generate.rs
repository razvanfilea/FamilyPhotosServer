use std::fs;
use std::io::{self, Read as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use mime_guess::MimeGuess;
use tracing::warn;
use wait_timeout::ChildExt;

const PREVIEW_TARGET_SIZE: u32 = 250;
const VIDEO_PREVIEW_TARGET_SIZE: &str = "500";
pub const THUMB_HASH_IMAGE_SIZE: usize = 72;

const GENERATION_TIMEOUT: Duration = Duration::from_secs(15);
pub const MIN_PREVIEW_SIZE: u64 = 100;

fn read_stderr(child: &mut std::process::Child) -> String {
    child
        .stderr
        .take()
        .map(|mut s| {
            let mut buf = String::new();
            s.read_to_string(&mut buf).ok();
            buf.trim().to_owned()
        })
        .unwrap_or_default()
}

fn generate_video_frame<P: AsRef<Path>, R: AsRef<Path>>(
    load_path: P,
    save_path: R,
) -> io::Result<()> {
    let mut child = Command::new("ffmpegthumbnailer")
        .arg("-i")
        .arg(load_path.as_ref())
        .arg("-o")
        .arg(save_path.as_ref())
        .arg("-s")
        .arg(VIDEO_PREVIEW_TARGET_SIZE)
        .stderr(Stdio::piped())
        .spawn()?;

    match child.wait_timeout(GENERATION_TIMEOUT) {
        Ok(Some(status)) => {
            if !status.success() {
                let stderr = read_stderr(&mut child);
                warn!(
                    "ffmpegthumbnailer failed for {}: exit={}, stderr={}",
                    load_path.as_ref().display(),
                    status,
                    stderr
                );
                return Err(io::Error::other(format!(
                    "ffmpegthumbnailer failed: {}",
                    stderr
                )));
            }
            Ok(())
        }
        Ok(None) => {
            child.kill()?;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ffmpegthumbnailer timeout",
            ))
        }
        Err(e) => {
            child.kill()?;
            Err(e)
        }
    }
}

fn generate_image_preview<P: AsRef<Path>, R: AsRef<Path>>(
    load_path: P,
    save_path: R,
) -> io::Result<()> {
    let mut child = Command::new("magick")
        .arg(load_path.as_ref())
        .arg("-auto-orient")
        .arg("-thumbnail")
        .arg(format!("{PREVIEW_TARGET_SIZE}x{PREVIEW_TARGET_SIZE}^"))
        .arg(save_path.as_ref())
        .stderr(Stdio::piped())
        .spawn()?;

    match child.wait_timeout(GENERATION_TIMEOUT) {
        Ok(Some(status)) => {
            if !status.success() {
                let stderr = read_stderr(&mut child);
                warn!(
                    "ImageMagick failed for {}: exit={}, stderr={}",
                    load_path.as_ref().display(),
                    status,
                    stderr
                );
                return Err(io::Error::other(format!("ImageMagick failed: {}", stderr)));
            }
            Ok(())
        }
        Ok(None) => {
            child.kill()?;
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ImageMagick timeout",
            ))
        }
        Err(e) => {
            child.kill()?;
            Err(e)
        }
    }
}

pub fn generate_preview<P, R>(load_path: P, save_path: R) -> io::Result<()>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    let load_path = load_path.as_ref();
    let save_path = save_path.as_ref();

    // Get parent directory for temp file (same filesystem = atomic rename)
    let preview_dir = save_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "save_path has no parent"))?;

    // Create temp file in same directory
    let temp_file = tempfile::NamedTempFile::new_in(preview_dir)?;
    let temp_path = temp_file.path();

    let mime = MimeGuess::from_path(load_path).first().ok_or_else(|| {
        io::Error::other(format!(
            "Couldn't detect mime type for: {}",
            load_path.display()
        ))
    })?;

    // Generate to temp file
    if mime.type_() == "video" {
        generate_video_frame(load_path, temp_path)?;
    } else {
        generate_image_preview(load_path, temp_path)?;
    }

    // Validate size
    let size = fs::metadata(temp_path)?.len();
    if size < MIN_PREVIEW_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Generated preview too small ({} bytes)", size),
        ));
    }

    // Atomic move to final location
    temp_file.persist(save_path).map_err(|e| e.error)?;

    Ok(())
}

pub fn generate_thumb_hash_raw_image(load_path: &Path) -> io::Result<Vec<u8>> {
    let size = THUMB_HASH_IMAGE_SIZE;
    let child = Command::new("magick")
        .arg(load_path)
        .args([
            "-auto-orient",
            "-resize",
            &format!("{size}x{size}^"),
            "-gravity",
            "center",
            "-extent",
            &format!("{size}x{size}"),
            "-colorspace",
            "sRGB",
            "-depth",
            "8",
            "-define",
            "quantum:format=unsigned",
            "rgba:-", // no alpha channel, 3 bytes per pixel
        ])
        .stdout(Stdio::piped())
        .spawn()?;

    child.wait_with_output().map(|output| output.stdout)
}

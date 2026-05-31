use std::fs;
use std::io::{self, Read as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use mime_guess::MimeGuess;
use tracing::warn;
use wait_timeout::ChildExt;

pub const THUMB_HASH_IMAGE_SIZE: usize = 64;

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

const PREVIEW_SIZE: u32 = 320;
const VIDEO_SCALE_FILTER: &str = "scale='if(gt(iw,ih),-1,320)':'if(gt(iw,ih),320,-1)'";

fn generate_video_frame(load_path: &Path, save_path: &Path) -> io::Result<()> {
    let filter = format!("thumbnail,{VIDEO_SCALE_FILTER}");
    run_ffmpeg_frame(load_path, save_path, &filter)
}

fn generate_video_frame_simple(load_path: &Path, save_path: &Path) -> io::Result<()> {
    run_ffmpeg_frame(load_path, save_path, VIDEO_SCALE_FILTER)
}

fn run_ffmpeg_frame(load_path: &Path, save_path: &Path, video_filter: &str) -> io::Result<()> {
    let mut child = Command::new("ffmpeg")
        .arg("-ss")
        .arg("0")
        .arg("-i")
        .arg(load_path)
        .arg("-vf")
        .arg(video_filter)
        .arg("-frames:v")
        .arg("1")
        .arg("-c:v")
        .arg("libwebp")
        .arg("-quality")
        .arg("75")
        .arg("-y")
        .arg(save_path)
        .stderr(Stdio::piped())
        .spawn()?;

    match child.wait_timeout(GENERATION_TIMEOUT) {
        Ok(Some(status)) => {
            if !status.success() {
                let stderr = read_stderr(&mut child);
                warn!(
                    "ffmpeg failed for {}: exit={}, stderr={}",
                    load_path.display(),
                    status,
                    stderr
                );
                return Err(io::Error::other(format!("ffmpeg failed: {}", stderr)));
            }
            Ok(())
        }
        Ok(None) => {
            child.kill()?;
            Err(io::Error::new(io::ErrorKind::TimedOut, "ffmpeg timeout"))
        }
        Err(e) => {
            child.kill()?;
            Err(e)
        }
    }
}

fn generate_image_preview(load_path: &Path, save_path: &Path) -> io::Result<()> {
    let mut child = Command::new("magick")
        .arg(format!("{}[0]", load_path.display())) // [0] selects first frame for GIFs
        .arg("-auto-orient")
        .arg("-thumbnail")
        .arg(format!("{PREVIEW_SIZE}x{PREVIEW_SIZE}^>"))
        .arg("-quality")
        .arg("75")
        .arg("-define")
        .arg("webp:method=4")
        .arg("-strip")
        .arg(save_path)
        .stderr(Stdio::piped())
        .spawn()?;

    match child.wait_timeout(GENERATION_TIMEOUT) {
        Ok(Some(status)) => {
            if !status.success() {
                let stderr = read_stderr(&mut child);
                warn!(
                    "ImageMagick failed for {}: exit={}, stderr={}",
                    load_path.display(),
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

    // Create temp file in same directory with .webp suffix
    let temp_file = tempfile::Builder::new()
        .suffix(".webp")
        .tempfile_in(preview_dir)?;
    let temp_path = temp_file.path();

    let mime = MimeGuess::from_path(load_path).first().ok_or_else(|| {
        io::Error::other(format!(
            "Couldn't detect mime type for: {}",
            load_path.display()
        ))
    })?;

    if mime.type_() == "video" {
        generate_video_frame(load_path, temp_path)
            .or_else(|_| generate_video_frame_simple(load_path, temp_path))?;
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

pub struct ThumbHashImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub fn generate_thumb_hash_raw_image(load_path: &Path) -> io::Result<ThumbHashImage> {
    let size = THUMB_HASH_IMAGE_SIZE;

    // Get dimensions after resize (fit within box, preserve aspect ratio)
    let dims_output = Command::new("magick")
        .arg(load_path)
        .args([
            "-auto-orient",
            "-resize",
            &format!("{size}x{size}"),
            "-format",
            "%wx%h",
            "info:",
        ])
        .output()?;

    let dims_str = String::from_utf8_lossy(&dims_output.stdout);
    let (w_str, h_str) = dims_str
        .trim()
        .split_once('x')
        .ok_or_else(|| io::Error::other("failed to parse dimensions"))?;
    let width: usize = w_str
        .parse()
        .map_err(|_| io::Error::other("invalid width"))?;
    let height: usize = h_str
        .parse()
        .map_err(|_| io::Error::other("invalid height"))?;

    // Get raw RGB pixels (no alpha needed for photos)
    let child = Command::new("magick")
        .arg(load_path)
        .args([
            "-auto-orient",
            "-resize",
            &format!("{size}x{size}"),
            "-colorspace",
            "sRGB",
            "-depth",
            "8",
            "rgba:-",
        ])
        .stdout(Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output()?;

    Ok(ThumbHashImage {
        width,
        height,
        rgba: output.stdout,
    })
}

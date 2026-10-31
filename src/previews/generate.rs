use std::fs;
use std::io::{self, Read as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use tracing::warn;
use wait_timeout::ChildExt;

use crate::utils::media::is_video;

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
    run_magick(
        load_path,
        save_path,
        // Lets the JPEG decoder scale down while decoding: about twice as fast, and 2x the
        // target size keeps the quality. Other formats ignore it.
        &["-define", "jpeg:size=640x640"],
        &[
            "-auto-orient",
            "-thumbnail",
            &format!("{PREVIEW_SIZE}x{PREVIEW_SIZE}^>"),
            "-quality",
            "75",
            "-define",
            "webp:method=4",
            "-strip",
        ],
    )
}

const SCREEN_SIZE: u32 = 2048;

fn run_magick(
    load_path: &Path,
    save_path: &Path,
    input_args: &[&str],
    args: &[&str],
) -> io::Result<()> {
    let mut child = Command::new("magick")
        .args(input_args)
        .arg(format!("{}[0]", load_path.display())) // [0] selects first frame for GIFs
        .args(args)
        .arg(format!("webp:{}", save_path.display()))
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

/// Writes the output of `generate` to a temporary file next to `save_path`, validates it and
/// atomically moves it into place.
fn generate_atomically(
    save_path: &Path,
    generate: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    // Get parent directory for temp file (same filesystem = atomic rename)
    let preview_dir = save_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "save_path has no parent"))?;
    fs::create_dir_all(preview_dir)?;

    // Create temp file in same directory with .webp suffix
    let temp_file = tempfile::Builder::new()
        .suffix(".webp")
        .tempfile_in(preview_dir)?;

    generate(temp_file.path())?;

    // Validate size
    let size = fs::metadata(temp_file.path())?.len();
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

/// Generates the ~2048px viewer rendition of an image
pub fn generate_screen_rendition(load_path: &Path, save_path: &Path) -> io::Result<()> {
    generate_atomically(save_path, |temp_path| {
        run_magick(
            load_path,
            temp_path,
            &[],
            &[
                "-auto-orient",
                "-resize",
                &format!("{SCREEN_SIZE}x{SCREEN_SIZE}>"),
                "-quality",
                "82",
            ],
        )
    })
}

pub fn generate_preview<P, R>(load_path: P, save_path: R) -> io::Result<()>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    let load_path = load_path.as_ref();

    generate_atomically(save_path.as_ref(), |temp_path| {
        if is_video(&load_path.to_string_lossy()) {
            generate_video_frame(load_path, temp_path)
                .or_else(|_| generate_video_frame_simple(load_path, temp_path))
        } else {
            generate_image_preview(load_path, temp_path)
        }
    })
}

pub struct ThumbHashImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// The pixel data of a PAM image (what `magick ... pam:-` writes) with 4 channels of 8 bits.
fn parse_rgba_pam(pam: &[u8]) -> io::Result<ThumbHashImage> {
    const END_OF_HEADER: &[u8] = b"ENDHDR\n";
    let invalid =
        |what: &str| io::Error::new(io::ErrorKind::InvalidData, format!("invalid PAM: {what}"));

    let header_end = pam
        .windows(END_OF_HEADER.len())
        .position(|window| window == END_OF_HEADER)
        .ok_or_else(|| invalid("no header end"))?
        + END_OF_HEADER.len();
    let header = String::from_utf8_lossy(&pam[..header_end]);
    let field = |name: &str| {
        header
            .lines()
            .find_map(|line| line.strip_prefix(name)?.trim().parse::<usize>().ok())
            .ok_or_else(|| invalid(name))
    };
    let (width, height) = (field("WIDTH")?, field("HEIGHT")?);
    if field("DEPTH")? != 4 {
        return Err(invalid("DEPTH"));
    }

    let rgba = pam
        .get(header_end..header_end + width * height * 4)
        .ok_or_else(|| invalid("truncated pixels"))?;
    Ok(ThumbHashImage {
        width,
        height,
        rgba: rgba.to_vec(),
    })
}

pub fn generate_thumb_hash_raw_image(load_path: &Path) -> io::Result<ThumbHashImage> {
    let size = THUMB_HASH_IMAGE_SIZE;

    // The PAM header carries the dimensions, so one process gives both them and the pixels
    let output = Command::new("magick")
        .arg(load_path)
        .args([
            "-auto-orient",
            "-resize",
            &format!("{size}x{size}"),
            "-colorspace",
            "sRGB",
            "-alpha",
            "set",
            "-depth",
            "8",
            "pam:-",
        ])
        .output()?;

    parse_rgba_pam(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rgba_pam() {
        let header = b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n";
        let pixels = [1, 2, 3, 255, 4, 5, 6, 255];
        let pam = [header.as_slice(), &pixels].concat();

        let image = parse_rgba_pam(&pam).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, pixels);

        let rgb_header = b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 3\nMAXVAL 255\nTUPLTYPE RGB\nENDHDR\n";
        for (name, pam) in [
            ("empty", Vec::new()),
            ("truncated", pam[..pam.len() - 1].to_vec()),
            ("rgb", [rgb_header.as_slice(), &pixels].concat()),
        ] {
            assert!(parse_rgba_pam(&pam).is_err(), "{name}");
        }
    }
}

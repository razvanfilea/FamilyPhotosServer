use std::path::Path;
use std::process::Command;
use std::time::Duration;

use mime_guess::MimeGuess;
use wait_timeout::ChildExt;

const PREVIEW_TARGET_SIZE: u32 = 300;
const VIDEO_PREVIEW_TARGET_SIZE: &str = "500";

fn generate_video_frame<P: AsRef<Path>, R: AsRef<Path>>(
    load_path: P,
    save_path: R,
) -> std::io::Result<()> {
    let mut command = Command::new("ffmpegthumbnailer");
    command
        .arg("-i")
        .arg(load_path.as_ref())
        .arg("-o")
        .arg(save_path.as_ref())
        .arg("-s")
        .arg(VIDEO_PREVIEW_TARGET_SIZE);

    let mut child = command.spawn()?;

    match child.wait_timeout(Duration::from_secs(15)) {
        Ok(status) => status
            .map(|_| ())
            .ok_or_else(|| std::io::Error::other("ffmpegthumbnailer timeout"))?,
        Err(e) => {
            child.kill()?;
            return Err(e);
        }
    }

    Ok(())
}

pub fn generate_preview<P, R>(load_path: P, save_path: R) -> std::io::Result<()>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    let load_path = load_path.as_ref();
    let save_path = save_path.as_ref();

    let mime = MimeGuess::from_path(load_path).first().ok_or_else(|| {
        std::io::Error::other(format!(
            "Couldn't detect mime type for: {}",
            load_path.display()
        ))
    })?;

    if mime.type_() == "video" {
        return generate_video_frame(load_path, save_path);
    }

    let mut child = Command::new("magick")
        .arg(load_path)
        .arg("-auto-orient")
        .arg("-thumbnail")
        .arg(format!("{PREVIEW_TARGET_SIZE}x{PREVIEW_TARGET_SIZE}^"))
        .arg(save_path)
        .spawn()?;

    match child.wait_timeout(Duration::from_secs(10)) {
        Ok(status) => status
            .map(|_| ())
            .ok_or_else(|| std::io::Error::other("ImageMagick timeout"))?,
        Err(e) => {
            child.kill()?;
            return Err(e);
        }
    }

    Ok(())
}

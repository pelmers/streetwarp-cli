use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::options::CLI_OPTIONS;
use crate::progress::progress;

type GetProgress = dyn Fn(usize) -> f64;
pub async fn ffmpeg<P: AsRef<Path>>(working_dir: P, get_progress: &GetProgress, args: &[&str]) {
    let mut command = Command::new("ffmpeg");
    let command = command
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped());
    let mut child = command.spawn().expect("ffmpeg spawn failure");
    let stdout = child.stdout.take().expect("ffmpeg stdout failure");
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    // Ensure the child process is spawned in the runtime so it can
    // make progress on its own while we await for any output.
    let thread = tokio::spawn(async {
        child.await.expect("child process encountered an error");
    });

    while let Some(line) = reader.next_line().await.expect("ffmpeg readline failure") {
        if line.contains("frame=") {
            let frame =
                str::parse::<usize>(&line["frame=".len()..]).expect("Could not parse frame");
            progress(&format!("{:.1}% rendered", get_progress(frame)));
        }
    }
    thread.await.expect("Failed to join ffmpeg thread");
}

pub async fn create_timelapse<P: AsRef<Path>>(image_dir: P, num_images: usize, out_filename: &str) {
    // ffmpeg -framerate 30 -pattern_type glob -i "folder-with-photos/*.JPG" -s:v 1440x1080 -c:v libx264 -crf 25 -pix_fmt yuv420p my-timelapse.mp4
    let pattern = if CLI_OPTIONS.optimizer.is_some() {
        "%d.opt.jpg"
    } else {
        "%d.jpg"
    };
    ffmpeg(
        image_dir,
        &(move |frame| 100.0 * (frame as f64) / (num_images as f64)),
        &[
            "-framerate",
            "24",
            "-pattern_type",
            "sequence",
            "-i",
            pattern,
            "-s:v",
            "640x480",
            "-c:v",
            "libx264",
            "-crf",
            "24",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-movflags",
            "faststart",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

pub async fn blend_timelapse<P: AsRef<Path>>(
    image_dir: P,
    num_images: usize,
    original_filename: &str,
    out_filename: &str,
) {
    // ffmpeg -i streetwarp.mp4-original.mp4 -filter_complex "[0:v]minterpolate=fps=48.0,tblend=all_mode=average,framestep=2[out]" -map "[out]" -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast -progress streetwarp-lapse24_blur.mp4
    ffmpeg(
        image_dir,
        &(move |frame| 100.0 * (frame as f64) / (num_images as f64)),
        &[
            "-i",
            original_filename,
            "-filter_complex",
            "[0:v]minterpolate=fps=48,tblend=all_mode=average,framestep=2[out]",
            "-map",
            "[out]",
            "-c:v",
            "libx264",
            "-crf",
            "24",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-movflags",
            "faststart",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

pub async fn minterp_timelapse<P: AsRef<Path>>(
    image_dir: P,
    num_images: usize,
    original_filename: &str,
    out_filename: &str,
) {
    // ffmpeg -i streetwarp-lapse24.mp4 -filter:v "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=50'" -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast streetwarp-lapse24_flow.mp4
    ffmpeg(
        image_dir,
        &(move |frame| 33.3 * (frame as f64) / (num_images as f64)),
        &[
            "-i",
            original_filename,
            "-filter:v",
            "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=72'",
            "-c:v",
            "libx264",
            "-crf",
            "24",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-movflags",
            "faststart",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

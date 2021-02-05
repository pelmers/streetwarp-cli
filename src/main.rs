#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use gpx::{read, Gpx};

use geo::{prelude::*, Point};

use structopt::StructOpt;

use futures::{stream, StreamExt};
use rayon::prelude::*;
use reqwest::Client;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

// example
// ffmpeg -framerate 30 -pattern_type glob -i "folder-with-photos/*.JPG" -s:v 1440x1080 -c:v libx264 -crf 25 -pix_fmt yuv420p my-timelapse.mp4

#[derive(StructOpt)]
struct Cli {
    /// The path to the file to read, accepts .gpx and .json (format: [{lat, lng}]) files
    #[structopt(parse(from_os_str))]
    input_path: PathBuf,

    /// Key for google streetview static API
    #[structopt(long)]
    api_key: String,

    /// Output location for individual frames. Default: tmp folder
    #[structopt(long)]
    output_dir: Option<String>,

    /// Output filename for timelapse. Default: streetwarp-lapse.mp4
    #[structopt(short, long)]
    output: Option<String>,

    /// Number of network calls to allow at once, default: 40.
    #[structopt(long)]
    network_concurrency: Option<usize>,

    /// Number of frames to search for per mile, default: 100.
    #[structopt(short, long)]
    frames_per_mile: Option<f64>,

    /// Maximum number of frames, default: unlimited (set to 0)
    #[structopt(long)]
    max_frames: Option<usize>,

    /// Don't fetch images or create video, just show metadata and expected error.
    #[structopt(short, long)]
    dry_run: bool,

    /// Print metadata before creating result video (implied if --dry-run)
    #[structopt(long)]
    print_metadata: bool,

    /// Linearly interpolate given number of points between each point in the source file, default: use frames_per_mile.
    #[structopt(long)]
    interp: Option<usize>,

    /// Use motion interpolation to smooth output video. Available: skip, fast, good. Default: good
    #[structopt(long)]
    minterp: Option<String>,

    /// Output in JSON format. Default: off.
    #[structopt(long)]
    json: bool,

    /// Whether to print out progress messages (in JSON) to stdout. Default: off.
    #[structopt(long)]
    progress: bool,
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone, Default, PartialEq)]
struct GSVPoint {
    lat: f64,
    lng: f64,
}

#[derive(Deserialize, Debug, Clone)]
struct GSVMetadata {
    #[serde(default)]
    date: String,

    #[serde(default)]
    location: GSVPoint,

    #[serde(default)]
    pano_id: String,

    #[serde(default)]
    status: String,
}

lazy_static! {
    static ref CLI_OPTIONS: Cli = Cli::from_args();
}

#[derive(Debug, Clone, Copy)]
struct PointBearing {
    point: Point<f64>,
    bearing: f64,
}

#[derive(Serialize, Debug, Clone)]
struct MetadataResult {
    distance: f64,
    frames: usize,
    gpsPoints: Vec<GSVPoint>,
    originalPoints: Vec<GSVPoint>,
    averageError: f64,
}

async fn get_images<P: AsRef<Path>>(point_bearings: &[PointBearing], out_dir: &P) {
    // and to correct points lat/lng
    // and to skip images that are a copy of the previous one
    let url = |point_bearing: &PointBearing| {
        format!(
"https://maps.googleapis.com/maps/api/streetview?size=640x480&location={},{}&fov=120&source=outdoor&heading={}&pitch=0&key={}", point_bearing.point.lat(), point_bearing.point.lng(), point_bearing.bearing, CLI_OPTIONS.api_key)
    };
    let client = Client::new();
    let bodies = stream::iter(point_bearings.iter().map(url).enumerate())
        .map(|(index, url)| {
            let client = &client;
            async move {
                let resp = client.get(&url).send().await;
                (index, resp.unwrap().bytes().await)
            }
        })
        .buffer_unordered(CLI_OPTIONS.network_concurrency.unwrap_or(40));

    bodies
        .for_each(|(index, bytes)| async move {
            let filename = out_dir.as_ref().join(format!("{}.jpg", &index));
            tokio::fs::write(filename, bytes.unwrap()).await.unwrap();
        })
        .await;
}

fn progress(msg: &str) {
    if !CLI_OPTIONS.progress {
        return;
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "PROGRESS",
            "message": msg,
        }))
        .expect("Could not print progress message")
    );
}

fn progress_stage(stage: &str) {
    if !CLI_OPTIONS.progress {
        return;
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "PROGRESS_STAGE",
            "stage": stage,
        }))
        .expect("Could not print progress message")
    );
}

async fn get_metadata(point_bearings: &[PointBearing]) -> Vec<GSVMetadata> {
    // use metadata requests to skip errors https://developers.google.com/maps/documentation/streetview/metadata
    // and to correct points lat/lng
    // and to skip images that are a copy of the previous one
    let url = |point_bearing: &PointBearing| {
        format!(
"https://maps.googleapis.com/maps/api/streetview/metadata?location={},{}&source=outdoor&key={}", point_bearing.point.lat(), point_bearing.point.lng(), CLI_OPTIONS.api_key)
    };
    let client = Client::new();
    let bodies = stream::iter(point_bearings.iter().map(url).enumerate())
        .map(|(index, url)| {
            let client = &client;
            async move {
                let resp = client.get(&url).send().await;
                (index, resp.unwrap().bytes().await)
            }
        })
        .buffer_unordered(CLI_OPTIONS.network_concurrency.unwrap_or(40));

    let mut indexed_metadata = bodies
        .map(|(index, bytes)| {
            let parsed = serde_json::from_slice::<GSVMetadata>(&bytes.unwrap())
                .expect("Could not parse GSV metadata");
            (index, parsed)
        })
        .collect::<Vec<_>>()
        .await;
    indexed_metadata.sort_unstable_by_key(|&(index, _)| index);
    indexed_metadata
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>()
}

type GetProgress = dyn Fn(usize) -> f64;
async fn ffmpeg<P: AsRef<Path>>(working_dir: P, get_progress: &GetProgress, args: &[&str]) {
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

async fn create_timelapse<P: AsRef<Path>>(image_dir: P, num_images: usize, out_filename: &str) {
    // ffmpeg -framerate 30 -pattern_type glob -i "folder-with-photos/*.JPG" -s:v 1440x1080 -c:v libx264 -crf 25 -pix_fmt yuv420p my-timelapse.mp4
    ffmpeg(
        image_dir,
        &(move |frame| 100.0 * (frame as f64) / (num_images as f64)),
        &[
            "-framerate",
            "8",
            "-pattern_type",
            "sequence",
            "-i",
            "%d.jpg",
            "-s:v",
            "640x480",
            "-c:v",
            "libx264",
            "-crf",
            "17",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

async fn blend_timelapse<P: AsRef<Path>>(
    image_dir: P,
    num_images: usize,
    original_filename: &str,
    out_filename: &str,
) {
    // ffmpeg -i streetwarp.mp4-original.mp4 -filter_complex "[0:v]minterpolate=fps=48.0,tblend=all_mode=average,framestep=2[out]" -map "[out]" -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast -progress streetwarp-lapse24_blur.mp4
    ffmpeg(
        image_dir,
        &(move |frame| 33.3 * (frame as f64) / (num_images as f64)),
        // TODO use tmix filter: https://video.stackexchange.com/a/26260
        &[
            "-i",
            original_filename,
            "-filter_complex",
            "[0:v]minterpolate=fps=24,tblend=all_mode=average,framestep=3[out]",
            "-map",
            "[out]",
            "-c:v",
            "libx264",
            "-crf",
            "17",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

async fn minterp_timelapse<P: AsRef<Path>>(
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
            "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=24'",
            "-c:v",
            "libx264",
            "-crf",
            "17",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
            "-progress",
            "pipe:1",
            "-y",
            out_filename,
        ],
    )
    .await;
}

fn group_by_location(
    point_bearings: Vec<PointBearing>,
    metadata: Vec<GSVMetadata>,
) -> (Vec<PointBearing>, Vec<GSVMetadata>, Vec<f64>) {
    let mut grouped_points = vec![vec![]];
    let mut last_pano = None;
    for (point_bearing, meta) in point_bearings
        .into_iter()
        .zip(metadata.into_iter())
        .filter(|(_, metadata)| metadata.status == "OK")
    {
        if let Some(last_pano) = last_pano {
            if last_pano != meta.pano_id {
                grouped_points.push(vec![]);
            }
        }
        let actual_point = point_bearing.point;
        let pano_point = Point::new(meta.location.lng, meta.location.lat);
        let err = actual_point.geodesic_distance(&pano_point);
        let groups = grouped_points.len();

        last_pano = Some(meta.pano_id.clone());
        grouped_points[groups - 1].push((point_bearing, meta, err));
    }
    let best_groups = grouped_points
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .min_by_key(|(_, _, err)| ordered_float::OrderedFloat(*err))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let errs = best_groups.iter().map(|(_, _, e)| *e).collect::<Vec<_>>();
    let (point_bearings, metadata) = best_groups.into_iter().map(|(p, m, _)| (p, m)).unzip();
    (point_bearings, metadata, errs)
}

fn interp_points(points: Vec<Point<f64>>, factor: usize) -> Vec<Point<f64>> {
    if factor < 2 {
        points
    } else {
        points
            .iter()
            .zip(points.iter().skip(1))
            .flat_map(|(p1, p2)| {
                p1.haversine_intermediate_fill(
                    p2,
                    p1.haversine_distance(p2) / (factor as f64),
                    /* include ends */ false,
                )
                .into_iter()
            })
            .collect::<Vec<_>>()
    }
}

fn find_distances(points: &[Point<f64>]) -> Vec<f64> {
    points
        .par_iter()
        .zip(points.par_iter().skip(1))
        .map(|(p1, p2)| p1.geodesic_distance(p2))
        .collect()
}

fn sample_points_by_distance(
    points: &[Point<f64>],
    n: usize,
    distances: &[f64],
) -> Vec<Point<f64>> {
    let total_dist: f64 = distances.iter().sum();
    let step = total_dist / (n as f64 - 0.99);
    let mut current = 0.0;
    let mut idx = 0;
    let mut sample = Vec::with_capacity(n);
    while sample.len() < n && idx < points.len() {
        if current >= step * sample.len() as f64 {
            sample.push(points[idx]);
        }
        // Bounds check necessary since the last point doesn't have a distance to the next.
        if idx < distances.len() {
            current += distances[idx];
        }
        idx += 1
    }
    sample
}

fn find_bearings(points: &[Point<f64>]) -> Vec<PointBearing> {
    let mut results = points
        .par_iter()
        .zip(points.par_iter().skip(1))
        .map(|(p1, p2)| PointBearing {
            point: *p1,
            bearing: p1.bearing(*p2),
        })
        .collect::<Vec<_>>();
    // Assume the direction of the second-to-last point continues to the end.
    let last_point = points[points.len() - 1];
    let last_bearing = results[results.len() - 1].bearing;
    results.push(PointBearing {
        point: last_point,
        bearing: last_bearing,
    });
    results
}

fn read_gpx<R: std::io::Read>(reader: R) -> Vec<Point<f64>> {
    let gpx: Gpx = read(reader).expect("Could not read gpx");
    gpx.tracks
        .par_iter()
        .map(|track| {
            track
                .segments
                .par_iter()
                .map(|segment| {
                    segment.points.par_iter().map(|p| {
                        let val = p.point();
                        Point::new(val.lng(), val.lat())
                    })
                })
                .flatten()
        })
        .flatten()
        .collect::<Vec<_>>()
}

fn read_json<R: std::io::Read>(reader: R) -> Vec<Point<f64>> {
    let points: Vec<GSVPoint> =
        serde_json::from_reader(reader).expect("Could not parse json input");
    points
        .into_iter()
        .map(|gsv| Point::new(gsv.lng, gsv.lat))
        .collect::<Vec<_>>()
}

#[tokio::main]
async fn main() {
    lazy_static::initialize(&CLI_OPTIONS);

    let file = File::open(&CLI_OPTIONS.input_path).unwrap();
    let reader = BufReader::new(file);
    let is_gpx = &CLI_OPTIONS.input_path.extension() == &Some(std::ffi::OsStr::new("gpx"));

    progress_stage("Parsing GPX data");
    progress("Reading GPX file");
    let original_points = if is_gpx {
        read_gpx(reader)
    } else {
        read_json(reader)
    };
    let all_points = original_points.clone();

    progress("Computing distance statistics");
    let distances = find_distances(&all_points);
    let distance = distances.iter().sum::<f64>();
    if !CLI_OPTIONS.json {
        println!("distance is {} with {} points", distance, all_points.len());
    }

    let output_dir = CLI_OPTIONS
        .output_dir
        .as_ref()
        .map(|o| PathBuf::from(o))
        .unwrap_or_else(|| {
            let start = SystemTime::now();
            let now = start
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");
            env::temp_dir().join(format!("streetwarp-tmp-{}", now.as_secs()))
        });
    fs::create_dir_all(&output_dir).expect("Could not open output directory");
    if !CLI_OPTIONS.json {
        println!("output dir is {}", output_dir.to_string_lossy());
    }

    // interpolate extra points to have more closely spaced pictures
    // from my observation it looks like Google can give back up to 300 points per mile
    let expected_frames =
        (CLI_OPTIONS.frames_per_mile.unwrap_or(100.0) * distance / 1600.0) as usize;
    let all_points = interp_points(
        all_points,
        CLI_OPTIONS
            .interp
            .unwrap_or(expected_frames / &distances.len() + 1),
    );
    let distances = find_distances(&all_points);

    progress("Finding viewpoints");
    let points = find_bearings(&sample_points_by_distance(
        &all_points,
        expected_frames,
        &distances,
    ));
    progress_stage("Fetching Streetview metadata");
    let metadata = get_metadata(&points).await;
    let (mut points, metadata, errs) = group_by_location(points, metadata);

    if !CLI_OPTIONS.json {
        println!(
            "distance is {} with {} points",
            distances.iter().sum::<f64>(),
            all_points.len()
        );
        println!("filtered to {} points", points.len());
        println!(
            "average error is {} meters",
            errs.iter().sum::<f64>() / errs.len() as f64
        );
    }
    let gps_points = metadata
        .iter()
        .map(|data| data.location)
        .collect::<Vec<_>>();

    let metadata_result = MetadataResult {
        distance: distances.iter().sum::<f64>(),
        frames: gps_points.len(),
        averageError: errs.iter().sum::<f64>() / errs.len() as f64,
        gpsPoints: gps_points,
        originalPoints: original_points
            .iter()
            .map(|p| GSVPoint {
                lat: p.lat(),
                lng: p.lng(),
            })
            .collect::<Vec<_>>(),
    };
    if CLI_OPTIONS.dry_run || CLI_OPTIONS.print_metadata {
        if CLI_OPTIONS.json {
            println!(
                "{}",
                serde_json::to_string(&metadata_result).expect("Serialization failed")
            );
        } else {
            println!("{:?}", &metadata_result);
        }
        if CLI_OPTIONS.dry_run {
            return;
        }
    }

    if CLI_OPTIONS.max_frames.unwrap_or(0) > 0 {
        points.truncate(CLI_OPTIONS.max_frames.unwrap());
    }
    get_images(&points, &output_dir).await;

    // TODO dynamic program images to remove bigtime outliers (like hyperlapse does)
    // 640 x 480 x 3 = about 1.6 MB per image to keep in memory
    // cost function could be some histogram operation? (maybe use hue?)

    let original_timelapse_name = format!(
        "{}-original.mp4",
        &CLI_OPTIONS
            .output
            .clone()
            .unwrap_or("streetwarp-lapse".to_string())
    );

    progress_stage("Joining images into video sequence");
    create_timelapse(&output_dir, points.len(), &original_timelapse_name).await;
    let output_timelapse_name = &CLI_OPTIONS
        .output
        .clone()
        .unwrap_or("streetwarp-lapse.mp4".to_string());

    match CLI_OPTIONS
        .minterp
        .clone()
        .unwrap_or("good".to_string())
        .as_str()
    {
        "skip" => {
            let result = tokio::fs::rename(&original_timelapse_name, &output_timelapse_name).await;
            result.expect("Could not rename video files");
        }
        "fast" => {
            progress_stage("Blending frames to apply blur");
            blend_timelapse(
                &output_dir,
                points.len(),
                &original_timelapse_name,
                &output_timelapse_name,
            )
            .await
        }
        _ => {
            progress_stage("Interpolating motion to apply blur");
            minterp_timelapse(
                &output_dir,
                points.len(),
                &original_timelapse_name,
                &output_timelapse_name,
            )
            .await
        }
    };

    // TODO optionally stabilize the output
}

// butterr but slow
// ffmpeg -i streetwarp-lapse24.mp4 -filter:v "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=48'" -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast streetwarp-lapse24_flow.mp4
// optional vid stab
// 1. ffmpeg -i streetwarp-lapse24_flow.mp4 -vf vidstabdetect=shakiness=5:accuracy=15 -f null -
// 2. ffmpeg -i streetwarp-lapse24_flow.mp4 -vf vidstabtransform,unsharp=5:5:0.8:3:3:0.4 -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast streetwarp-lapse24_flow_stab.mp4
// TODO by priority
// - most obvious issue is output smoothness
// perhaps we can calculate a zoom/blend motion based on field of view + distance between points, etc.
// or we can pay lots of money at google and with extra data create a hyperlapse
//   - (i tried this, see 'twisty', it doesn't look good on its own)
// hyperlapse example: https://vimeo.com/63653873, I think this video does about 80-100 frames per mile based on golden gate section
//   - they very obviously have some kind of blur thing going on (probably some stabilization too?)
//   - hmm that helps quite a bit actually, https://www.reddit.com/r/ffmpeg/comments/g2isg9/is_motion_blur_effect_possible_with_ffmpeg/fnm9uiz/?utm_source=reddit&utm_medium=web2x&context=3
//   - maybe this would be even better? https://github.com/slowmoVideo/slowmoVideo
//   - I think I still need some heuristic to cut out obviously wrong frames (some kind of DP algorithm)
//      - maybe image hash with some kind of frame skip penalty
// for stabilization, maybe try https://github.com/georgmartius/vid.stab ?
//   - maybe that helped a little bit...
// maybe lowest hanging fruit is to cut out frames that are very out of place in the output

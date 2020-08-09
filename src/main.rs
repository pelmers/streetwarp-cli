#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use gpx::{read, Gpx};

use geo::{prelude::*, Point};

use structopt::StructOpt;

use futures::{stream, StreamExt};
use rayon::prelude::*;
use reqwest::Client;
use tokio::process::Command;

/*
TODO
1. load gpx track file
  1b. support other formats like gpx route, .fit course
2. select points and orientations along gpx track
  2b. use kalman filter to make it more accurate
3. query streetview for pictures of these orientations
https://maps.googleapis.com/maps/api/streetview?size=400x400&location=47.5763831,-122.4211769&fov=80&heading=70&pitch=0&key=YOUR_API_KEY
  3b. maybe add option to query mapillary instead
  3c. maybe make this a separate crate
4. turn into a video by concatenating frames and sending to ffmpeg
  4b. use hyperlapse algo to make it look smoother
  4c. or add some kind of zoom-blur effect to transition between images
*/
// example
// ffmpeg -framerate 30 -pattern_type glob -i "folder-with-photos/*.JPG" -s:v 1440x1080 -c:v libx264 -crf 25 -pix_fmt yuv420p my-timelapse.mp4

#[derive(StructOpt)]
struct Cli {
    /// The path to the file to read
    #[structopt(parse(from_os_str))]
    gpx_path: PathBuf,

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
    frames_per_mile: Option<usize>,

    /// Don't fetch images or create video, just show metadata and expected error.
    #[structopt(short, long)]
    dry_run: bool,

    /// Linearly interpolate given number of points between each point in the source file, default: off.
    #[structopt(long)]
    interp: Option<usize>,

    /// Output in JSON format. Default: off.
    #[structopt(long)]
    json: bool,
}

#[derive(Deserialize, Debug, Copy, Clone, Default, PartialEq)]
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

// TODO hook up to some sort of web app that gets the route from strava

async fn get_images<P: AsRef<Path>>(point_bearings: &[PointBearing], out_dir: &P) {
    // and to correct points lat/lng
    // and to skip images that are a copy of the previous one
    // TODO add api request limits (about 15,000 monthly?)
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

async fn get_metadata(point_bearings: &[PointBearing]) -> Vec<GSVMetadata> {
    // TODO use metadata requests to skip errors https://developers.google.com/maps/documentation/streetview/metadata
    // and to correct points lat/lng
    // and to skip images that are a copy of the previous one
    // TODO add api request limits (about 15,000 monthly?)
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

async fn create_timelapse<P: AsRef<Path>>(image_dir: P) {
    // ffmpeg -framerate 30 -pattern_type glob -i "folder-with-photos/*.JPG" -s:v 1440x1080 -c:v libx264 -crf 25 -pix_fmt yuv420p my-timelapse.mp4
    let args = [
        "-framerate",
        "15",
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
        "-y",
        &format!(
            "{}-original.mp4",
            &CLI_OPTIONS
                .output
                .clone()
                .unwrap_or("streetwarp-lapse".to_string())
        ),
    ];
    eprintln!("ffmpeg {:?}", args);
    let output = Command::new("ffmpeg")
        .args(args.iter())
        .current_dir(image_dir)
        .output()
        .await
        .unwrap();
    eprintln!("out: {}", String::from_utf8(output.stdout).unwrap());
    eprintln!("err: {}", String::from_utf8(output.stderr).unwrap());
}

async fn minterp_timelapse<P: AsRef<Path>>(image_dir: P) {
    // ffmpeg -i streetwarp-lapse24.mp4 -filter:v "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=50'" -c:v libx264 -crf 17 -pix_fmt yuv420p -y -preset ultrafast streetwarp-lapse24_flow.mp4
    let args = [
        "-i",
        &format!(
            "{}-original.mp4",
            &CLI_OPTIONS
                .output
                .clone()
                .unwrap_or("streetwarp-lapse".to_string())
        ),
        "-filter:v",
        "minterpolate='mi_mode=mci:mc_mode=aobmc:vsbmc=1:fps=30'",
        "-c:v",
        "libx264",
        "-crf",
        "17",
        "-pix_fmt",
        "yuv420p",
        "-preset",
        "ultrafast",
        "-y",
        &CLI_OPTIONS
            .output
            .clone()
            .unwrap_or("streetwarp-lapse.mp4".to_string()),
    ];
    eprintln!("ffmpeg {:?}", args);
    let output = Command::new("ffmpeg")
        .args(args.iter())
        .current_dir(image_dir)
        .output()
        .await
        .unwrap();
    eprintln!("out: {}", String::from_utf8(output.stdout).unwrap());
    eprintln!("err: {}", String::from_utf8(output.stderr).unwrap());
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
                // technically this changes the paths just a little bit? since 1 lat != 1 lng
                // TODO make it correct by taking into account bearings
                let x_step = (p2.lng() - p1.lng()) / (factor as f64);
                let y_step = (p2.lat() - p1.lat()) / (factor as f64);
                (0..factor).map(move |i| {
                    Point::new(
                        p1.lng() + x_step * (i as f64),
                        p1.lat() + y_step * (i as f64),
                    )
                })
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

#[tokio::main]
async fn main() {
    lazy_static::initialize(&CLI_OPTIONS);

    let file = File::open(&CLI_OPTIONS.gpx_path).unwrap();
    let reader = BufReader::new(file);

    // read takes any io::Read and gives a Result<Gpx, Error>.
    let gpx: Gpx = read(reader).unwrap();
    let all_points = gpx
        .tracks
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
        .collect::<Vec<_>>();

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
    println!("output dir is {}", output_dir.to_string_lossy());

    // interpolate extra points to have more closely spaced pictures
    // from my observation it looks like Google can give back up to 300 points per mile
    let expected_frames =
        (CLI_OPTIONS.frames_per_mile.unwrap_or(100) as f64 * distance / 1600.0) as usize;
    let all_points = interp_points(
        all_points,
        CLI_OPTIONS
            .interp
            .unwrap_or(expected_frames / &distances.len() + 1),
    );
    let distances = find_distances(&all_points);

    let points = find_bearings(&sample_points_by_distance(
        &all_points,
        expected_frames,
        &distances,
    ));
    let metadata = get_metadata(&points).await;
    let (points, metadata, errs) = group_by_location(points, metadata);

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
    if CLI_OPTIONS.dry_run {
        for (index, (point, meta)) in points.iter().zip(metadata.iter()).enumerate() {
            let expected = point.point;
            if meta.status == "OK" {
                let actual = Point::new(meta.location.lng, meta.location.lat);
                let err = actual.geodesic_distance(&expected);
                println!("{},{},{},{}", index, err, meta.date, meta.pano_id);
            }
        }
        return;
    }
    // TODO create line and put it on a google map?
    // or an open street map... https://leafletjs.com/reference-1.7.1.html#polyline

    get_images(&points, &output_dir).await;

    // TODO dynamic program images to remove bigtime outliers (like hyperlapse does)
    // cost function could be some histogram operation? (maybe use hue?)

    create_timelapse(&output_dir).await;

    minterp_timelapse(&output_dir).await;

    // TODO optionally stabilize the output
    // TODO json output

    // TODO in javascript animate map position while video plays!
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
// next step: visualize the error between our gps point and google's provided streetview point (can use metadata request for this)

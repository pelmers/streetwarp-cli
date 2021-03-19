use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Cli {
    /// The path to the file to read, accepts .gpx and .json (format: [{lat, lng}]) files
    #[structopt(parse(from_os_str))]
    pub input_path: PathBuf,

    /// Key for google streetview static API
    #[structopt(long)]
    pub api_key: String,

    /// Output location for individual frames. Default: tmp folder
    #[structopt(long)]
    pub output_dir: Option<String>,

    /// Output filename for timelapse. Default: streetwarp-lapse.mp4
    #[structopt(short, long)]
    pub output: Option<String>,

    /// Number of network calls to allow at once, default: 40.
    #[structopt(long)]
    pub network_concurrency: Option<usize>,

    /// Number of frames to search for per mile, default: 100.
    #[structopt(short, long)]
    pub frames_per_mile: Option<f64>,

    /// Maximum number of frames, default: unlimited (set to 0)
    #[structopt(long)]
    pub max_frames: Option<usize>,

    /// Don't fetch images or create video, just show metadata and expected error.
    #[structopt(short, long)]
    pub dry_run: bool,

    /// Print metadata before creating result video (implied if --dry-run)
    #[structopt(long)]
    pub print_metadata: bool,

    /// Linearly interpolate given number of points between each point in the source file, default: use frames_per_mile.
    #[structopt(long)]
    pub interp: Option<usize>,

    /// Use motion interpolation to smooth output video. Available: skip, fast, good. Default: good
    #[structopt(long)]
    pub minterp: Option<String>,

    /// Output in JSON format. Default: off.
    #[structopt(long)]
    pub json: bool,

    /// Whether to print out progress messages (in JSON) to stdout. Default: off.
    #[structopt(long)]
    pub progress: bool,

    /// The path to the image optimization executable file.
    #[structopt(long, parse(from_os_str))]
    pub optimizer: Option<PathBuf>,

    /// Additional argument to pass to optimization executable (after output folder)
    #[structopt(long)]
    pub optimizer_arg: Option<String>,
}

lazy_static! {
    pub static ref CLI_OPTIONS: Cli = Cli::from_args();
}
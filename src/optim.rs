use std::path::Path;

use futures::{stream, StreamExt};
use palette::Srgb;
use rayon::prelude::*;

const LOOKAHEAD: usize = 3;
const SKIP_PENALTY: f64 = 0.1;

// TODO dynamic program images to remove bigtime outliers (like hyperlapse does)
// 640 x 480 x 3 = about 1.6 MB per image to keep in memory
// cost function could be some histogram operation? (maybe use hue?)
// idea: load all the images in memory, record their histogram/hashes, then do DP
// then go thru the filesystem and perform lots of renames/unlinks
// then also adjust metadata result to remove the removed ones
pub async fn optimize_sequence<P: AsRef<Path>>(image_dir: &P, n_images: usize) {
    let image_files = (0..n_images)
        .map(|i| image_dir.as_ref().join(format!("{}.jpg", &i)))
        .collect::<Vec<_>>();
    let hashes = image_files
        .par_iter()
        .map(|filename| {
            hash(
                &image::open(filename)
                    .expect(&format!("Could not open {:?}", filename))
                    .into_rgb8(),
            )
        })
        .collect::<Vec<_>>();
    let kept_indices = dp(hashes);

    stream::iter(kept_indices.iter().enumerate())
        .for_each(|(to, from)| async move {
            let from_filename = image_dir.as_ref().join(format!("{}.jpg", &from));
            let to_filename = image_dir.as_ref().join(format!("{}.opt.jpg", &to));
            let res = tokio::fs::copy(&from_filename, &to_filename).await;
            res.expect(&format!(
                "Could not copy {:?} to {:?}",
                &from_filename, &to_filename
            ));
        })
        .await;
}

fn hash(img: &image::RgbImage) -> Vec<f64> {
    let scale = img.width() * img.height() * 255;
    // TODO improve this "hashing algo" :)
    (0..3)
        .map(|channel| img.pixels().map(|p| p[channel] as f64 / scale as f64).sum())
        .collect::<Vec<f64>>()
}

fn cost(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(p, q)| (p - q).abs()).sum::<f64>() / (a.len() as f64)
}

fn dp(hashes: Vec<Vec<f64>>) -> Vec<usize> {
    // Constructs costs vec which maps each image to the lowest cost it can be used.
    let mut costs: Vec<f64> = Vec::with_capacity(hashes.len());
    let mut prevs: Vec<usize> = Vec::with_capacity(hashes.len());
    for (i, hash) in hashes.iter().enumerate() {
        let lb = std::cmp::max(0, (i as i32) - (LOOKAHEAD as i32)) as usize;
        let (cost, prev) = (lb..i)
            .map(|candidate_index| {
                (
                    costs[candidate_index]
                        + cost(&hashes[candidate_index], hash)
                        + (i - candidate_index - 1) as f64 * SKIP_PENALTY,
                    candidate_index,
                )
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).expect("Cannot comapare NaN"))
            .unwrap_or((0.0, 0));
        costs.push(cost);
        prevs.push(prev);
    }
    let mut new_indices: Vec<usize> = Vec::with_capacity(hashes.len());
    let mut next = hashes.len() - 1;
    while next > 0 {
        new_indices.push(next);
        next = prevs[next];
    }
    new_indices.push(0);
    new_indices.reverse();
    let skipped = (0..hashes.len())
        .filter(|i| !new_indices.contains(i))
        .collect::<Vec<_>>();
    println!("skipped!: {:?}", skipped);
    new_indices
}

use std::path::Path;

use futures::{stream, StreamExt};
use img_hash::{HashAlg, HasherConfig, ImageHash};
use rayon::prelude::*;

const LOOKAHEAD: usize = 3;
const SKIP_PENALTY: f64 = 0.3;

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

fn hash(img: &image::RgbImage) -> ImageHash {
    let hasher = HasherConfig::new()
        .hash_size(16, 16)
        .hash_alg(HashAlg::Blockhash)
        .to_hasher();
    hasher.hash_image(img)
    // TODO improve this "hashing algo" :)
    // let scale = img.width() * img.height() * 255;
    // (0..3)
    // .map(|channel| img.pixels().map(|p| p[channel] as f64 / scale as f64).sum())
    // .collect::<Vec<f64>>()
}

fn cost(a: &ImageHash, b: &ImageHash) -> f64 {
    let scale = (8 * a.as_bytes().len()) as f64;
    a.dist(b) as f64 / scale
}

fn dp(hashes: Vec<ImageHash>) -> Vec<usize> {
    // Constructs costs vec which maps each image to the lowest cost it can be used.
    let mut costs: Vec<f64> = Vec::with_capacity(hashes.len());
    let mut prevs: Vec<usize> = Vec::with_capacity(hashes.len());
    for (i, hash) in hashes.iter().enumerate() {
        let lb = std::cmp::max(0, (i as i32) - (LOOKAHEAD as i32)) as usize;
        let (cost, prev) = (lb..i)
            .map(|candidate_index| {
                (
                    costs[candidate_index]
                        + cost(hash, &hashes[candidate_index])
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
    /*
    let skipped = (0..hashes.len())
        .filter(|i| !new_indices.contains(i))
        .collect::<Vec<_>>();
    println!("costs! {:?}", costs);
    println!("skipped {} ! {:?}", skipped.len(), skipped);
    */
    new_indices
}

use fs_extra::dir::get_dir_content;
use std::path::Path;
use tokio::process::Command;

use crate::options::CLI_OPTIONS;
use futures::{stream, StreamExt};

pub async fn optimize_sequence<P: AsRef<Path>>(image_dir: &P) -> Vec<usize> {
    let optimizer_cmd = CLI_OPTIONS.optimizer.clone().unwrap();
    let mut args = vec![image_dir
        .as_ref()
        .to_str()
        .expect("Could not stringify image_dir")
        .to_string()];
    if let Some(arg) = CLI_OPTIONS.optimizer_arg.clone() {
        args.push(arg)
    }
    let mut command = Command::new(optimizer_cmd);
    let command = command.args(args);
    let output = (command.output().await).expect("Failed to get optimizer output");
    if !output.stderr.is_empty() {
        eprintln!(
            "optimizer stderr: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        );
    }
    if !output.status.success() {
        eprintln!("optimizer exit code {:?}", output.status.code());
        return vec![];
    }
    let kept_indices: Vec<usize> =
        serde_json::from_str(std::str::from_utf8(&output.stdout).expect("Output was not utf8"))
            .unwrap();

    stream::iter(kept_indices.iter().enumerate())
        .for_each(|(to, from)| async move {
            let from_filename = image_dir.as_ref().join(format!("{}.jpg", &from));
            let to_filename = image_dir.as_ref().join(format!("{}.opt.jpg", &to));
            let res = tokio::fs::rename(&from_filename, &to_filename).await;
            if !res.is_ok() {
                let dir_files = get_dir_content(&image_dir)
                    .expect(&format!(
                        "Could not get contents of {:?}",
                        image_dir.as_ref()
                    ))
                    .files;
                eprintln!(
                    "file operation error detected, current folder contents are {:?}",
                    &dir_files
                );
            }
            res.expect(&format!(
                "Could not move {:?} to {:?}",
                &from_filename, &to_filename
            ));
        })
        .await;
    kept_indices
}

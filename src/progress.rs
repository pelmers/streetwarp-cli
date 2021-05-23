use serde_json::json;

use crate::options::CLI_OPTIONS;

pub fn progress(msg: &str) {
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

pub fn progress_stage(stage: &str) {
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

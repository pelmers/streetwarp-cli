use serde_json::json;
use std::sync::Mutex;

use crate::options::CLI_OPTIONS;

const PROGRESS_DEBOUNCE_MS: u128 = 200;

lazy_static! {
    static ref LAST_PROGRESS_TIME: Mutex<u128> = Mutex::new(0);
}

pub fn progress(msg: &str) {
    if !CLI_OPTIONS.progress {
        return;
    }
    // If last progress time + debounce < current time, then skip
    {
        // Start new context so we can drop the lock before printing
        let mut last_progress_time = LAST_PROGRESS_TIME.lock().unwrap();
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        if *last_progress_time + PROGRESS_DEBOUNCE_MS > current_time {
            return;
        }
        *last_progress_time = current_time;
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
    {
        // Reset the last progress time to 0
        let mut last_progress_time = LAST_PROGRESS_TIME.lock().unwrap();
        *last_progress_time = 0;
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

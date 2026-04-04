mod real_data;

use std::path::PathBuf;
use real_data::evaluate_real_data;

fn main() {
    let testdata = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");

    let wavs = [
        "191111_110130.wav",
        "191111_110200.wav",
    ];

    let mut total_decoded = 0usize;
    let mut any_found = false;

    for name in &wavs {
        let path = testdata.join(name);
        if !path.exists() {
            println!("SKIP {name} (not found — download from https://github.com/jl1nie/RustFT8/tree/main/data)");
            continue;
        }
        any_found = true;
        match evaluate_real_data(&path) {
            Ok(report) => {
                total_decoded += report.messages.len();
                report.print();
            }
            Err(e) => eprintln!("ERROR {name}: {e}"),
        }
    }

    if any_found {
        println!("Total decoded across all files: {total_decoded}");
    }
}

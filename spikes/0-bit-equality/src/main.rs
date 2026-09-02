use std::env;
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};

use bit_equality_spike::{corpus, probe_at};

fn main() {
    let count: u64 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(corpus::COUNT);

    create_dir_all("results").expect("create results/");
    let file = File::create("results/native.bits").expect("create results/native.bits");
    let mut out = BufWriter::new(file);

    for i in 0..count {
        writeln!(out, "{:016x}", probe_at(i).to_bits()).expect("write");
    }
    out.flush().expect("flush");
    eprintln!("wrote {} samples to results/native.bits", count);
}

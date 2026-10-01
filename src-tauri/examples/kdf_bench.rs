//! Argon2id 默认参数（m=64MB, t=3, p=4）KEK 派生耗时基准。
//! 运行：cargo run --release --example kdf_bench

use std::time::Instant;
use tenjee_vault_lib::crypto::kdf::{derive_kek, generate_salt, KdfParams};

fn main() {
    let salt = generate_salt();
    let params = KdfParams::default();
    // 预热一次（分配内存页等）
    let _ = derive_kek("warmup", &salt, &params).unwrap();
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let _ = derive_kek("benchmark-password", &salt, &params).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let min = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let avg = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "KdfParams: m={} t={} p={}",
        params.m_cost, params.t_cost, params.p_cost
    );
    println!(
        "runs (ms): {}",
        times
            .iter()
            .map(|t| format!("{t:.0}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("min: {min:.0} ms, avg: {avg:.0} ms");
}

use anyhow::Result;
use mini_kv::{Engine, SyncMode};  // 把 mini-lsm 改成 mini_kv
use std::time::Duration;

fn main() -> Result<()> {
    println!("=== Minimal KV: fsync strategy demonstration ===\n");
    
    // 演示不同sync模式
    let modes = vec![
        ("Always fsync", SyncMode::Always),
        ("Batch (100 writes)", SyncMode::Batch(100)),
        ("Periodic (10ms)", SyncMode::Periodic(Duration::from_millis(10))),
    ];
    
    for (name, mode) in modes {
        println!("Testing mode: {}", name);
        
        let path = format!("test_{}.db", name.replace(' ', "_"));
        let mut engine = Engine::with_sync(&path, mode)?;
        
        // 写入少量数据做演示
        let start = std::time::Instant::now();
        for i in 0..1000 {
            let key = format!("key{}", i).into_bytes();
            let value = vec![0u8; 128];
            engine.put(key, value)?;
        }
        let duration = start.elapsed();
        
        println!("  Wrote 1000 records in {:?}", duration);
        println!("  Throughput: {:.2} ops/sec", 1000.0 / duration.as_secs_f64());
        println!();
    }
    
    println!("Run `cargo run --bin bench` for detailed benchmarks");
    Ok(())
}
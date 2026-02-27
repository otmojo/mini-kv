use anyhow::Result;
use mini_kv::{Engine, SyncMode};
use std::time::{Duration, Instant};
use std::fs;

struct BenchConfig {
    name: String,
    sync_mode: SyncMode,
    record_size: usize,
    count: usize,
}

struct BenchResult {
    throughput: f64,
    total_time: Duration,
    latencies: Vec<Duration>,
}

fn run_bench(config: &BenchConfig) -> Result<BenchResult> {
    let path = format!("bench_{}.db", config.name);
    
    // 清理旧文件
    let _ = fs::remove_file(&path);
    
    let mut engine = Engine::with_sync(&path, config.sync_mode)?;
    
    // 预热
    for i in 0..1000 {
        let key = format!("warmup_{}", i).into_bytes();
        let value = vec![0u8; config.record_size];
        engine.put(key, value)?;
    }
    
    let mut latencies = Vec::with_capacity(config.count);
    
    let start = Instant::now();
    
    for i in 0..config.count {
        let key = format!("key{}", i).into_bytes();
        // 用固定值代替随机值
        let value = vec![(i % 256) as u8; config.record_size];
        
        let op_start = Instant::now();
        engine.put(key, value)?;
        latencies.push(op_start.elapsed());
    }
    
    let total_time = start.elapsed();
    
    // 强制sync剩余数据
    engine.sync()?;
    
    Ok(BenchResult {
        throughput: config.count as f64 / total_time.as_secs_f64(),
        total_time,
        latencies,
    })
}

fn percentile(latencies: &[Duration], p: f64) -> Duration {
    if latencies.is_empty() {
        return Duration::from_nanos(0);
    }
    
    let mut sorted: Vec<_> = latencies.iter().map(|d| d.as_nanos()).collect();
    sorted.sort_unstable();
    
    let idx = (p * sorted.len() as f64).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    
    Duration::from_nanos(sorted[idx] as u64)
}

fn main() -> Result<()> {
    println!("mode,record_size,count,total_time_ms,throughput,p50_ns,p99_ns,p999_ns");
    
    let configs = vec![
        ("always_128b", SyncMode::Always, 128, 10_000),
        ("batch100_128b", SyncMode::Batch(100), 128, 10_000),
        ("batch1000_128b", SyncMode::Batch(1000), 128, 10_000),
        ("periodic_10ms", SyncMode::Periodic(Duration::from_millis(10)), 128, 10_000),
        ("periodic_100ms", SyncMode::Periodic(Duration::from_millis(100)), 128, 10_000),
    ];
    
    for (name, mode, size, count) in configs {
        let config = BenchConfig {
            name: name.to_string(),
            sync_mode: mode,
            record_size: size,
            count,
        };
        
        match run_bench(&config) {
            Ok(result) => {
                let p50 = percentile(&result.latencies, 0.5);
                let p99 = percentile(&result.latencies, 0.99);
                let p999 = percentile(&result.latencies, 0.999);
                
                println!(
                    "{},{},{},{:.2},{:.2},{},{},{}",
                    name,
                    size,
                    count,
                    result.total_time.as_millis(),
                    result.throughput,
                    p50.as_nanos(),
                    p99.as_nanos(),
                    p999.as_nanos(),
                );
            }
            Err(e) => {
                eprintln!("Error running {}: {}", name, e);
            }
        }
    }
    
    Ok(())
}
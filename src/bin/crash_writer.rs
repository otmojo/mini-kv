use anyhow::Result;
use mini_kv::{Engine, SyncMode};
use std::env;
use std::time::Duration;

const DB_PATH: &str = "crash_test.db";
const TOTAL_WRITES: usize = 10_000;

/// Child process that writes data until killed by parent
fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: crash_writer <mode> <run_id>");
        std::process::exit(1);
    }
    
    let mode_str = &args[1];
    let run_id = args[2].parse::<usize>().unwrap();
    
    // Enable progress reporting for parent
    env::set_var("CRASH_TEST", "1");
    
    // Parse sync mode from command line
    let sync_mode = match mode_str.as_str() {
        "always" => SyncMode::Always,
        s if s.starts_with("batch:") => {
            let n = s[6..].parse::<usize>().unwrap();
            SyncMode::Batch(n)
        },
        s if s.starts_with("periodic:") => {
            let ms = s[9..].parse::<u64>().unwrap();
            SyncMode::Periodic(Duration::from_millis(ms))
        },
        _ => {
            eprintln!("Unknown mode: {}", mode_str);
            std::process::exit(1);
        }
    };
    
    // Open engine and start writing
    let mut engine = Engine::with_sync(DB_PATH, sync_mode)?;
    
    for i in 0..TOTAL_WRITES {
        let key = format!("key_{}_{}", run_id, i).into_bytes();
        let value = vec![i as u8; 128];
        engine.put(key, value)?;
    }
    
    for i in 0..10_000 {
        // ... engine.put ...
        
        // 关键：每写 10 条睡 200 微秒，确保父进程能抓到正在运行的它
        if i % 10 == 0 {
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
}
    // Normal shutdown: final sync
    engine.sync()?;
    Ok(())
}
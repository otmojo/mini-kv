use anyhow::Result;
use mini_kv::{Engine, SyncMode};
use std::fs;
use std::process::{Command, Child};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;

const DB_PATH: &str = "crash_test.db";
const PROGRESS_FILE: &str = "durable_progress.txt";
const TOTAL_WRITES: usize = 10_000;

// 1. 确保结构体定义包含所有需要的字段
#[derive(Debug, Clone)]
struct CrashResult {
    mode: String,
    runs: usize,
    crash_point: usize,
    recovered: usize,
    lost: usize,
    min_recovered: usize,
    max_recovered: usize,
    max_lost: usize,
}

// 2. 修复后的聚合函数，处理所有统计字段
fn aggregate_results(results: Vec<CrashResult>) -> CrashResult {
    let runs = results.len();
    if runs == 0 {
        return CrashResult {
            mode: "unknown".into(), runs: 0, crash_point: 0, recovered: 0, 
            lost: 0, min_recovered: 0, max_recovered: 0, max_lost: 0,
        };
    }
    
    let mode = results[0].mode.clone();
    let avg_crash = results.iter().map(|r| r.crash_point).sum::<usize>() / runs;
    let avg_recovered = results.iter().map(|r| r.recovered).sum::<usize>() / runs;
    let avg_lost = results.iter().map(|r| r.lost).sum::<usize>() / runs;
    
    let min_rec = results.iter().map(|r| r.recovered).min().unwrap_or(0);
    let max_rec = results.iter().map(|r| r.recovered).max().unwrap_or(0);
    let m_lost = results.iter().map(|r| r.lost).max().unwrap_or(0);

    CrashResult {
        mode,
        runs,
        crash_point: avg_crash,
        recovered: avg_recovered,
        lost: avg_lost,
        min_recovered: min_rec,
        max_recovered: max_rec,
        max_lost: m_lost,
    }
}

fn wait_for_durable_progress(target: usize, timeout: Duration) -> Result<usize> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(content) = fs::read_to_string(PROGRESS_FILE) {
            if let Ok(progress) = content.trim().parse::<usize>() {
                if progress >= target {
                    return Ok(progress);
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    Err(anyhow::anyhow!("Timeout waiting for durable progress {}", target))
}

fn run_crash_test(mode: SyncMode, runs: usize) -> Result<Vec<CrashResult>> {
    let mut results = Vec::new();
    let mut rng = rand::thread_rng();
    
    let mode_display = match mode {
        SyncMode::Always => "always".to_string(),
        SyncMode::Batch(n) => format!("batch_{}", n),
        SyncMode::Periodic(d) => format!("periodic_{}ms", d.as_millis()),
    };
    
    println!("Testing {} mode ({} runs)...", mode_display, runs);
    
    for run in 0..runs {
        let _ = fs::remove_file(DB_PATH);
        let _ = fs::remove_file(PROGRESS_FILE);
        
        let crash_point = rng.gen_range(2000..8000);
        let mut child = spawn_writer(&mode, run)?;
        
        match wait_for_durable_progress(crash_point, Duration::from_secs(10)) {
            Ok(durable_at) => {
                child.kill().ok();
                let _ = child.wait();
                
                thread::sleep(Duration::from_millis(50));
                
                // 3. 校验逻辑修复：处理 recovered > durable_at 的情况
                match verify_data(run) {
                    Ok(recovered) => {
                        // 如果实际恢复的大于进度标记，则丢失为0，不报错
                        let lost = if recovered >= durable_at { 0 } else { durable_at - recovered };
                        
                        results.push(CrashResult {
                            mode: mode_display.clone(),
                            runs: 1,
                            crash_point: durable_at,
                            recovered,
                            lost,
                            min_recovered: recovered,
                            max_recovered: recovered,
                            max_lost: lost,
                        });
                        print!(".");
                    }
                    Err(e) => println!("\nCorruption: {}", e),
                }
            }
            Err(_) => { child.kill().ok(); }
        }
    }
    println!(" Done.");
    Ok(results)
}

fn spawn_writer(mode: &SyncMode, run: usize) -> Result<Child> {
    let mode_arg = match mode {
        SyncMode::Always => "always".to_string(),
        SyncMode::Batch(n) => format!("batch:{}", n),
        SyncMode::Periodic(d) => format!("periodic:{}", d.as_millis()),
    };
    
    let mut cmd = Command::new("target/debug/crash_writer.exe");
    cmd.arg(mode_arg).arg(run.to_string());
    Ok(cmd.spawn()?)
}

fn verify_data(run: usize) -> Result<usize> {
    let engine = Engine::open(DB_PATH)?;
    let mut count = 0;
    for i in 0..TOTAL_WRITES {
        let key = format!("key_{}_{}", run, i).into_bytes();
        if engine.contains_key(&key) { count = i + 1; } else { break; }
    }
    Ok(count)
}

fn main() -> Result<()> {
    println!("=== Mini-KV Crash Consistency Lab ===\n");
    Command::new("cargo").args(&["build", "--bin", "crash_writer"]).status()?;
    
    let modes = vec![
        SyncMode::Always,
        SyncMode::Batch(100),
        SyncMode::Periodic(Duration::from_millis(100)),
    ];
    
    println!("\n{:<15} {:>6} {:>12} {:>12} {:>12} {:>10} {:>10} {:>10}",
             "Mode", "Runs", "Avg Durable", "Avg Recov", "Avg Lost", "Min Rec", "Max Rec", "Max Lost");
    println!("{:-<95}", "");
    
    for mode in modes {
        let results = run_crash_test(mode, 10)?;
        let agg = aggregate_results(results);
        println!("{:<15} {:>6} {:>12} {:>12} {:>12} {:>10} {:>10} {:>10}",
                 agg.mode, agg.runs, agg.crash_point, agg.recovered, agg.lost, 
                 agg.min_recovered, agg.max_recovered, agg.max_lost);
    }
    Ok(())
}
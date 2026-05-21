use std::env;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use blockchain::network::NetworkMessage;
use blockchain::transaction::Transaction;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::sleep;

#[derive(Clone, Copy, Debug)]
enum LoadMode {
    Ping,
    Tx,
}

#[derive(Clone, Debug)]
struct Config {
    target: String,
    duration_secs: u64,
    concurrency: usize,
    mode: LoadMode,
    rps_limit: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct RunStats {
    attempted: u64,
    succeeded: u64,
    failed_connect: u64,
    failed_write: u64,
    avg_latency_ms: f64,
    achieved_msg_per_sec: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if has_flag(&args, "--help") || has_flag(&args, "-h") {
        print_help();
        return Ok(());
    }

    let target = arg_value(&args, "--target").unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let mode = match arg_value(&args, "--mode").as_deref() {
        Some("tx") => LoadMode::Tx,
        _ => LoadMode::Ping,
    };

    if has_flag(&args, "--find-max") {
        let min_c = parse_u64(&args, "--min-concurrency", 10) as usize;
        let max_c = parse_u64(&args, "--max-concurrency", 500) as usize;
        let step = parse_u64(&args, "--step", 10) as usize;
        let step_duration = parse_u64(&args, "--step-duration", 15);
        let success_threshold = parse_f64(&args, "--success-threshold", 0.99);
        let rps_limit = arg_value(&args, "--rps").and_then(|s| s.parse::<u64>().ok());

        if min_c == 0 || max_c < min_c || step == 0 {
            eprintln!("Invalid sweep settings. Check --min-concurrency/--max-concurrency/--step.");
            std::process::exit(2);
        }

        println!(
            "Running max-finder sweep: target={}, mode={:?}, range={}..={} step={} step_duration={}s",
            target, mode, min_c, max_c, step, step_duration
        );

        let mut best: Option<(usize, RunStats)> = None;

        let mut c = min_c;
        while c <= max_c {
            let cfg = Config {
                target: target.clone(),
                duration_secs: step_duration,
                concurrency: c,
                mode,
                rps_limit,
            };

            println!("\n=== Sweep run: concurrency={} ===", c);
            let stats = run_once(cfg).await;
            print_summary(stats);

            let success_rate = if stats.attempted == 0 {
                0.0
            } else {
                stats.succeeded as f64 / stats.attempted as f64
            };

            if success_rate >= success_threshold {
                match best {
                    Some((_, prev)) if prev.achieved_msg_per_sec >= stats.achieved_msg_per_sec => {}
                    _ => best = Some((c, stats)),
                }
            }

            c = c.saturating_add(step);
            if c == usize::MAX {
                break;
            }
        }

        println!("\n=== Sweep result ===");
        match best {
            Some((concurrency, stats)) => {
                println!(
                    "Best sustainable (success >= {:.2}%): concurrency={} achieved_msg_per_sec={:.2}",
                    success_threshold * 100.0,
                    concurrency,
                    stats.achieved_msg_per_sec
                );
            }
            None => {
                println!(
                    "No run met success threshold {:.2}%. Try lower load or lower threshold.",
                    success_threshold * 100.0
                );
            }
        }

        return Ok(());
    }

    let cfg = Config {
        target,
        duration_secs: parse_u64(&args, "--duration", 30),
        concurrency: parse_u64(&args, "--concurrency", 100) as usize,
        mode,
        rps_limit: arg_value(&args, "--rps").and_then(|s| s.parse::<u64>().ok()),
    };

    if cfg.concurrency == 0 || cfg.duration_secs == 0 {
        eprintln!("--concurrency and --duration must be > 0");
        std::process::exit(2);
    }

    println!(
        "Starting load test: target={}, mode={:?}, duration={}s, concurrency={}, rps_limit={:?}",
        cfg.target, cfg.mode, cfg.duration_secs, cfg.concurrency, cfg.rps_limit
    );

    let stats = run_once(cfg).await;
    print_summary(stats);

    Ok(())
}

async fn run_once(cfg: Config) -> RunStats {
    let attempted = Arc::new(AtomicU64::new(0));
    let succeeded = Arc::new(AtomicU64::new(0));
    let failed_connect = Arc::new(AtomicU64::new(0));
    let failed_write = Arc::new(AtomicU64::new(0));
    let total_latency_ns = Arc::new(AtomicU64::new(0));

    let stop = Arc::new(AtomicBool::new(false));
    let end_at = Instant::now() + Duration::from_secs(cfg.duration_secs);
    let tx_counter = Arc::new(AtomicU64::new(1));

    let report_attempted = attempted.clone();
    let report_succeeded = succeeded.clone();
    let report_failed_connect = failed_connect.clone();
    let report_failed_write = failed_write.clone();
    let report_stop = stop.clone();

    let reporter = tokio::spawn(async move {
        let mut last_attempted = 0;
        let mut last_succeeded = 0;
        while !report_stop.load(Ordering::Relaxed) {
            sleep(Duration::from_secs(1)).await;

            let a = report_attempted.load(Ordering::Relaxed);
            let s = report_succeeded.load(Ordering::Relaxed);
            let fc = report_failed_connect.load(Ordering::Relaxed);
            let fw = report_failed_write.load(Ordering::Relaxed);

            let attempted_ps = a.saturating_sub(last_attempted);
            let succeeded_ps = s.saturating_sub(last_succeeded);
            last_attempted = a;
            last_succeeded = s;

            println!(
                "[LOADGEN] attempted/s={} succeeded/s={} total_succeeded={} failed_connect={} failed_write={}",
                attempted_ps, succeeded_ps, s, fc, fw
            );
        }
    });

    let per_worker_sleep = cfg.rps_limit.and_then(|global_rps| {
        let per_worker = global_rps as f64 / cfg.concurrency as f64;
        if per_worker <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(1.0 / per_worker))
        }
    });

    let mut handles = Vec::with_capacity(cfg.concurrency);
    for _ in 0..cfg.concurrency {
        let target = cfg.target.clone();
        let mode = cfg.mode;
        let attempted = attempted.clone();
        let succeeded = succeeded.clone();
        let failed_connect = failed_connect.clone();
        let failed_write = failed_write.clone();
        let total_latency_ns = total_latency_ns.clone();
        let tx_counter = tx_counter.clone();

        let handle = tokio::spawn(async move {
            while Instant::now() < end_at {
                attempted.fetch_add(1, Ordering::Relaxed);
                let started = Instant::now();

                match TcpStream::connect(&target).await {
                    Ok(mut stream) => {
                        let payload = build_payload(mode, &tx_counter);
                        match stream.write_all(&payload).await {
                            Ok(_) => {
                                let _ = stream.shutdown().await;
                                succeeded.fetch_add(1, Ordering::Relaxed);
                                total_latency_ns.fetch_add(
                                    started.elapsed().as_nanos() as u64,
                                    Ordering::Relaxed,
                                );
                            }
                            Err(_) => {
                                failed_write.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(_) => {
                        failed_connect.fetch_add(1, Ordering::Relaxed);
                    }
                }

                if let Some(delay) = per_worker_sleep {
                    sleep(delay).await;
                }
            }
        });

        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    stop.store(true, Ordering::Relaxed);
    let _ = reporter.await;

    let attempted_v = attempted.load(Ordering::Relaxed);
    let succeeded_v = succeeded.load(Ordering::Relaxed);
    let failed_connect_v = failed_connect.load(Ordering::Relaxed);
    let failed_write_v = failed_write.load(Ordering::Relaxed);
    let total_latency_ns_v = total_latency_ns.load(Ordering::Relaxed);

    let avg_latency_ms = if succeeded_v == 0 {
        0.0
    } else {
        (total_latency_ns_v as f64 / succeeded_v as f64) / 1_000_000.0
    };

    RunStats {
        attempted: attempted_v,
        succeeded: succeeded_v,
        failed_connect: failed_connect_v,
        failed_write: failed_write_v,
        avg_latency_ms,
        achieved_msg_per_sec: succeeded_v as f64 / cfg.duration_secs as f64,
    }
}

fn build_payload(mode: LoadMode, tx_counter: &Arc<AtomicU64>) -> Vec<u8> {
    let message = match mode {
        LoadMode::Ping => NetworkMessage::Ping,
        LoadMode::Tx => {
            let n = tx_counter.fetch_add(1, Ordering::Relaxed);
            let id = format!("{:064x}", n);
            let tx = Transaction {
                id,
                from: "loadgen_sender".to_string(),
                to: "loadgen_receiver".to_string(),
                amount: 1.0,
                fee: 0.001,
                timestamp: chrono::Utc::now().timestamp(),
                signature: None,
                data: None,
            };
            NetworkMessage::NewTransaction(tx)
        }
    };

    serde_json::to_vec(&message).unwrap_or_else(|_| b"\"Ping\"".to_vec())
}

fn print_summary(stats: RunStats) {
    let success_rate = if stats.attempted == 0 {
        0.0
    } else {
        (stats.succeeded as f64 / stats.attempted as f64) * 100.0
    };

    println!("\n=== Load test summary ===");
    println!("Attempted:          {}", stats.attempted);
    println!("Succeeded:          {}", stats.succeeded);
    println!("Failed connect:     {}", stats.failed_connect);
    println!("Failed write:       {}", stats.failed_write);
    println!("Success rate:       {:.2}%", success_rate);
    println!("Avg send latency:   {:.3} ms", stats.avg_latency_ms);
    println!("Achieved msg/sec:   {:.2}", stats.achieved_msg_per_sec);
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == key).map(|w| w[1].clone())
}

fn parse_u64(args: &[String], key: &str, default: u64) -> u64 {
    arg_value(args, key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_f64(args: &[String], key: &str, default: f64) -> f64 {
    arg_value(args, key)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn print_help() {
    println!(
        "loadgen - local load test harness for blockchain-node\n\n\
Usage:\n  cargo run --manifest-path blockchain-node/Cargo.toml --bin loadgen -- [options]\n\n\
Basic options:\n  --target <host:port>       Target node address (default: 127.0.0.1:8080)\n  --duration <secs>          Test duration in seconds (default: 30)\n  --concurrency <n>          Number of concurrent workers (default: 100)\n  --mode <ping|tx>           Message type (default: ping)\n  --rps <n>                  Optional global send-rate cap\n\n\
Max finder sweep:\n  --find-max                 Run concurrency sweep and report best sustainable result\n  --min-concurrency <n>      Sweep start (default: 10)\n  --max-concurrency <n>      Sweep end (default: 500)\n  --step <n>                 Sweep step (default: 10)\n  --step-duration <secs>     Duration for each sweep run (default: 15)\n  --success-threshold <f>    Minimum success ratio, 0.0..1.0 (default: 0.99)\n\n\
Examples:\n  cargo run --manifest-path blockchain-node/Cargo.toml --bin loadgen -- --concurrency 200 --duration 20 --mode ping\n  cargo run --manifest-path blockchain-node/Cargo.toml --bin loadgen -- --find-max --min-concurrency 50 --max-concurrency 1000 --step 50 --step-duration 10\n"
    );
}

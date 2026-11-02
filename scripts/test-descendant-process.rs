//! Native fixture for the ignored descendant tracking system test.
use std::os::windows::process::CommandExt;
use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<_> = env::args().collect();
    let dir = PathBuf::from(&args[1]);
    let role = &args[2];
    let prefix = &args[3];
    let spawn = |image: &str, role: &str| {
        Command::new(dir.join(format!("{prefix}-{image}.exe")))
            .args([dir.to_str().unwrap(), role, prefix])
            .creation_flags(0x08000000)
            .spawn()
            .unwrap()
    };
    let mut children = Vec::new();
    if role == "root" {
        children.push(spawn("worker", "worker"));
    }
    if role == "worker" {
        children.push(spawn("leaf", "leaf"));
    }
    if role == "leaf" {
        children.push(spawn("deep", "deep"));
    }
    fs::write(
        dir.join(format!("{role}.pid")),
        std::process::id().to_string(),
    )
    .unwrap();
    let started = Instant::now();
    let mut later = false;
    let mut allocation = vec![1u8; 8 * 1024 * 1024];
    while !dir.join("stop-all").exists() && started.elapsed() < Duration::from_secs(240) {
        if role == "root" && dir.join("exit-root").exists() {
            return;
        }
        if role == "worker" && !later && dir.join("start-later").exists() {
            children.push(spawn("late", "late"));
            later = true;
        }
        for page in allocation.chunks_mut(4096) {
            page[0] = page[0].wrapping_add(1);
        }
        std::hint::black_box(&allocation);
        thread::sleep(Duration::from_millis(50));
    }
    for mut child in children {
        let _ = child.wait();
    }
    fs::write(dir.join(format!("{role}.done")), b"done").unwrap();
}

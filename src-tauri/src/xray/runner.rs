//! Lifecycle of the xray.exe child process.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleWindow, ATTACH_PARENT_PROCESS, CTRL_BREAK_EVENT,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const LOG_LINES: usize = 500;
const GRACEFUL_STOP: Duration = Duration::from_secs(4);

/// Asks xray to shut down cleanly with Ctrl+Break. Killing it instead leaves the wintun
/// adapter half-removed for a few seconds, and the next start with the same adapter name
/// then fails with "Cannot create a file when that file already exists".
fn request_exit(pid: u32) -> bool {
    unsafe {
        // Ctrl events only reach processes sharing our console, so borrow xray's (hidden) one.
        // xray runs in its own process group, so the event does not reach this process.
        let had_console = !GetConsoleWindow().is_invalid();
        let _ = FreeConsole();
        let sent = AttachConsole(pid).is_ok() && GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid).is_ok();
        let _ = FreeConsole();
        if had_console {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        sent
    }
}

/// Kills every assigned process when the last handle closes, i.e. when the client exits
/// or crashes, so xray.exe can never outlive it and keep the tunnel up.
struct KillOnCloseJob(HANDLE);

unsafe impl Send for KillOnCloseJob {}
unsafe impl Sync for KillOnCloseJob {}

impl KillOnCloseJob {
    fn new() -> Option<Self> {
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok.is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }

    fn assign(&self, pid: u32) {
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
                let _ = AssignProcessToJobObject(self.0, process);
                let _ = CloseHandle(process);
            }
        }
    }
}

pub type LogBuffer = Arc<Mutex<VecDeque<String>>>;

pub struct XrayRunner {
    child: Option<Child>,
    job: Option<KillOnCloseJob>,
    log: LogBuffer,
}

fn push_log(log: &LogBuffer, line: String) {
    let mut log = log.lock().unwrap();
    if log.len() >= LOG_LINES {
        log.pop_front();
    }
    log.push_back(line);
}

type LogFile = Option<Arc<Mutex<std::fs::File>>>;

fn pipe_to_log(stream: impl Read + Send + 'static, log: LogBuffer, file: LogFile) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if let Some(f) = &file {
                let _ = writeln!(f.lock().unwrap(), "{line}");
            }
            push_log(&log, line);
        }
    });
}

fn xray_command(core_dir: &Path) -> Command {
    let mut cmd = Command::new(core_dir.join("xray.exe"));
    cmd.current_dir(core_dir)
        .env("XRAY_LOCATION_ASSET", core_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null());
    cmd
}

impl XrayRunner {
    pub fn new() -> Self {
        Self { child: None, job: KillOnCloseJob::new(), log: Arc::new(Mutex::new(VecDeque::new())) }
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().cloned().collect()
    }

    /// Shared handle, so the UI can read the log without waiting on the runner lock.
    pub fn log_buffer(&self) -> LogBuffer {
        self.log.clone()
    }

    pub fn is_running(&mut self) -> bool {
        match &mut self.child {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Validates the config with `xray run -test`, returning xray's own error text on failure.
    pub fn test_config(core_dir: &Path, config: &Path) -> anyhow::Result<()> {
        let out = xray_command(core_dir).args(["run", "-test", "-c"]).arg(config).output()?;
        if out.status.success() {
            return Ok(());
        }
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let reason = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with("Xray ") && !l.starts_with("A unified platform"))
            .last()
            .unwrap_or("invalid configuration")
            .to_string();
        anyhow::bail!(reason)
    }

    /// Starts xray and waits until `ready` reports success or the process dies.
    pub fn start(
        &mut self,
        core_dir: &Path,
        config: &Path,
        ready: impl Fn() -> bool,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        self.stop();
        Self::test_config(core_dir, config)?;
        push_log(&self.log, format!("--- starting {}", config.display()));

        let mut child = xray_command(core_dir)
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
            .args(["run", "-c"])
            .arg(config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(job) = &self.job {
            job.assign(child.id());
        }
        // The file keeps the log of the last session for diagnostics after a crash or restart.
        let file: LogFile = std::fs::File::create(config.with_file_name("xray.log")).ok().map(|f| Arc::new(Mutex::new(f)));
        if let Some(out) = child.stdout.take() {
            pipe_to_log(out, self.log.clone(), file.clone());
        }
        if let Some(err) = child.stderr.take() {
            pipe_to_log(err, self.log.clone(), file);
        }
        self.child = Some(child);

        let started = Instant::now();
        loop {
            if !self.is_running() {
                let tail: Vec<String> = self.log().into_iter().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
                self.child = None;
                anyhow::bail!("xray exited: {}", tail.join("\n"));
            }
            if ready() {
                return Ok(());
            }
            if started.elapsed() > timeout {
                self.stop();
                anyhow::bail!("xray did not become ready in {} s", timeout.as_secs());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        let mut graceful = false;
        if matches!(child.try_wait(), Ok(None)) && request_exit(child.id()) {
            let started = Instant::now();
            while started.elapsed() < GRACEFUL_STOP {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    graceful = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if !graceful {
            let _ = child.kill();
        }
        let _ = child.wait();
        push_log(&self.log, format!("--- stopped{}", if graceful { "" } else { " (killed)" }));
    }
}

impl Drop for XrayRunner {
    fn drop(&mut self) {
        self.stop();
        if let Some(job) = self.job.take() {
            unsafe {
                let _ = CloseHandle(job.0);
            }
        }
    }
}

/// Reads the version of the core: `VERSION` file written by our downloader, falling back
/// to parsing `xray version`.
pub fn core_version(core_dir: &Path) -> Option<String> {
    if let Ok(v) = std::fs::read_to_string(core_dir.join("VERSION")) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let out = xray_command(core_dir).arg("version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?;
    first.split_whitespace().nth(1).map(|v| format!("v{v}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reconnecting the way the client does: graceful stop, then an immediate start with the
    /// other adapter name (Windows may still be removing the previous adapter even after a
    /// clean shutdown). Needs administrator rights: `cargo test -- --ignored --test-threads=1`.
    /// The adapters get no routes, so the system's traffic is not affected.
    #[test]
    #[ignore]
    fn tun_reconnects_with_alternating_adapter_names() {
        let core_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/xray");
        let port = crate::net::free_port().unwrap();
        let tag = std::process::id() % 10000;
        let path = std::env::temp_dir().join(format!("literay-tun-test-{}.json", std::process::id()));

        let ready = || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
        let mut runner = XrayRunner::new();
        for cycle in 1..=6 {
            // Unique per run, so stale adapters of an earlier failed run cannot interfere.
            let name = format!("LRTest{tag}{}", if cycle % 2 == 0 { "a" } else { "b" });
            let config = serde_json::json!({
                "log": {"loglevel": "warning", "access": "none"},
                "metrics": {"tag": "metrics", "listen": format!("127.0.0.1:{port}")},
                "inbounds": [{"tag": "tun-in", "protocol": "tun", "settings": {"name": name, "mtu": 1500}}],
                "outbounds": [{"protocol": "freedom"}],
            });
            std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
            let started = Instant::now();
            runner
                .start(&core_dir, &path, ready, Duration::from_secs(20))
                .unwrap_or_else(|e| panic!("cycle {cycle}: {e}\n{}", runner.log().join("\n")));
            eprintln!("cycle {cycle}: ready in {:?}", started.elapsed());
            runner.stop();
            let log = runner.log();
            assert_eq!(log.last().map(String::as_str), Some("--- stopped"), "cycle {cycle} was not a graceful stop");
        }
        let _ = std::fs::remove_file(&path);
    }
}

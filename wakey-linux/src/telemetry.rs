//! Linux procfs reader for core's OS-independent process/host instruments.
use procfs::Current;
use wakey_core::telemetry::ProcessMetricsSource;

/// Reads only this process and the visible host clock; never scans other PIDs.
/// Constructing the reader does no I/O, so disabled OTLP remains local-only.
pub struct ProcfsProcessMetrics;

impl ProcessMetricsSource for ProcfsProcessMetrics {
    fn process_uptime_seconds(&self) -> Option<f64> {
        process_uptime()
    }
    fn resident_memory_bytes(&self) -> Option<u64> {
        resident_bytes()
    }
    fn host_uptime_seconds(&self) -> Option<f64> {
        host_uptime()
    }
}

fn host_uptime() -> Option<f64> {
    let seconds = procfs::Uptime::current().ok()?.uptime;
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

fn process_uptime() -> Option<f64> {
    let stat = procfs::process::Process::myself().ok()?.stat().ok()?;
    let ticks = procfs::ticks_per_second();
    if ticks == 0 {
        return None;
    }
    // Both values use the kernel's boot-time clock, not wall time or telemetry
    // initialization time. This also avoids parsing process names in stat ourselves.
    let age = host_uptime()? - stat.starttime as f64 / ticks as f64;
    (age.is_finite() && age >= 0.0).then_some(age)
}

fn resident_bytes() -> Option<u64> {
    let status = procfs::process::Process::myself().ok()?.status().ok()?;
    // procfs documents VmRSS in KiB even though the Linux file spells it "kB".
    // It includes anonymous, file-backed and shared resident pages, not heap alone.
    status.vmrss?.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_and_visible_host_can_be_sampled() {
        let source = ProcfsProcessMetrics;
        let age = source
            .process_uptime_seconds()
            .expect("process start/uptime available");
        let host = source.host_uptime_seconds().expect("host uptime available");
        assert!(age.is_finite() && age >= 0.0);
        assert!(host.is_finite() && host >= age);
        assert!(source.resident_memory_bytes().expect("VmRSS available") > 0);
    }
}

//! Linux procfs reader for core's OS-independent process/host instruments.
use procfs::Current;
use wakey_core::telemetry::{HostLoad, HostMemory, ProcessMetricsSource};

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
    fn cpu_time_seconds(&self) -> Option<f64> {
        let stat = procfs::process::Process::myself().ok()?.stat().ok()?;
        let ticks = procfs::ticks_per_second();
        if ticks == 0 {
            return None;
        }
        // utime/stime account for all process threads, not child processes.
        Some(stat.utime.checked_add(stat.stime)? as f64 / ticks as f64)
    }
    fn open_file_descriptors(&self) -> Option<u64> {
        let process = procfs::process::Process::myself().ok()?;
        // procfs uses the kernel's count when available, otherwise enumerates
        // directory entries without reading every FD's target. Sampling itself
        // temporarily opens procfs descriptors, which can contribute to this count.
        u64::try_from(process.fd_count().ok()?).ok()
    }
    fn host_memory(&self) -> Option<HostMemory> {
        let memory = procfs::Meminfo::current().ok()?;
        // Unlike Status::vmrss, Meminfo has already converted its fields to bytes.
        // Do not substitute MemFree when an older kernel omits MemAvailable.
        Some(HostMemory {
            total_bytes: memory.mem_total,
            available_bytes: memory.mem_available,
        })
    }
    fn host_load(&self) -> Option<HostLoad> {
        let load = procfs::LoadAverage::current().ok()?;
        Some(HostLoad {
            one_minute: f64::from(load.one),
            five_minutes: f64::from(load.five),
            fifteen_minutes: f64::from(load.fifteen),
        })
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
        let cpu = source
            .cpu_time_seconds()
            .expect("process CPU ticks available");
        assert!(cpu.is_finite() && cpu >= 0.0);
        assert!(source.open_file_descriptors().expect("FD count available") > 0);
        let memory = source.host_memory().expect("host memory available");
        assert!(memory.total_bytes > 0);
        if let Some(available) = memory.available_bytes {
            assert!(available <= memory.total_bytes);
        }
        let load = source.host_load().expect("host load available");
        for average in [load.one_minute, load.five_minutes, load.fifteen_minutes] {
            assert!(average.is_finite() && average >= 0.0);
        }
    }
}

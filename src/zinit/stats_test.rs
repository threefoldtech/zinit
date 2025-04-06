#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::Pid;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_process_stats_new() {
        let stats = ProcessStats::new();
        assert_eq!(stats.memory_kb, 0);
        assert_eq!(stats.cpu_percent, 0.0);
    }

    #[test]
    fn test_process_stats_update_invalid_pid() {
        let mut stats = ProcessStats::new();
        // Use PID 0 which is not a valid process
        let result = stats.update(Pid::from_raw(0));
        assert!(result.is_ok());
        assert_eq!(stats.memory_kb, 0);
        assert_eq!(stats.cpu_percent, 0.0);
    }

    #[test]
    fn test_process_stats_update_real_process() {
        // This test will only work on Linux systems
        if !cfg!(target_os = "linux") {
            return;
        }

        // Start a CPU-intensive process
        let mut child = Command::new("yes")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start test process");

        // Give the process some time to run
        thread::sleep(Duration::from_millis(100));

        let pid = Pid::from_raw(child.id() as i32);
        let mut stats = ProcessStats::new();

        // First update to initialize
        let result = stats.update(pid);
        assert!(result.is_ok());

        // Sleep to allow CPU usage to be measured
        thread::sleep(Duration::from_millis(200));

        // Second update to get actual measurements
        let result = stats.update(pid);
        assert!(result.is_ok());

        // Memory should be greater than 0 for a running process
        assert!(stats.memory_kb > 0, "Memory usage should be greater than 0");

        // CPU usage should be greater than 0 for a CPU-intensive process
        assert!(
            stats.cpu_percent > 0.0,
            "CPU usage should be greater than 0"
        );

        // Clean up
        child.kill().expect("Failed to kill test process");
    }

    #[test]
    fn test_process_stats_update_memory_only() {
        // This test will only work on Linux systems
        if !cfg!(target_os = "linux") {
            return;
        }

        // Start a memory-intensive process
        let mut child = Command::new("sleep")
            .arg("1")
            .spawn()
            .expect("Failed to start test process");

        let pid = Pid::from_raw(child.id() as i32);
        let mut stats = ProcessStats::new();

        // Update memory stats
        let result = stats.update_memory(pid);
        assert!(result.is_ok());

        // Memory should be greater than 0 for a running process
        assert!(stats.memory_kb > 0, "Memory usage should be greater than 0");

        // Clean up
        child.kill().expect("Failed to kill test process");
    }
}

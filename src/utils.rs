use libc::{getgrnam, getpwnam, gid_t, setgid, setuid, uid_t};
use std::{
    ffi::CString,
    fs::File,
    io::{BufRead, BufReader},
    sync::Arc,
    sync::atomic::{AtomicU32, Ordering},
    thread, time,
};

/// Sets the bits that are passed as arguments.
#[macro_export]
macro_rules! gpio_bits {
    ( $( $x:expr ),* ) => {
        {
            0
            $(
                | (1 << $x)
            )*
        }
    };
}

pub(crate) fn linux_has_module_loaded(name: &str) -> bool {
    let Ok(file) = File::open("/proc/modules") else {
        return false;
    };
    let reader = BufReader::new(file);
    // Each line starts with the module name followed by a space.
    reader
        .lines()
        .map_while(Result::ok)
        .any(|line| line.split(' ').next() == Some(name))
}

pub(crate) fn linux_has_isol_cpu(cpu: usize) -> bool {
    let Ok(file) = File::open("/sys/devices/system/cpu/isolated") else {
        return false;
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .any(|line| cpu_list_contains(&line, cpu))
}

/// Checks if a kernel CPU list such as "1-3,5" contains the given CPU.
fn cpu_list_contains(list: &str, cpu: usize) -> bool {
    list.trim().split(',').any(|range| {
        let mut bounds = range.split('-').map(|n| n.trim().parse::<usize>());
        match (bounds.next(), bounds.next()) {
            (Some(Ok(first)), None) => first == cpu,
            (Some(Ok(first)), Some(Ok(last))) => (first..=last).contains(&cpu),
            _ => false,
        }
    })
}

/// Pins the current thread to the given core. Returns `true` on success.
#[cfg(target_os = "linux")]
pub fn set_thread_affinity(core_id: usize) -> bool {
    use libc::{CPU_SET, cpu_set_t, sched_setaffinity};
    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_SET(core_id, &mut set) }
    let cpusetsize = std::mem::size_of::<cpu_set_t>();
    let res = unsafe { sched_setaffinity(0, cpusetsize, &set) };
    res == 0
}

/// Thread affinity is only supported on Linux.
#[cfg(not(target_os = "linux"))]
pub fn set_thread_affinity(_core_id: usize) -> bool {
    false
}

fn gid_from_name(group: &str) -> Option<gid_t> {
    let group_cstr = CString::new(group).ok()?;
    unsafe {
        let g = getgrnam(group_cstr.as_ptr());
        if g.is_null() { None } else { Some((*g).gr_gid) }
    }
}

fn set_gid(gid: gid_t) -> bool {
    unsafe { setgid(gid) == 0 }
}

fn set_supplementary_groups(gid: gid_t) -> bool {
    let groups = [gid];
    unsafe { libc::setgroups(groups.len() as _, groups.as_ptr()) == 0 }
}

fn uid_from_name(user: &str) -> Option<uid_t> {
    let user_cstr = CString::new(user).ok()?;
    unsafe {
        let p = getpwnam(user_cstr.as_ptr());
        if p.is_null() { None } else { Some((*p).pw_uid) }
    }
}

fn set_uid(uid: uid_t) -> bool {
    unsafe { setuid(uid) == 0 }
}

/// Drop to the provided user / group's privileges.
pub(crate) fn drop_privs(user: &str, group: &str) -> Result<(), String> {
    let gid: gid_t = group
        .parse()
        .ok()
        .or_else(|| gid_from_name(group))
        .ok_or_else(|| format!("Failed to get GID for group '{group}'"))?;

    let uid: uid_t = user
        .parse()
        .ok()
        .or_else(|| uid_from_name(user))
        .ok_or_else(|| format!("Failed to get UID for user '{user}'"))?;

    if !set_supplementary_groups(gid) {
        return Err(String::from("Failed to set supplementary groups"));
    }
    if !set_gid(gid) {
        return Err(String::from("Failed to set GID"));
    }
    if !set_uid(uid) {
        return Err(String::from("Failed to set UID"));
    }

    Ok(())
}

const WINDOW_LENGTH: usize = 60;

pub(crate) struct FrameRateMonitor {
    times: [f32; WINDOW_LENGTH],
    index: usize,
    last_time: Option<time::Instant>,
    framerate: Arc<AtomicU32>,
}

impl FrameRateMonitor {
    pub(crate) fn new(framerate: Arc<AtomicU32>) -> Self {
        Self {
            times: [1.0 / WINDOW_LENGTH as f32; WINDOW_LENGTH],
            index: 0,
            last_time: None,
            framerate,
        }
    }

    pub(crate) fn update(&mut self) {
        if let Some(last_time) = self.last_time.take() {
            self.times[self.index] = last_time.elapsed().as_secs_f32();
            self.index = (self.index + 1) % WINDOW_LENGTH;
        }
        self.last_time = Some(time::Instant::now());

        self.framerate.store(self.fps() as u32, Ordering::Relaxed);
    }

    /// Stop measuring until the next update, so that the time in between is not counted as a frame.
    pub(crate) fn pause(&mut self) {
        self.last_time = None;
        self.framerate.store(0, Ordering::Relaxed);
    }

    pub(crate) fn fps(&self) -> f32 {
        WINDOW_LENGTH as f32 / self.times.iter().sum::<f32>()
    }
}

/// Sleep for as long as we are comfortable without overshooting. The OS usually wakes us up late, so we wake
/// up `min_sys_sleep_time` early and the caller is expected to busy-wait for the rest.
pub(crate) fn sleep_at_most(duration: time::Duration, min_sys_sleep_time: time::Duration) {
    if duration > min_sys_sleep_time {
        thread::sleep(duration - min_sys_sleep_time);
    }
}

/// Wait for the given number of nanoseconds, sleeping for longer durations and busy-waiting for the rest.
pub(crate) fn sleep_nanos(nanos: u32, min_sys_sleep_time: time::Duration) {
    let start = time::Instant::now();
    let duration = time::Duration::from_nanos(u64::from(nanos));
    sleep_at_most(duration, min_sys_sleep_time);
    while start.elapsed() < duration {
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::cpu_list_contains;

    #[test]
    fn test_gpio_bits() {
        assert_eq!(gpio_bits!(1, 4, 5), 1 << 1 | 1 << 4 | 1 << 5);
    }

    #[test]
    fn test_cpu_list_contains() {
        assert!(cpu_list_contains("3\n", 3));
        assert!(!cpu_list_contains("13", 3));
        assert!(cpu_list_contains("1-3", 2));
        assert!(cpu_list_contains("0,2-3", 3));
        assert!(!cpu_list_contains("0,2-3", 1));
        assert!(!cpu_list_contains("", 0));
    }
}

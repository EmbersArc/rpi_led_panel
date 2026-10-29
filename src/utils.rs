use libc::{
    CPU_SET, cpu_set_t, getgrnam, getpwnam, gid_t, sched_setaffinity, setgid, setuid, uid_t,
};
use std::{
    ffi::CString,
    fs::read_to_string,
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

#[must_use]
pub(crate) fn linux_has_module_loaded(name: &str) -> bool {
    read_to_string("/proc/modules").is_ok_and(|modules| module_list_contains(&modules, name))
}

/// Each line of `/proc/modules` starts with the module name, e.g. `snd_bcm2835 24576 0 - Live 0x0`.
#[must_use]
fn module_list_contains(modules: &str, name: &str) -> bool {
    modules
        .lines()
        .any(|line| line.split_whitespace().next() == Some(name))
}

#[must_use]
pub(crate) fn linux_has_isol_cpu(cpu: usize) -> bool {
    read_to_string("/sys/devices/system/cpu/isolated")
        .is_ok_and(|cpus| cpu_list_contains(&cpus, cpu))
}

/// Whether a CPU list like `1,3-5` contains the CPU.
#[must_use]
fn cpu_list_contains(cpus: &str, cpu: usize) -> bool {
    cpus.trim().split(',').any(|range| {
        let (start, end) = range.split_once('-').unwrap_or((range, range));
        match (start.parse::<usize>(), end.parse::<usize>()) {
            (Ok(start), Ok(end)) => (start..=end).contains(&cpu),
            _ => false,
        }
    })
}

/// Pin the current thread to a core. Returns whether it succeeded.
#[must_use]
pub(crate) fn set_thread_affinity(core_id: usize) -> bool {
    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_SET(core_id, &mut set) }
    let cpusetsize = std::mem::size_of::<cpu_set_t>();
    let mask = &set;
    let res = unsafe { sched_setaffinity(0, cpusetsize, mask) };
    res == 0
}

fn get_gid_from_name(group: &str) -> Option<gid_t> {
    let group_cstr = CString::new(group).unwrap();
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
    unsafe { libc::setgroups(groups.len(), groups.as_ptr()) == 0 }
}

fn get_uid_from_name(user: &str) -> Option<uid_t> {
    let user_cstr = CString::new(user).unwrap();
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
        .or_else(|| get_gid_from_name(group))
        .ok_or("Failed to get GID for given group")?;

    let uid: uid_t = user
        .parse()
        .ok()
        .or_else(|| get_uid_from_name(user))
        .ok_or("Failed to get UID for given user")?;

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

        self.framerate
            .store(self.get_fps() as u32, Ordering::Relaxed);
    }

    /// Stop measuring until the next update, so that the time in between is not counted as a frame.
    pub(crate) fn pause(&mut self) {
        self.last_time = None;
        self.framerate.store(0, Ordering::Relaxed);
    }

    pub(crate) fn get_fps(&self) -> f32 {
        WINDOW_LENGTH as f32 / self.times.iter().sum::<f32>()
    }
}

/// Sleep for as long as we are confortable without overshooting.
pub(crate) fn sleep_at_most(duration: time::Duration, min_sys_sleep_time: time::Duration) {
    if duration > min_sys_sleep_time {
        thread::sleep(duration - min_sys_sleep_time);
    }
}

/// Sleep for exactly this long by busy waiting the last part.
pub(crate) fn sleep_exactly(duration: time::Duration, min_sys_sleep_time: time::Duration) {
    let end_time = time::Instant::now() + duration;
    sleep_at_most(duration, min_sys_sleep_time);
    while time::Instant::now() < end_time {
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::{cpu_list_contains, module_list_contains};

    #[test]
    fn test_gpio_bits() {
        assert_eq!(gpio_bits!(1, 4, 5), 1 << 1 | 1 << 4 | 1 << 5);
    }

    #[test]
    fn test_module_list_contains() {
        let modules = "snd_bcm2835 24576 0 - Live 0x0\nsnd_pcm 114688 1 snd_bcm2835, Live 0x0\n";
        assert!(module_list_contains(modules, "snd_bcm2835"));
        assert!(!module_list_contains(modules, "snd"));
        assert!(!module_list_contains(modules, "bcm2835"));
        assert!(!module_list_contains(modules, "w1_gpio"));
        assert!(!module_list_contains("", "w1_gpio"));
    }

    #[test]
    fn test_cpu_list_contains() {
        assert!(cpu_list_contains("3\n", 3));
        assert!(!cpu_list_contains("13\n", 3));
        assert!(cpu_list_contains("1-4\n", 3));
        assert!(cpu_list_contains("0,2-3\n", 3));
        assert!(!cpu_list_contains("0,2-3\n", 1));
        assert!(!cpu_list_contains("\n", 0));
    }
}

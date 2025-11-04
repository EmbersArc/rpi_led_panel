use std::{
    fs::File,
    io::{BufRead, BufReader},
    thread, time,
};

use libc::{CPU_SET, cpu_set_t, sched_setaffinity};

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
    reader.lines().any(|line| line.unwrap().contains(name))
}

pub(crate) fn linux_has_isol_cpu(cpu: usize) -> bool {
    let Ok(file) = File::open("/sys/devices/system/cpu/isolated") else {
        return false;
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .any(|line| line.unwrap().contains(&cpu.to_string()))
}

pub fn set_thread_affinity(core_id: usize) -> bool {
    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_SET(core_id, &mut set) }
    let cpusetsize = std::mem::size_of::<cpu_set_t>();
    let mask = &set;
    let res = unsafe { sched_setaffinity(0, cpusetsize, mask) };
    res != 0
}

const WINDOW_LENGTH: usize = 60;

pub(crate) struct FrameRateMonitor {
    times: [f32; WINDOW_LENGTH],
    index: usize,
    last_time: Option<time::Instant>,
}

impl FrameRateMonitor {
    pub(crate) fn new() -> Self {
        Self {
            times: [1.0 / WINDOW_LENGTH as f32; WINDOW_LENGTH],
            index: 0,
            last_time: None,
        }
    }

    pub(crate) fn update(&mut self) {
        if let Some(last_time) = self.last_time.take() {
            self.times[self.index] = last_time.elapsed().as_secs_f32();
            self.index = (self.index + 1) % WINDOW_LENGTH;
        }
        self.last_time = Some(time::Instant::now());
    }

    pub(crate) fn get_fps(&self) -> f32 {
        WINDOW_LENGTH as f32 / self.times.iter().sum::<f32>()
    }
}

/// Sleep for as long as we are confortable without overshooting.
pub(crate) fn sleep_at_most(duration: time::Duration) {
    const MIN_SYS_SLEEP_TIME: time::Duration = time::Duration::from_micros(200);
    if duration > MIN_SYS_SLEEP_TIME {
        thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_gpio_bits() {
        assert_eq!(gpio_bits!(1, 4, 5), 1 << 1 | 1 << 4 | 1 << 5);
    }
}

//! The real sampler on macOS: cheap kernel counters, no root, no helper
//! process.
//!
//! - CPU: `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`, ticks per core.
//! - Memory: `host_statistics64(HOST_VM_INFO64)` like Activity Monitor
//!   (app memory + wired + compressed), total from `hw.memsize`, swap from
//!   `vm.swapusage`, pressure from `kern.memorystatus_vm_pressure_level`.
//! - GPU: IOKit, the `IOAccelerator` services' `PerformanceStatistics` →
//!   `Device Utilization %` (readable without privileges on Apple silicon).
//!
//! One reading takes well under a millisecond.

#![allow(non_camel_case_types, non_upper_case_globals)]

use std::ffi::{c_char, c_void, CString};

use core_foundation_sys::base::{kCFAllocatorDefault, CFGetTypeID, CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::{
    CFDictionaryGetTypeID, CFDictionaryGetValue, CFDictionaryRef,
};
use core_foundation_sys::number::{
    kCFNumberSInt64Type, CFNumberGetTypeID, CFNumberGetValue, CFNumberRef,
};
use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringRef};

use crate::sample::{CpuTicks, HostInfo, MemInfo, Pressure, RawSample, Sampler};

type kern_return_t = i32;
type mach_port_t = u32;
type natural_t = u32;
type integer_t = i32;
type io_object_t = mach_port_t;

const KERN_SUCCESS: kern_return_t = 0;
const PROCESSOR_CPU_LOAD_INFO: i32 = 2;
const CPU_STATE_MAX: usize = 4;
const HOST_VM_INFO64: i32 = 4;

/// `struct vm_statistics64` (mach/vm_statistics.h).
#[repr(C)]
#[derive(Default)]
struct vm_statistics64 {
    free_count: natural_t,
    active_count: natural_t,
    inactive_count: natural_t,
    wire_count: natural_t,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: natural_t,
    speculative_count: natural_t,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: natural_t,
    throttled_count: natural_t,
    external_page_count: natural_t,
    internal_page_count: natural_t,
    total_uncompressed_pages_in_compressor: u64,
}

/// `struct xsw_usage` (sys/sysctl.h), for `vm.swapusage`.
#[repr(C)]
#[derive(Default)]
struct xsw_usage {
    xsu_total: u64,
    xsu_avail: u64,
    xsu_used: u64,
    xsu_pagesize: u32,
    xsu_encrypted: i32,
}

extern "C" {
    fn mach_host_self() -> mach_port_t;
    static mach_task_self_: mach_port_t;
    fn mach_port_deallocate(task: mach_port_t, name: mach_port_t) -> kern_return_t;
    fn host_processor_info(
        host: mach_port_t,
        flavor: i32,
        out_processor_count: *mut natural_t,
        out_processor_info: *mut *mut integer_t,
        out_processor_info_cnt: *mut u32,
    ) -> kern_return_t;
    fn host_statistics64(
        host: mach_port_t,
        flavor: i32,
        info: *mut integer_t,
        count: *mut u32,
    ) -> kern_return_t;
    fn vm_deallocate(task: mach_port_t, address: usize, size: usize) -> kern_return_t;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: mach_port_t,
        matching: CFDictionaryRef,
        existing: *mut io_object_t,
    ) -> kern_return_t;
    fn IOIteratorNext(iterator: io_object_t) -> io_object_t;
    fn IOObjectRelease(object: io_object_t) -> kern_return_t;
    fn IORegistryEntryCreateCFProperty(
        entry: io_object_t,
        key: CFStringRef,
        allocator: *const c_void,
        options: u32,
    ) -> CFTypeRef;
}

/// `sysctlbyname` for an integer of 4 or 8 bytes.
fn sysctl_u64(name: &str) -> Option<u64> {
    let c = CString::new(name).ok()?;
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    // SAFETY: `buf` has `len` bytes; the kernel writes at most `len`.
    let r = unsafe {
        libc::sysctlbyname(
            c.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return None;
    }
    match len {
        4 => Some(u32::from_ne_bytes(buf[..4].try_into().ok()?) as u64),
        8 => Some(u64::from_ne_bytes(buf)),
        _ => None,
    }
}

fn sysctl_string(name: &str) -> Option<String> {
    let c = CString::new(name).ok()?;
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: as above.
    let r = unsafe {
        libc::sysctlbyname(
            c.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 || len == 0 {
        return None;
    }
    let s = String::from_utf8_lossy(&buf[..len])
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!s.is_empty()).then_some(s)
}

fn swap_used() -> Option<u64> {
    let c = CString::new("vm.swapusage").ok()?;
    let mut x = xsw_usage::default();
    let mut len = std::mem::size_of::<xsw_usage>();
    // SAFETY: `x` is a plain C struct of `len` bytes.
    let r = unsafe {
        libc::sysctlbyname(
            c.as_ptr(),
            (&mut x as *mut xsw_usage).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (r == 0).then_some(x.xsu_used)
}

/// A CFString for an IOKit key; released on drop.
struct CfStr(CFStringRef);

impl CfStr {
    fn new(s: &str) -> Option<CfStr> {
        let c = CString::new(s).ok()?;
        // SAFETY: a valid NUL-terminated UTF-8 string.
        let r = unsafe {
            CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), kCFStringEncodingUTF8)
        };
        (!r.is_null()).then_some(CfStr(r))
    }
}

impl Drop for CfStr {
    fn drop(&mut self) {
        // SAFETY: created (owned) in `new`.
        unsafe { CFRelease(self.0.cast()) }
    }
}

/// A CFNumber as i64 (`None` for other types).
///
/// # Safety
/// `v` must be null or a valid CF object.
unsafe fn cf_i64(v: CFTypeRef) -> Option<i64> {
    if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
        return None;
    }
    let mut out: i64 = 0;
    let ok = CFNumberGetValue(
        v as CFNumberRef,
        kCFNumberSInt64Type,
        (&mut out as *mut i64).cast(),
    );
    ok.then_some(out)
}

/// Call `f` with each `IOAccelerator` service.
fn for_each_accelerator(mut f: impl FnMut(io_object_t)) {
    let name = c"IOAccelerator";
    let mut iter: io_object_t = 0;
    // SAFETY: IOServiceGetMatchingServices consumes the matching dictionary;
    // the iterator and each service are released here.
    unsafe {
        let matching = IOServiceMatching(name.as_ptr());
        if matching.is_null()
            || IOServiceGetMatchingServices(0, matching, &mut iter) != KERN_SUCCESS
        {
            return;
        }
        loop {
            let service = IOIteratorNext(iter);
            if service == 0 {
                break;
            }
            f(service);
            IOObjectRelease(service);
        }
        IOObjectRelease(iter);
    }
}

/// An integer property of a registry entry.
fn registry_i64(entry: io_object_t, key: &CfStr) -> Option<i64> {
    // SAFETY: the property is a +1 CF object, released below.
    unsafe {
        let v = IORegistryEntryCreateCFProperty(entry, key.0, kCFAllocatorDefault.cast(), 0);
        let n = cf_i64(v);
        if !v.is_null() {
            CFRelease(v);
        }
        n
    }
}

/// The highest "Device Utilization %" of all GPUs, 0–1.
fn gpu_utilization(stats_key: &CfStr, util_key: &CfStr) -> Option<f32> {
    let mut best: Option<i64> = None;
    for_each_accelerator(|service| {
        // SAFETY: the dictionary is a +1 CF object, released below; the
        // value inside is borrowed from it.
        unsafe {
            let d = IORegistryEntryCreateCFProperty(
                service,
                stats_key.0,
                kCFAllocatorDefault.cast(),
                0,
            );
            if d.is_null() {
                return;
            }
            if CFGetTypeID(d) == CFDictionaryGetTypeID() {
                let v = CFDictionaryGetValue(d as CFDictionaryRef, util_key.0.cast());
                if let Some(n) = cf_i64(v) {
                    best = Some(best.map_or(n, |b| b.max(n)));
                }
            }
            CFRelease(d);
        }
    });
    best.map(|n| (n as f32 / 100.0).clamp(0.0, 1.0))
}

pub struct MacSampler {
    host: mach_port_t,
    page_size: u64,
    total: u64,
    stats_key: Option<CfStr>,
    util_key: Option<CfStr>,
    /// No GPU statistics on this Mac: stop asking.
    gpu_missing: bool,
}

// SAFETY: the CF strings are immutable and only used from the thread that
// owns the sampler; the mach port is a plain integer.
unsafe impl Send for MacSampler {}

impl MacSampler {
    pub fn new() -> Self {
        // SAFETY: returns a send right to the host port (released on drop).
        let host = unsafe { mach_host_self() };
        MacSampler {
            host,
            page_size: sysctl_u64("hw.pagesize")
                .filter(|p| *p > 0)
                .unwrap_or(16384),
            total: sysctl_u64("hw.memsize").unwrap_or(0),
            stats_key: CfStr::new("PerformanceStatistics"),
            util_key: CfStr::new("Device Utilization %"),
            gpu_missing: false,
        }
    }

    fn cpu(&self) -> Vec<CpuTicks> {
        let mut count: natural_t = 0;
        let mut info: *mut integer_t = std::ptr::null_mut();
        let mut info_count: u32 = 0;
        // SAFETY: the kernel allocates `info` (`info_count` integers) in our
        // address space; it is read, then deallocated.
        unsafe {
            if host_processor_info(
                self.host,
                PROCESSOR_CPU_LOAD_INFO,
                &mut count,
                &mut info,
                &mut info_count,
            ) != KERN_SUCCESS
                || info.is_null()
            {
                return Vec::new();
            }
            let n = (count as usize).min(info_count as usize / CPU_STATE_MAX);
            let ticks = std::slice::from_raw_parts(info as *const u32, n * CPU_STATE_MAX);
            let v = ticks
                .as_chunks::<CPU_STATE_MAX>()
                .0
                .iter()
                .map(|t| CpuTicks::new(t[0], t[1], t[2], t[3]))
                .collect();
            vm_deallocate(
                mach_task_self_,
                info as usize,
                info_count as usize * std::mem::size_of::<integer_t>(),
            );
            v
        }
    }

    fn mem(&self) -> Option<MemInfo> {
        let mut vm = vm_statistics64::default();
        let mut count =
            (std::mem::size_of::<vm_statistics64>() / std::mem::size_of::<integer_t>()) as u32;
        // SAFETY: `vm` has room for `count` integers.
        let r = unsafe {
            host_statistics64(
                self.host,
                HOST_VM_INFO64,
                (&mut vm as *mut vm_statistics64).cast(),
                &mut count,
            )
        };
        if r != KERN_SUCCESS || self.total == 0 {
            return None;
        }
        let pages = |n: natural_t| n as u64 * self.page_size;
        let pressure = sysctl_u64("kern.memorystatus_vm_pressure_level").and_then(|l| match l {
            1 => Some(Pressure::Normal),
            2 => Some(Pressure::Warning),
            4 => Some(Pressure::Critical),
            _ => None,
        });
        Some(MemInfo {
            total: self.total,
            app: pages(vm.internal_page_count.saturating_sub(vm.purgeable_count)),
            wired: pages(vm.wire_count),
            compressed: pages(vm.compressor_page_count),
            swap_used: swap_used(),
            pressure,
        })
    }

    fn gpu(&mut self) -> Option<f32> {
        if self.gpu_missing {
            return None;
        }
        let (Some(s), Some(u)) = (&self.stats_key, &self.util_key) else {
            return None;
        };
        let g = gpu_utilization(s, u);
        self.gpu_missing = g.is_none();
        g
    }
}

impl Default for MacSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MacSampler {
    fn drop(&mut self) {
        // SAFETY: the send right from `mach_host_self`.
        unsafe {
            mach_port_deallocate(mach_task_self_, self.host);
        }
    }
}

impl Sampler for MacSampler {
    fn info(&mut self) -> HostInfo {
        let gpu_cores = CfStr::new("gpu-core-count").and_then(|key| {
            let mut cores = None;
            for_each_accelerator(|s| {
                if cores.is_none() {
                    cores = registry_i64(s, &key)
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|n| *n > 0);
                }
            });
            cores
        });
        HostInfo {
            chip: sysctl_string("machdep.cpu.brand_string"),
            perf_cores: sysctl_u64("hw.perflevel0.logicalcpu").map(|n| n as u32),
            eff_cores: sysctl_u64("hw.perflevel1.logicalcpu").map(|n| n as u32),
            gpu_cores,
        }
    }

    fn sample(&mut self) -> RawSample {
        RawSample {
            cpu: self.cpu(),
            mem: self.mem(),
            gpu: self.gpu(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads this Mac once and prints the numbers. Run explicitly:
    /// `cargo test -p manta-plugin-sysstats real_sampler -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the real machine"]
    fn real_sampler_prints_plausible_numbers() {
        let mut s = MacSampler::new();
        let info = s.info();
        let mut t = crate::sample::Tracker::default();
        t.update(s.sample());
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let start = std::time::Instant::now();
        let raw = s.sample();
        let took = start.elapsed();
        let st = t.update(raw);
        eprintln!("info: {info:?}");
        eprintln!("one reading took {took:?}");
        eprintln!("cores: {}", st.cores);
        let cpu = st.cpu.expect("cpu load");
        eprintln!(
            "cpu: {:.1} % (busiest core {:.1} %)",
            cpu.total * 100.0,
            cpu.busiest() * 100.0
        );
        let m = st.mem.expect("memory");
        const GIB: f64 = (1u64 << 30) as f64;
        eprintln!(
            "ram: used {:.2} GiB of {:.2} GiB (app {:.2}, wired {:.2}, compressed {:.2}), swap {:?}, pressure {:?}",
            m.used() as f64 / GIB,
            m.total as f64 / GIB,
            m.app as f64 / GIB,
            m.wired as f64 / GIB,
            m.compressed as f64 / GIB,
            m.swap_used.map(|b| b as f64 / GIB),
            m.pressure
        );
        eprintln!("gpu: {:?}", st.gpu.map(|g| g * 100.0));
        assert!(st.cores >= 1);
        assert!((0.0..=1.0).contains(&cpu.total));
        assert!(m.total >= 4 << 30, "at least 4 GB");
        assert!(m.used() > 1 << 30 && m.used() <= m.total);
        assert!(took < std::time::Duration::from_millis(50), "{took:?}");
    }
}

use std::fs::File;
use std::io::{self, BufRead};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub enum IpCidr {
    V4(Ipv4Addr, u8),
    V6(Ipv6Addr, u8),
}

impl IpCidr {
    fn parts(&self) -> (u128, u8, u8, u128) {
        match self {
            IpCidr::V4(ip, len) => (u32::from(*ip) as u128, *len, 32, u32::MAX as u128),
            IpCidr::V6(ip, len) => (u128::from(*ip), *len, 128, u128::MAX),
        }
    }

    pub fn range_u128(&self) -> (u128, u128) {
        let (val, len, max_bits, full_mask) = self.parts();
        let host_bits = max_bits - len;

        if host_bits >= max_bits {
            return (0, full_mask);
        }

        let mask = full_mask << host_bits & full_mask;
        let start = val & mask;
        let end = start | (!mask & full_mask);
        
        (start, end)
    }

    pub fn prefix_len(&self) -> u8 {
        match self {
            IpCidr::V4(_, len) | IpCidr::V6(_, len) => *len,
        }
    }

    pub fn is_single_host(&self) -> bool {
        matches!(self, IpCidr::V4(_, 32) | IpCidr::V6(_, 128))
    }

    pub fn to_ipaddr(self) -> IpAddr {
        let (start, _) = self.range_u128();
        match self {
            IpCidr::V4(..) => IpAddr::V4(Ipv4Addr::from(start as u32)),
            IpCidr::V6(..) => IpAddr::V6(Ipv6Addr::from(start)),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (ip_part, prefix_part) = s.split_once('/')?;
        let ip = IpAddr::from_str(ip_part).ok()?;
        let prefix = prefix_part.parse::<u8>().ok()?;
        match ip {
            IpAddr::V4(v4) if prefix <= 32 => Some(IpCidr::V4(v4, prefix)),
            IpAddr::V6(v6) if prefix <= 128 => Some(IpCidr::V6(v6, prefix)),
            _ => None,
        }
    }
}

/// 128 位确定性伪随机数生成器，基于 splitmix64 扩展。
/// 输入 obj_addr 确保同一来源生成的随机序列确定可复现。
fn generate_refined_random(obj_addr: usize) -> u128 {
    static SHARED_STATE: AtomicUsize = AtomicUsize::new(0);

    let hasher_seed = generate_refined_random as *const () as usize;
    let s = SHARED_STATE.fetch_add(1, Ordering::Relaxed);
    let t = &s as *const _ as usize;

    let mut lo = s ^ obj_addr ^ t;
    lo = lo.wrapping_mul(hasher_seed | 1);
    lo = lo.rotate_left(usize::BITS / 2);
    lo = lo.swap_bytes();

    let mut hi = lo.wrapping_mul(0x9E3779B97F4A7C15_u64 as usize);
    hi = hi.rotate_left(usize::BITS / 4);
    hi = hi.swap_bytes();
    hi ^= s;

    (hi as u128) << 64 | (lo as u128)
}

const DEFAULT_SAMPLES_PER_SUBNET: u128 = 5;

fn calculate_sample_count(prefix: u8, is_ipv4: bool) -> u128 {
    let threshold: u8 = if is_ipv4 { 24 } else { 48 };
    let shift = u32::from(threshold.saturating_sub(prefix));
    (1u128 << shift).saturating_mul(DEFAULT_SAMPLES_PER_SUBNET)
}

#[derive(Debug)]
enum IpSegment {
    Static {
        ips: Vec<IpAddr>,
        cursor: AtomicUsize,
        exhausted_notified: AtomicBool,
    },
    Cidr {
        start: u128,
        interval_size: u128,
        last_size: u128,
        total_count: u64,
        current: AtomicUsize,
        is_v6: bool,
        exhausted_notified: AtomicBool,
    },
}

impl IpSegment {
    fn next_ip(&self) -> Option<IpAddr> {
        match self {
            IpSegment::Static { ips, cursor, .. } => {
                let idx = cursor.fetch_add(1, Ordering::Relaxed);
                ips.get(idx).copied()
            }
            IpSegment::Cidr { start, interval_size, last_size, total_count, current, is_v6, .. } => {
                let idx = current.fetch_add(1, Ordering::Relaxed);
                let total = *total_count as usize;
                if idx >= total {
                    return None;
                }

                let interval = *interval_size;
                let interval_start = *start + (idx as u128 * interval);
                let actual_interval_size = if idx == total - 1 {
                    *last_size
                } else {
                    interval
                };

                let random_offset = if actual_interval_size <= 1 {
                    0
                } else {
                    generate_refined_random(self as *const Self as usize) % actual_interval_size
                };

                let ip_val = interval_start + random_offset;
                if *is_v6 {
                    Some(IpAddr::V6(Ipv6Addr::from(ip_val)))
                } else {
                    Some(IpAddr::V4(Ipv4Addr::from(ip_val as u32)))
                }
            }
        }
    }

    fn mark_exhausted_once(&self) -> bool {
        match self {
            IpSegment::Static { exhausted_notified, .. }
            | IpSegment::Cidr { exhausted_notified, .. } => {
                exhausted_notified
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
            }
        }
    }

    fn reset(&self) {
        match self {
            IpSegment::Static { cursor, exhausted_notified, .. } => {
                cursor.store(0, Ordering::Relaxed);
                exhausted_notified.store(false, Ordering::Relaxed);
            }
            IpSegment::Cidr { current, exhausted_notified, .. } => {
                current.store(0, Ordering::Relaxed);
                exhausted_notified.store(false, Ordering::Relaxed);
            }
        }
    }
}

#[derive(Debug)]
pub struct IpPool {
    segments: Vec<Arc<IpSegment>>,
    cursor: AtomicUsize,
    active_count: AtomicUsize,
    total_count: AtomicU64,
}

impl IpPool {
    pub fn new(sources: &[String]) -> Self {
        let mut single_ips: Vec<IpAddr> = Vec::new();
        let mut cidr_segments: Vec<Arc<IpSegment>> = Vec::new();
        let mut total: u64 = 0;

        for source in sources {
            let s = source.trim();
            if s.is_empty() || s.starts_with('#') || s.starts_with("//") {
                continue;
            }

            let (cidr_part, custom_count) = if let Some((cidr_part, count_str)) = s.split_once('=') {
                let count = count_str.trim().parse::<u128>().ok().filter(|&n| n > 0);
                (cidr_part.trim(), count)
            } else {
                (s, None)
            };

            if let Ok(ip) = cidr_part.parse::<IpAddr>() {
                single_ips.push(ip);
                total += 1;
            } else if let Some(cidr) = IpCidr::parse(cidr_part) {
                if cidr.is_single_host() {
                    single_ips.push(cidr.to_ipaddr());
                    total += 1;
                } else {
                    let (start, end) = cidr.range_u128();
                    let range_size = (end - start).saturating_add(1);
                    let is_ipv6 = matches!(cidr, IpCidr::V6(_, _));

                    let sample_count = if let Some(count) = custom_count {
                        count.min(range_size)
                    } else {
                        calculate_sample_count(cidr.prefix_len(), !is_ipv6)
                    };

                    let interval_size = if sample_count > 0 {
                        range_size.saturating_div(sample_count).max(1)
                    } else {
                        1
                    };

                    let last_size = if sample_count > 0 {
                        let last_start = start + (sample_count - 1) * interval_size;
                        (end - last_start).saturating_add(1)
                    } else {
                        interval_size
                    };

                    cidr_segments.push(Arc::new(IpSegment::Cidr {
                        start,
                        interval_size,
                        last_size,
                        total_count: sample_count as u64,
                        current: AtomicUsize::new(0),
                        is_v6: is_ipv6,
                        exhausted_notified: AtomicBool::new(false),
                    }));
                    total += sample_count as u64;
                }
            }
        }

        let mut segments: Vec<Arc<IpSegment>> = Vec::new();

        if !single_ips.is_empty() {
            segments.push(Arc::new(IpSegment::Static {
                ips: single_ips,
                cursor: AtomicUsize::new(0),
                exhausted_notified: AtomicBool::new(false),
            }));
        }

        segments.extend(cidr_segments);

        let active_count = segments.len();

        Self {
            segments,
            cursor: AtomicUsize::new(0),
            active_count: AtomicUsize::new(active_count),
            total_count: AtomicU64::new(total),
        }
    }

    pub fn from_file(path: &str) -> Self {
        let mut lines = Vec::new();
        
        if let Ok(file) = File::open(path) {
            for line in io::BufReader::new(file).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        
        Self::new(&lines)
    }

    pub fn total_count(&self) -> u64 {
        self.total_count.load(Ordering::Relaxed)
    }

    pub fn pop(&self) -> Option<IpAddr> {
        loop {
            if self.active_count.load(Ordering::Acquire) == 0 {
                return None;
            }

            let start_idx = self.cursor.fetch_add(1, Ordering::Relaxed);

            for i in 0..self.segments.len() {
                let idx = (start_idx + i) % self.segments.len();
                let segment = &self.segments[idx];

                if let Some(ip) = segment.next_ip() {
                    return Some(ip);
                }

                if segment.mark_exhausted_once() {
                    self.active_count.fetch_sub(1, Ordering::SeqCst);
                }
            }

            // 全部耗尽后重置循环
            for segment in &self.segments {
                segment.reset();
            }
            self.cursor.store(0, Ordering::Relaxed);
            self.active_count.store(self.segments.len(), Ordering::Relaxed);
        }
    }
}
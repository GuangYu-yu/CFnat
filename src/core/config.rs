use std::sync::OnceLock;
use std::time::Duration;

/// EWMA 样本数达到样本窗口的该比例后才考虑剔除
const EVICT_THRESHOLD_RATIO: f32 = 0.4;

#[derive(Clone)]
pub struct Config {
    pub sample_window: f32,
    pub alpha: f32,
    pub evict_threshold: usize,
    pub max_backup_target: usize,
    pub ping_times: u8,
    pub health_check_concurrency: usize,
    pub health_check_interval: Duration,
    pub warming_duration: Duration,
    pub sticky_base_interval: Duration,
    pub sticky_increment_interval: Duration,
    pub sticky_slot_ttl: Duration,
    pub sticky_slot_expand_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Self {
        let sample_window = 50.0;
        Config {
            sample_window,                                        // EWMA 样本窗口大小
            alpha: 2.0 / (sample_window + 1.0),                   // EWMA 衰减因子
            evict_threshold: (sample_window * EVICT_THRESHOLD_RATIO) as usize, // 剔除阈值：EWMA 样本数达到此比例后才考虑剔除
            max_backup_target: 10,                                // 最大备选 IP 数量
            ping_times: 4,                                        // 每个 IP 的测速次数
            health_check_concurrency: 2,                          // 健康检查并发数
            health_check_interval: Duration::from_secs(25),       // 健康检查间隔
            warming_duration: Duration::from_secs(60),            // 新增 IP 预热时长
            sticky_base_interval: Duration::from_secs(10),        // Sticky 模式基础切换间隔
            sticky_increment_interval: Duration::from_secs(5),    // Sticky 每增加一个槽位的间隔增量
            sticky_slot_ttl: Duration::from_secs(15),             // Sticky 槽位过期时间
            sticky_slot_expand_interval: Duration::from_secs(10), // Sticky 槽位扩展检查间隔
        }
    }
}

pub static GLOBAL_CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get_global_config() -> &'static Config {
    GLOBAL_CONFIG.get_or_init(Config::new)
}
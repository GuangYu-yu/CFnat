use std::env;
use std::net::SocketAddr;

use crate::core::ServiceConfig;

pub struct Args;

impl Args {
    pub fn parse_to_config() -> Option<ServiceConfig> {
        let args: Vec<String> = env::args().collect();
        
        if args.len() == 1 {
            return None;
        }
        
        if args.iter().any(|a| a == "-h") {
            print_help();
            std::process::exit(0);
        }
        
        Some(Self::parse_from_iter(args.iter().skip(1).cloned()))
    }
    
    pub fn parse_line_to_config(line: &str) -> Option<ServiceConfig> {
        let args: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
        
        if args.is_empty() {
            return None;
        }
        
        Some(Self::parse_from_iter(args.into_iter()))
    }
    
    fn parse_from_iter<I: Iterator<Item = String>>(iter: I) -> ServiceConfig {
        let parsed = Self::parse_args_to_vec(iter);
        let mut config = ServiceConfig::default();
        
        for (k, v_opt) in parsed {
            match k.as_str() {
                "addr" => {
                    if let Some(v) = v_opt
                        && let Ok(addr) = v.parse::<SocketAddr>()
                    {
                        config.addr = addr;
                    }
                }
                "colo" => {
                    config.colo = v_opt.map(|v| {
                        v.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    });
                }
                "dl" => {
                    config.delay_limit = v_opt
                        .and_then(|v| v.parse::<u64>().ok())
                        .map_or(config.delay_limit, |v| v.clamp(1, 2000));
                }
                "tlr" => {
                    config.tlr = v_opt
                        .and_then(|v| v.parse::<f64>().ok())
                        .map_or(config.tlr, |v| v.clamp(0.0, 1.0));
                }
                "http" => {
                    if let Some(v) = v_opt {
                        config.http = v;
                    }
                }
                "ips" => {
                    config.ips = v_opt
                        .and_then(|v| v.parse::<usize>().ok())
                        .map_or(config.ips, |v| v.clamp(1, 128));
                }
                "n" => {
                    config.threads = v_opt
                        .and_then(|v| v.parse::<usize>().ok())
                        .map_or(config.threads, |v| v.clamp(1, 1024));
                }
                "tp" => {
                    config.tls_port = v_opt
                        .and_then(|v| v.parse::<u16>().ok())
                        .map_or(config.tls_port, |v| v.clamp(1, u16::MAX));
                }
                "p" => {
                    config.http_port = v_opt
                        .and_then(|v| v.parse::<u16>().ok())
                        .map_or(config.http_port, |v| v.clamp(1, u16::MAX));
                }
                "f" => {
                    if let Some(v) = v_opt {
                        config.ip_file = v;
                    }
                }
                "s" => {
                    config.max_sticky_slots = v_opt
                        .and_then(|v| v.parse::<usize>().ok())
                        .map_or(config.max_sticky_slots, |v| v.clamp(1, 32));
                }
                #[cfg(feature = "web")]
                "api" => {
                    if let Some(ref v) = v_opt
                        && let Ok(parsed_addr) = v.parse::<SocketAddr>()
                    {
                        config.api_addr = Some(parsed_addr);
                    }
                }
                #[cfg(not(feature = "web"))]
                "api" => {
                    eprintln!("错误: -api 参数需要启用 web feature");
                    std::process::exit(1);
                }
                _ => {
                    print_help();
                    eprintln!("无效的参数: {k}");
                    std::process::exit(1);
                }
            }
        }
        
        config
    }

    fn parse_args_to_vec<I: Iterator<Item = String>>(iter: I) -> Vec<(String, Option<String>)> {
        let mut iter = iter.peekable();
        let mut result = Vec::new();

        while let Some(arg) = iter.next() {
            if arg.starts_with('-') {
                let key = arg.trim_start_matches('-').to_string();
                let value = iter
                    .peek()
                    .filter(|next| !next.starts_with('-'))
                    .map(|next| next.to_string());

                if value.is_some() {
                    iter.next();
                }

                result.push((key, value));
            }
        }

        result
    }
}

fn approximate_display_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;

    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        } else if in_escape {
            if c == 'm' || c.is_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        width += if c.is_ascii() { 1 } else { 2 };
    }
    width
}

fn format_help_line(name: &str, desc: &str, default: &str) -> String {
    let name_colored = format!("\x1b[32m{}\x1b[0m", name);
    let name_width = approximate_display_width(&name_colored);
    let name_padding = " ".repeat(11usize.saturating_sub(name_width));

    let desc_width = approximate_display_width(desc);
    let desc_padding = " ".repeat(45usize.saturating_sub(desc_width));

    let default_colored = format!("\x1b[2m{}\x1b[0m", default);
    let default_width = approximate_display_width(&default_colored);
    let default_padding = " ".repeat(15usize.saturating_sub(default_width));

    format!(
        " {}{}{}{}{}{}\n",
        name_colored,
        name_padding,
        desc,
        desc_padding,
        default_colored,
        default_padding
    )
}

pub fn print_help() {
    let default = ServiceConfig::default();
    let default_addr = default.addr.to_string();
    let default_delay_limit = default.delay_limit.to_string();
    let default_tlr = default.tlr.to_string();
    let default_max_sticky_slots = default.max_sticky_slots.to_string();
    let default_ips = default.ips.to_string();
    let default_threads = default.threads.to_string();
    let default_tls_port = default.tls_port.to_string();
    let default_http_port = default.http_port.to_string();
    #[cfg(feature = "web")]
    let default_api_addr = crate::api::ApiConfig::default().api_addr.to_string();

    let help_args = vec![
        ("-addr", "本地监听的 IP 和端口", default_addr.as_str()),
        #[cfg(feature = "web")]
        ("-api", "API 服务地址和端口，端口 0 自动分配", default_api_addr.as_str()),
        ("-colo", "筛选一个或多个数据中心，例如 HKG,LAX", "未指定"),
        ("-dl", "有效连接的平均延迟上限（毫秒）", default_delay_limit.as_str()),
        ("-tlr", "有效连接的平均丢包率上限", default_tlr.as_str()),
        ("-http", "测速地址", default.http.as_str()),
        ("-s", "最大负载槽数", default_max_sticky_slots.as_str()),
        ("-ips", "目标负载数量", default_ips.as_str()),
        ("-n", "延迟测速并发上限", default_threads.as_str()),
        ("-tp", "TLS 流量使用的端口号", default_tls_port.as_str()),
        ("-p", "HTTP 流量使用的端口号", default_http_port.as_str()),
        ("-f", "从文件读取 IP 或 CIDR", default.ip_file.as_str()),
    ];

    println!("\x1b[1;35m参数说明\x1b[0m\n");

    for (name, desc, default) in help_args.iter() {
        print!("{}", format_help_line(name, desc, default));
    }
}
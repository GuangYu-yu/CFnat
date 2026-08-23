use std::sync::Arc;
use std::io::{self, BufRead, Write};

use cfnat::{
    args::Args,
    core::ServiceState,
};

#[cfg(feature = "web")]
use cfnat::{
    api::{create_router, AppState, ApiConfig},
};

fn print_banner() {
    println!(r#"
    ▄▄▄▄   ▄▄▄▄▄▄▄▄
  ██▀▀▀▀█  ██▀▀▀▀▀▀                        ██
 ██▀       ██        ██▄████▄   ▄█████▄  ███████
 ██        ███████   ██▀   ██   ▀ ▄▄▄██    ██
 ██▄       ██        ██    ██  ▄██▀▀▀██    ██
  ██▄▄▄▄█  ██        ██    ██  ██▄▄▄███    ██▄▄▄
    ▀▀▀▀   ▀▀        ▀▀    ▀▀   ▀▀▀▀ ▀▀     ▀▀▀▀
"#);
}

async fn run(service: Arc<ServiceState>) {
    #[cfg(feature = "web")]
    {
        let config = service.get_config();
        let addr = config.addr;
        let api_addr = config.api_addr
            .map(|a| {
                if a == addr && a.port() != 0 {
                    eprintln!("错误: -api 和 -addr 参数不能使用相同地址: {}", a);
                    std::process::exit(1);
                }
                a
            })
            .unwrap_or_else(|| ApiConfig::default().api_addr);

        let api_state = AppState {
            service: service.clone(),
        };

        let app = create_router(api_state);

        let listener = match tokio::net::TcpListener::bind(api_addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("无法绑定 API 地址 {}: {}", api_addr, e);
                std::process::exit(1);
            }
        };

        let actual_addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("API 服务异常退出: {}", e);
                std::process::exit(1);
            }
        });

        println!("通过 Web 界面控制: http://{} ", actual_addr);
    }

    match service.start() {
        Ok(_) => println!("服务已启动"),
        Err(e) => {
            eprintln!("启动失败: {}", e);
            std::process::exit(1);
        }
    }

    tokio::signal::ctrl_c().await.ok();
}

#[tokio::main]
async fn main() {
    print_banner();

    let service = Arc::new(ServiceState::new());

    if let Some(config) = Args::parse_to_config() {
        service.update_config(config);
        run(service).await;
        return;
    }

    println!();
    println!("使用方式:");
    println!("  1. 按 Enter 使用默认参数启动");
    println!("  2. 传入自定义参数启动");
    println!();
    print!("> ");
    io::stdout().flush().ok();

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    if let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            println!("使用默认参数启动服务...");
        } else if let Some(config) = Args::parse_line_to_config(&line) {
            service.update_config(config);
            println!("使用传入参数启动服务...");
        }
    }

    run(service).await;
}
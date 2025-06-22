use std::net::TcpListener;
use std::thread;
use std::sync::{Arc, Mutex};

mod server;
mod config;
mod filter;
mod client;
mod hub;
mod web;

fn main() {
    let config = match config::Config::load_from_file("aprsserver.toml") {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    let hub = Arc::new(Mutex::new(hub::Hub::new()));
    let hub_web = hub.clone();

    // Start web UI in background
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(web::serve_web_ui("0.0.0.0:8080", hub_web));
    });

    let user_listener = TcpListener::bind(("0.0.0.0", config.user_port)).expect("Could not bind to user port");
    let server_listener = TcpListener::bind(("0.0.0.0", config.server_port)).expect("Could not bind to server port");
    println!("{} listening on ports {} (user) and {} (server)", config.server_name, config.user_port, config.server_port);

    let hub_server = hub.clone();
    let server_thread = thread::spawn(move || {
        for stream in server_listener.incoming() {
            match stream {
                Ok(stream) => {
                    let hub = hub_server.clone();
                    thread::spawn(|| {
                        server::handle_client(stream, hub);
                    });
                }
                Err(e) => {
                    eprintln!("Server port connection failed: {}", e);
                }
            }
        }
    });

    for stream in user_listener.incoming() {
        match stream {
            Ok(stream) => {
                let hub = hub.clone();
                thread::spawn(|| {
                    server::handle_client(stream, hub);
                });
            }
            Err(e) => {
                eprintln!("User port connection failed: {}", e);
            }
        }
    }

    let _ = server_thread.join();
}

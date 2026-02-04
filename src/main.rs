use std::{io, thread};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration};
use clap::Parser;
use env_logger::Builder;
use log::LevelFilter;
use tiny_http::{Request, Response, Server, StatusCode};
use flapit_server::{Message, Protocol};
use flapit_server::Message::{Echo};

type Serial = String;

struct Device {
    stream: TcpStream,
    peer_addr: SocketAddr,
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let level = match args.verbose { true => LevelFilter::Debug, false => LevelFilter::Info };
    Builder::new().filter_module("flapit_server", level).init();

    log::info!("Starting API server on '0.0.0.0:{}'", args.api_port);
    log::info!("Starting device server on '0.0.0.0:{}'", args.device_port);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.device_port))?;
    let http = Server::http(format!("0.0.0.0:{}", args.api_port)).unwrap();

    let devices: Arc<Mutex<HashMap<Serial, Device>>> = Arc::new(Mutex::new(HashMap::new()));
    let devices_clone = Arc::clone(&devices);

    thread::spawn(move || {
        for request in http.incoming_requests() {
            log::debug!("Incoming http request on {} from {}", request.url(), request.remote_addr().unwrap());
            let devices_clone = Arc::clone(&devices);
            let _ = handle_http(request, devices_clone).map_err(|e| eprintln!("Error: {}", e));
        }
    });

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let devices_clone = Arc::clone(&devices_clone);
            thread::spawn(move || {
                let _ = handle_connection(stream, devices_clone).map_err(|e| eprintln!("Error: {}", e));
            });
        }
    }

    Ok(())
}

fn remove_device(serial: &str, peer_addr: SocketAddr, devices: Arc<Mutex<HashMap<Serial, Device>>>) {
    let mut devices = devices.lock().unwrap();
    if let Some(device) = devices.get(serial) {
        if peer_addr == device.peer_addr {
            devices.remove(serial);
            log::info!("{} Removed!", serial);
        }
    }
}

fn handle_http(mut request: Request, devices: Arc<Mutex<HashMap<Serial, Device>>>) -> io::Result<()> {
    if request.url() != "/" {
        request.respond(Response::new_empty(StatusCode::from(404)))?;

        return Ok(());
    }

    let mut content = String::new();
    request.as_reader().read_to_string(&mut content)?;

    let parameters = parse_query_string(&content);

    if !parameters.contains_key("device") || !parameters.contains_key("message") {
        let response = Response::from_string("Missing \"device\" or \"message\" parameter.").with_status_code(StatusCode::from(400));
        request.respond(response)?;

        return Ok(());
    }

    let message = Message::SetCounterValue(parameters["message"].clone());
    let serial = parameters["device"].as_str();

    if let Some(device) = devices.lock().unwrap().get(serial) {
        request.respond(Response::new_empty(StatusCode::from(202)))?;

        let mut protocol = Protocol::with_stream(device.stream.try_clone()?)?;
        if protocol.send_message(&message).is_err() {
            remove_device(serial, device.peer_addr, devices.clone());
        }

        return Ok(());
    }

    let response = Response::from_string(format!("Device {} not found.", parameters["device"])).with_status_code(StatusCode::from(400));
    request.respond(response)?;

    Ok(())
}

fn handle_connection(stream: TcpStream, devices: Arc<Mutex<HashMap<String, Device>>>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let peer_addr = stream.peer_addr()?;
    let mut protocol = Protocol::with_stream(stream.try_clone()?)?;
    let mut serial: Option<String> = None;
    let mut wait_for_echo = false;

    loop {
        let message = match protocol.read_message::<Message>() {
            Ok(m) => m,
            Err(e) if (e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock) && serial.is_some() => {
                match wait_for_echo {
                    true => break,
                    false => {
                        log::debug!("Sending Echo");
                        let _ = protocol.send_message(&Echo());
                        wait_for_echo = true;
                        continue
                    }
                }
            },
            Err(_) => break
        };

        log::debug!("Incoming {:?} [{}]", message, peer_addr);

        match message {
            Message::AuthAssociate(s, _, _) => {
                protocol.send_message(&Message::Ok())?;
                serial = Some(s.clone());
                devices.lock().unwrap().insert(s.clone(), Device { stream: stream.try_clone()?, peer_addr });
                log::info!("{} Associated!", s);
            },
            Echo() => wait_for_echo = false,
            _ => ()
        }
    }

    if serial.is_some() {
        remove_device(&serial.unwrap(), peer_addr, devices.clone());
    }

    Ok(())
}

fn parse_query_string(string: &String) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    for parameter in string.split("&") {
        if let Some((key, value)) = parameter.split_once("=") {
            map.insert(String::from(key), String::from(value));
        }
    }

    map
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value_t=3000)]
    api_port: u16,

    #[arg(short, long, default_value_t=443)]
    device_port: u16,

    #[arg(short, long, default_value_t=false)]
    verbose: bool
}

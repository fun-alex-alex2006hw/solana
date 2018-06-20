extern crate atty;
extern crate bincode;
extern crate env_logger;
extern crate getopts;
extern crate serde_json;
extern crate solana;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;

use atty::{is, Stream as atty_stream};
use bincode::deserialize;
use getopts::Options;
use solana::crdt::get_ip_addr;
use solana::drone::{Drone, DroneRequest};
use solana::mint::MintDemo;
use solana::signature::GenKeys;
use std::io::{stdin, Read};
use std::net::SocketAddr;
use std::env;
use std::process::exit;
use std::sync::{Arc, Mutex};
use tokio_codec::{Decoder, BytesCodec};
use tokio::net::TcpListener;
use tokio::prelude::*;

fn print_usage(program: &str, opts: Options) {
    // TODO: Write this help information
    let mut brief = format!("Usage: cat <transaction.log> | {} [options]\n\n", program);
    brief += "  This is dummy content and still needs updating\n";

    print!("{}", opts.usage(&brief));
}

fn main() {
    env_logger::init();
    let mut opts = Options::new();
    opts.optopt("t", "", "time", "time slice over which to limit token requests to drone");
    opts.optopt("c", "", "cap", "request limit for time slice");
    opts.optflag("h", "help", "print help");
    let args: Vec<String> = env::args().collect();
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    if matches.opt_present("h") {
        let program = args[0].clone();
        print_usage(&program, opts);
        return;
    }
    let time_slice: Option<u64>;
    if matches.opt_present("t") {
        time_slice = matches.opt_str("t").expect("unexpected string from input").parse().ok();
    }
    else {
        time_slice = None;
    }
    let request_cap: Option<u64>;
    if matches.opt_present("c") {
        request_cap = matches.opt_str("c").expect("unexpected string from input").parse().ok();
    }
    else {
        request_cap = None;
    }

    if is(atty_stream::Stdin) {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    let demo: MintDemo = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });

    let mint_keypair = demo.mint.keypair();

    let mut drone_addr: SocketAddr = "0.0.0.0:9900".parse().unwrap();
    drone_addr.set_ip(get_ip_addr().unwrap());
    let mut transactions_addr = drone_addr.clone();
    transactions_addr.set_port(8000);
    let mut requests_addr = drone_addr.clone();
    requests_addr.set_port(8003);
    let drone = Arc::new(Mutex::new(Drone::new(mint_keypair, drone_addr, transactions_addr, requests_addr, time_slice, request_cap)));
    let socket = TcpListener::bind(&drone_addr).unwrap();
    println!("Listening on: {}", drone_addr);
    let done = socket
        .incoming()
        .map_err(|e| println!("failed to accept socket; error = {:?}", e))
        .for_each(move |socket| {
            let drone1 = drone.clone();
            let client_ip = socket.peer_addr().expect("drone peer_addr").ip();
            let framed = BytesCodec::new().framed(socket);
            let (_writer, reader) = framed.split();

            let processor = reader
                .for_each(move |bytes| {
                    let req: DroneRequest = deserialize(&bytes).expect("deserialize packet in drone");
                    println!("req: {:?}", req);
                    let result = drone1.lock().unwrap().check_rate_limit(client_ip);
                    println!("ip check: {:?}", result);
                    let result1 = drone1.lock().unwrap().send_airdrop(req);
                    println!("data: {:?}", result1);
                    Ok(())
                })
                .and_then(|()| {
                    println!("Socket received FIN packet and closed connection");
                    Ok(())
                })
                .or_else(|err| {
                    println!("Socket closed with error: {:?}", err);
                    Err(err)
                })
                .then(|result| {
                    println!("Socket closed with result: {:?}", result);
                    Ok(())
                });
            tokio::spawn(processor)
        });
    tokio::run(done);
}
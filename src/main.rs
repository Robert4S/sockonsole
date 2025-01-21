use std::{
    collections::HashMap,
    fs,
    io::{self, prelude::*, stdin, BufRead, BufReader, BufWriter},
    os::unix::net::{UnixListener, UnixStream},
    process::{Child, Command, Stdio},
    str,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Mutex,
    },
    thread::{self, spawn},
    time::Duration,
};

use clap::Parser;

use serde::Deserialize;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    action: String,
}

#[derive(Deserialize, Clone)]
struct Config {
    command: String,
    response_timeout: u32,
    env_vars: HashMap<String, String>,
}

fn main() {
    let args = Args::parse();

    let config_file = fs::read_to_string("/home/robert/.config/sockonsole/config.toml").unwrap();

    let config: Config = toml::from_str(&config_file).unwrap();

    let (sender, reciever) = channel();
    if args.action == "start" {
        let (serverout, clientout) = start_socket();

        let _ = start_control_socket(sender);
        handle_socket(serverout, clientout, reciever, config);
    } else if args.action == "stop" {
        stop_socket();
    } else if args.action == "connect" {
        connect_socket()
    } else if args.action == "stop" {
        stop_socket();
    }
}

fn start_socket() -> (UnixListener, UnixListener) {
    let _ = fs::remove_file("/var/lib/remoteconsole_server.sock");
    let _ = fs::remove_file("/var/lib/remoteconsole_client.sock");
    let listener = UnixListener::bind("/var/lib/remoteconsole_server.sock").unwrap();
    let listener2 = UnixListener::bind("/var/lib/remoteconsole_client.sock").unwrap();

    (listener, listener2)
}

fn handle_socket(
    serverout: UnixListener,
    clientout: UnixListener,
    rx: Receiver<()>,
    config: Config,
) {
    let child = Command::new(config.command.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(config.env_vars.clone())
        .spawn()
        .unwrap();

    let m = Mutex::new(child);
    serverout.set_nonblocking(true).unwrap();
    clientout.set_nonblocking(true).unwrap();
    loop {
        if let Ok(_) = rx.try_recv() {
            break;
        }

        match serverout.accept() {
            Ok(serverout) => {
                let mut clientoutserv = None;

                'inner: loop {
                    match clientout.accept() {
                        Ok(c) => {
                            clientoutserv = Some(c);
                            break 'inner;
                        }

                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                        Err(e) => {
                            eprintln!("Socket error: {e:?}");
                            break 'inner;
                        }
                    }
                }
                if let Some(clientout) = clientoutserv {
                    handle_conn(
                        serverout.0,
                        clientout.0,
                        &mut m.lock().unwrap(),
                        config.clone(),
                    )
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => eprintln!("Socket error: {e:?}"),
        }
    }
}

fn start_control_socket(tx: Sender<()>) -> UnixListener {
    let _ = fs::remove_file("/var/lib/remoteconsole_control.sock");
    let listener = UnixListener::bind("/var/lib/remoteconsole_control.sock").unwrap();
    let l_clone = listener.try_clone().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let mut buf = [0; 10];
                    stream.read(&mut buf).unwrap();
                    if buf.starts_with(b"stop") {
                        tx.send(()).unwrap();
                        break;
                    }
                }
                Err(e) => eprintln!("Control socket error: {e:?}"),
            }
        }
    });
    l_clone
}

fn handle_conn(
    mut serverout: UnixStream,
    clientout: UnixStream,
    child: &mut Child,
    config: Config,
) {
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (txout, rxout) = channel();

    spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line.unwrap();
            txout.send(line).unwrap()
        }
    });

    let mut line = String::new();
    let mut streamreader = BufReader::new(clientout);
    let mut resp = String::new();
    loop {
        line.clear();
        resp.clear();
        let _ = streamreader.read_line(&mut line).unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        loop {
            match rxout.recv_timeout(Duration::from_millis(config.response_timeout.into())) {
                Ok(line) => {
                    resp.push_str(&line);
                }
                Err(_e) => break,
            }
        }
        if !resp.is_empty() {
            serverout.write_all(resp.as_bytes()).unwrap();
            serverout.write_all(b"\nEND_RESPONSE\n").unwrap();
        }
    }
}

fn stop_socket() {
    if let Ok(mut stream) = UnixStream::connect("/var/lib/remoteconsole_control.sock") {
        stream.write_all(b"stop").unwrap();
    }
}

fn read_until_sequence(reader: &mut impl BufRead, sequence: &[u8]) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut temp_buffer = [0; 1024];
    let sequence_len = sequence.len();

    loop {
        let bytes_read = reader.read(&mut temp_buffer)?;
        if bytes_read == 0 {
            break; // EOF reached
        }

        buffer.extend_from_slice(&temp_buffer[..bytes_read]);

        if buffer
            .windows(sequence_len)
            .any(|window| window == sequence)
        {
            break;
        }
    }

    Ok(buffer)
}

fn connect_socket() {
    let serverout = UnixStream::connect("/var/lib/remoteconsole_server.sock").unwrap();
    let clientout = UnixStream::connect("/var/lib/remoteconsole_client.sock").unwrap();
    let mut stream_writer = BufWriter::new(clientout);
    let mut stream_reader = BufReader::new(serverout);
    let mut line = String::new();
    let stdin = stdin();
    loop {
        line.clear();
        let _ = stdin.read_line(&mut line).unwrap();
        stream_writer.write_all(line.as_bytes()).unwrap();
        let _ = stream_writer.flush();
        let resp = read_until_sequence(&mut stream_reader, b"\nEND_RESPONSE\n").unwrap();

        let resp2 = &resp[..(resp.len() - "\nEND_RESPONSE\n".len())];

        let resp_text = str::from_utf8(resp2).unwrap();
        println!("{resp_text}")
    }
}

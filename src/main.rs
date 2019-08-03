extern crate regex;
extern crate pbr;

use std::{env, process, thread};
use std::net::{TcpStream, IpAddr, Ipv4Addr, Shutdown};
use std::io::{Write, Read};
use std::str::from_utf8;
use std::fs::File;

use regex::Regex;
use pbr::{ProgressBar, Units};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("Invalid parameter count.");
        println!("Usage: ircdl <bot_name> <package_number>");
        process::exit(1);
    }
    let bot_name = args[1].clone();
    let package_number = args[2].clone();
    let dcc_send_regex = Regex::new(r#"DCC SEND "?(.*)"? (\d+) (\d+) (\d+)"#).unwrap();

    let mut stream = TcpStream::connect("irc.rizon.net:6667")?;
    let mut has_joined = false;
    let mut download_finished = false;

    stream.write("NICK randomRustacean\r\n".as_bytes())?;
    stream.write("USER randomRustacean 0 * randomRustacean\r\n".as_bytes())?;

    let handle = thread::spawn(move || {
        let mut message_builder = String::new();
        let mut buffer = [0; 4];
        while !download_finished {
            while !message_builder.contains("\n")  {
                let count = stream.read(&mut buffer[..]).unwrap();
                message_builder.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
            }
            let endline_offset = message_builder.find('\n').unwrap() + 1;
            let message = message_builder.get(..endline_offset).unwrap().to_owned();
            message_builder.replace_range(..endline_offset, "");

            if message.contains("PING") {
                let pong = message.replace("PING", "PONG");
                stream.write(pong.as_bytes()).unwrap();
                if !has_joined {
                    stream.write("JOIN #NIBL\r\n".as_bytes()).unwrap();
                    let xdcc_send_cmd = format!("PRIVMSG {} :xdcc send #{}\r\n", bot_name, package_number);
                    stream.write(xdcc_send_cmd.as_bytes()).unwrap();
                    has_joined = true;
                }
            }
            if dcc_send_regex.is_match(&message) {
                let captures = dcc_send_regex.captures(&message).unwrap();
                let ip_number = captures[2].parse::<u32>().unwrap();
                let ip = IpAddr::V4(Ipv4Addr::from(ip_number));
                let size = captures[4].parse::<usize>().unwrap();

                stream.shutdown(Shutdown::Both).unwrap();
                download(&captures[1], &ip, &captures[3], size).unwrap();
                download_finished = true;
            }
        }
    });

    handle.join().unwrap();
    Ok(())
}

fn download(filename: &str, ip: &IpAddr, port: &str, total_size: usize) -> std::result::Result<(), std::io::Error> {
    println!("Will download {} from {}:{}", filename, ip, port);
    let mut file = File::create(filename)?;
    let mut stream = TcpStream::connect(format!("{}:{}", ip, port))?;
    let mut buffer = [0; 4096];
    let mut progress: usize = 0;
    let mut progress_bar = ProgressBar::new(total_size as u64);
    progress_bar.set_units(Units::Bytes);

    while progress < total_size {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        progress_bar.set(progress as u64);
    }
    println!("End of download");
    stream.shutdown(Shutdown::Both)?;
    file.flush()
}

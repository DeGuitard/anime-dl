extern crate regex;

use std::thread;
use std::net::{TcpStream, IpAddr, Ipv4Addr, Shutdown};
use std::io::{Write, Read};
use std::str::from_utf8;
use regex::Regex;
use std::fs::File;

fn main() -> std::io::Result<()> {
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
            print!("< {}", message);
            message_builder.replace_range(..endline_offset, "");

            if message.contains("PING") {
                let pong = message.replace("PING", "PONG");
                println!("> {}", pong);
                stream.write(pong.as_bytes());
                if !has_joined {
                    stream.write("JOIN #NIBL\r\n".as_bytes());
                    stream.write("PRIVMSG Ginpachi-Sensei :xdcc send #1\r\n".as_bytes());
                    has_joined = true;
                }
            }
            if dcc_send_regex.is_match(&message) {
                let captures = dcc_send_regex.captures(&message).unwrap();
                let ip_number = captures[2].parse::<u32>().unwrap();
                let ip = IpAddr::V4(Ipv4Addr::from(ip_number));
                let size = captures[4].parse::<usize>().unwrap();

                stream.shutdown(Shutdown::Both);
                download(&captures[1], &ip, &captures[3], size).unwrap();
                download_finished = true;
            }
        }
    });

    handle.join().unwrap();
    Ok(())
}

fn download(filename: &str, ip: &IpAddr, port: &str, totalSize: usize) -> std::result::Result<(), std::io::Error> {
    println!("Will download {} from {}:{}", filename, ip, port);
    let mut file = File::create(filename)?;
    let mut stream = TcpStream::connect(format!("{}:{}", ip, port))?;
    let mut buffer = [0; 4096];
    let mut progress: usize = 0;

    while progress < totalSize {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        println!("{}/{}", progress, totalSize);
    }
    println!("End of download");
    stream.shutdown(Shutdown::Both)?;
    file.flush()
}
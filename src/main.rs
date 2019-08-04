extern crate getopts;
extern crate pbr;
extern crate regex;

use std::{env, thread, process};
use std::net::{TcpStream, IpAddr, Ipv4Addr, Shutdown};
use std::io::{Write, Read};
use std::str::from_utf8;
use std::fs::File;

use getopts::{Matches, Options};
use lazy_static::lazy_static;
use pbr::{ProgressBar, Units, MultiBar, Pipe};
use regex::Regex;

lazy_static! {
    static ref DCC_SEND_REGEX: Regex = Regex::new(r#"DCC SEND "?(.*)"? (\d+) (\d+) (\d+)"#).unwrap();
    static ref PING_REGEX: Regex = Regex::new(r#"PING :\d+"#).unwrap();
    static ref JOIN_REGEX: Regex = Regex::new(r#"JOIN :#.*"#).unwrap();
}

fn print_usage(program: &str, opts: Options) {
    let msg = format!("Usage: {} -b BOT -p PACKAGE1[,PACKAGE2,...] [options]", program);
    print!("{}", opts.usage(&msg));
}

struct IRCRequest {
    server: String,
    channel: String,
    nickname: String,
    bot: String,
    packages: Vec<String>,
}

struct DCCSend {
    filename: String,
    ip: IpAddr,
    port: String,
    file_size: usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.reqopt("b", "bot", "IRC Bot name", "NAME")
        .reqopt("p", "package", "DCC package number(s), separated with comma", "NUMBER")
        .optopt("s", "server", "IRC server and port", "DOMAIN:PORT")
        .optopt("n", "nickname", "Nickname to use", "NICK")
        .optopt("c", "channel", "IRC channel to log in", "CHANNEL")
        .optflag("h", "help", "print this help menu");

    // Unfortunately, cannot use getopts to check for a single optional flag
    // https://github.com/rust-lang-nursery/getopts/issues/46
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_usage(&program, opts);
        return;
    }

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(error) => { 
            eprintln!("{}.", error);
            eprintln!("{}", opts.short_usage(&program));
            process::exit(1);
        }
    };

    let request = parse_args(&matches);
    connect_and_download(request);
}

fn parse_args(args: &Matches) -> IRCRequest {
    let default_server = "irc.rizon.net:6667";
    let default_channel = "nibl";
    let default_nickname = "randomRustacean";
    let package_numbers = args.opt_str("p").expect("Package number(s) must be specified.");

    IRCRequest {
        server: args.opt_str("s").unwrap_or(default_server.to_string()),
        channel: args.opt_str("c").unwrap_or(default_channel.to_string()),
        nickname: args.opt_str("n").unwrap_or(default_nickname.to_string()),
        bot: args.opt_str("b").unwrap(),
        packages: package_numbers.split(",").map(String::from).collect::<Vec<_>>()
    }
}

fn connect_and_download(request: IRCRequest) {
    let mut download_handles = Vec::new();
    let mut has_joined = false;
    let mut multi_bar = MultiBar::new();
    let mut stream = log_in(&request).unwrap();

    let mut message_buffer = String::new();
    while download_handles.len() < request.packages.len() {
        let message = read_next_message(&mut stream, &mut message_buffer).unwrap();

        if PING_REGEX.is_match(&message) {
            let pong = message.replace("PING", "PONG");
            stream.write(pong.as_bytes()).unwrap();
            if !has_joined {
                let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                stream.write(channel_join_cmd.as_bytes()).unwrap();
                has_joined = true;
            }
        }
        if JOIN_REGEX.is_match(&message) {
            for package in &request.packages {
                let xdcc_send_cmd = format!("PRIVMSG {} :xdcc send #{}\r\n", request.bot, package);
                stream.write(xdcc_send_cmd.as_bytes()).unwrap();
            }
        }
        if DCC_SEND_REGEX.is_match(&message) {
            let request = parse_dcc_send(&message);
            let mut progress_bar = multi_bar.create_bar(request.file_size as u64);
            let handle = thread::spawn(move || {
                download_file(request, &mut progress_bar).unwrap();
            });
            download_handles.push(handle);
        }
    }
    stream.write("QUIT :my job is done here!\r\n".as_bytes()).unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    multi_bar.listen();
    download_handles.into_iter().for_each(|handle| handle.join().unwrap());
}

fn log_in(request: &IRCRequest) -> Result<TcpStream, std::io::Error> {
    let mut stream = TcpStream::connect(&request.server)?;
    stream.write(format!("NICK {}\r\n", request.nickname).as_bytes())?;
    stream.write(format!("USER {} 0 * {}\r\n", request.nickname, request.nickname).as_bytes())?;
    Ok(stream)
}

fn read_next_message(stream: &mut TcpStream, message_builder: &mut String) -> Result<String, std::io::Error> {
    let mut buffer = [0; 4];
    while !message_builder.contains("\n") {
        let count = stream.read(&mut buffer[..])?;
        message_builder.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
    }
    let endline_offset = message_builder.find('\n').unwrap() + 1;
    let message = message_builder.get(..endline_offset).unwrap().to_string();
    message_builder.replace_range(..endline_offset, "");
    Ok(message)
}

fn parse_dcc_send(message: &String) -> DCCSend {
    let captures = DCC_SEND_REGEX.captures(&message).unwrap();
    let ip_number = captures[2].parse::<u32>().unwrap();
    DCCSend {
        filename: captures[1].to_string(),
        ip: IpAddr::V4(Ipv4Addr::from(ip_number)),
        port: captures[3].to_string(),
        file_size: captures[4].parse::<usize>().unwrap()
    }
}

fn download_file(request: DCCSend, progress_bar: &mut ProgressBar<Pipe>) -> std::result::Result<(), std::io::Error> {
    let mut file = File::create(&request.filename)?;
    let mut stream = TcpStream::connect(format!("{}:{}", request.ip, request.port))?;
    let mut buffer = [0; 4096];
    let mut progress: usize = 0;
    progress_bar.set_units(Units::Bytes);
    progress_bar.message(&format!("{}: ", &request.filename));

    while progress < request.file_size {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        progress_bar.set(progress as u64);
    }
    progress_bar.finish();
    stream.shutdown(Shutdown::Both)?;
    file.flush()?;
    Ok(())
}

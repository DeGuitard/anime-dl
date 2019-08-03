extern crate getopts;
extern crate pbr;
extern crate regex;

use std::{env, thread};
use std::net::{TcpStream, IpAddr, Ipv4Addr, Shutdown};
use std::io::{Write, Read};
use std::str::from_utf8;
use std::fs::File;

use pbr::{ProgressBar, Units};
use regex::Regex;
use getopts::Options;

fn print_usage(program: &str, opts: Options) {
    let msg = format!("Usage: {} -b BOT -p PACKAGE1[,PACKAGE2,...] [options]", program);
    print!("{}", opts.usage(&msg));
}

struct IRCRequest {
    server: String,
    channel: String,
    bot: String,
    package: String
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.reqopt("b", "bot", "IRC Bot name", "NAME")
        .reqopt("p", "package", "DCC package number(s), separated with comma", "NUMBER")
        .optopt("n", "network", "IRC server and port", "DOMAIN:PORT")
        .optopt("c", "channel", "IRC channel to log in", "CHANNEL")
        .optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(error) => { panic!(error.to_string()) }
    };

    if matches.opt_present("h") {
        print_usage(&program, opts);
        return;
    }

    let default_network = "irc.rizon.net:6667";
    let default_channel = "nibl";
    let package_numbers = matches.opt_str("p").expect("Package number(s) must be specified.");

    for package_number in package_numbers.split(",") {
        let request = IRCRequest {
            server: matches.opt_str("n").unwrap_or(default_network.to_string()),
            channel: matches.opt_str("c").unwrap_or(default_channel.to_string()),
            bot: matches.opt_str("b").unwrap(),
            package: package_number.replace("#", "").to_owned(),
        };
        connect_and_download(request);
    }

}

fn connect_and_download(request: IRCRequest) {
    let dcc_send_regex = Regex::new(r#"DCC SEND "?(.*)"? (\d+) (\d+) (\d+)"#).unwrap();
    let ping_regex = Regex::new(r#"PING :\d+"#).unwrap();
    let join_regex = Regex::new(r#"JOIN :#.*"#).unwrap();
    let handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(request.server).unwrap();
        let mut has_joined = false;
        let mut keep_irc_alive = true;

        stream.write("NICK randomRustacean\r\n".as_bytes()).unwrap();
        stream.write("USER randomRustacean 0 * randomRustacean\r\n".as_bytes()).unwrap();

        let mut message_builder = String::new();
        let mut buffer = [0; 4];
        while keep_irc_alive {
            while !message_builder.contains("\n")  {
                let count = stream.read(&mut buffer[..]).unwrap();
                message_builder.push_str(from_utf8(&buffer[..count]).unwrap_or_default());
            }
            let endline_offset = message_builder.find('\n').unwrap() + 1;
            let message = message_builder.get(..endline_offset).unwrap().to_owned();
            message_builder.replace_range(..endline_offset, "");

            if ping_regex.is_match(&message) {
                let pong = message.replace("PING", "PONG");
                stream.write(pong.as_bytes()).unwrap();
                if !has_joined {
                    let xdcc_send_cmd = format!("PRIVMSG {} :xdcc send #{}\r\n", request.bot, request.package);
                    stream.write(xdcc_send_cmd.as_bytes()).unwrap();
                }
            }
            if join_regex.is_match(&message) {
                let channel_join_cmd = format!("JOIN #{}\r\n", request.channel);
                stream.write(channel_join_cmd.as_bytes()).unwrap();
                has_joined = true;
            }
            if dcc_send_regex.is_match(&message) {
                let captures = dcc_send_regex.captures(&message).unwrap();
                let ip_number = captures[2].parse::<u32>().unwrap();
                let ip = IpAddr::V4(Ipv4Addr::from(ip_number));
                let size = captures[4].parse::<usize>().unwrap();

                download(&captures[1], &ip, &captures[3], size).unwrap();
                stream.write("QUIT :job done\r\n".as_bytes()).unwrap();
                stream.shutdown(Shutdown::Both).unwrap();
                keep_irc_alive = false;
            }
        }
    });

    handle.join().unwrap();
}

fn download(filename: &str, ip: &IpAddr, port: &str, total_size: usize) -> std::result::Result<(), std::io::Error> {
    let mut file = File::create(filename)?;
    let mut stream = TcpStream::connect(format!("{}:{}", ip, port))?;
    let mut buffer = [0; 4096];
    let mut progress: usize = 0;
    let mut progress_bar = ProgressBar::new(total_size as u64);
    progress_bar.set_units(Units::Bytes);
    progress_bar.message(&format!("{}: ", filename));

    while progress < total_size {
        let count = stream.read(&mut buffer[..])?;
        file.write(&mut buffer[..count])?;
        progress += count;
        progress_bar.set(progress as u64);
    }
    progress_bar.finish_println("");
    stream.shutdown(Shutdown::Both)?;
    file.flush()?;
    Ok(())
}

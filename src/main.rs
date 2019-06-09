use clap::{App, Arg, SubCommand};

#[allow(dead_code)]
mod api;
mod app;
#[allow(dead_code)]
mod manager;
#[allow(dead_code)]
mod settings;

#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
extern crate simple_logger;

fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let matches = App::new("zinit")
        .author("ThreeFold Tech, https://github.com/threefoldtech")
        .version("0.1")
        .about("A runit replacement")
        .subcommand(
            SubCommand::with_name("init")
                .arg(
                    Arg::with_name("config")
                        .value_name("DIR")
                        .short("c")
                        .long("config")
                        .help("service configurations directory")
                        .default_value("/etc/zinit/"),
                )
                .arg(Arg::with_name("debug").short("d").long("debug").help("run in debug mode"))
                .about("run in init mode, start and maintain configured services"),
        )
        .subcommand(
            SubCommand::with_name("list")
                .about("quick view of current known services and their status"),
        )
        .subcommand(
            SubCommand::with_name("status")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .about("show detailed service status"),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .about("stop service"),
        )
        .subcommand(
            SubCommand::with_name("start")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .about("start service. has no effect if the service is already running"),
        )
        .subcommand(
            SubCommand::with_name("forget")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .about("forget a service. you can only forget a stopped service"),
        )
        .subcommand(
            SubCommand::with_name("monitor")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .about("start monitoring a service. configuration is loaded from server config directory"),
        )
        .subcommand(
            SubCommand::with_name("kill")
                .arg(
                    Arg::with_name("service")
                        .value_name("SERVICE")
                        .required(true)
                        .help("service name"),
                )
                .arg(
                    Arg::with_name("signal")
                        .value_name("SIGNAL")
                        .required(true)
                        .default_value("SIGTERM")
                        .help("signal name (example: SIGTERM)"),
                )
                .about("send a signal to a running service."),
        )
        .get_matches();

    let result = match matches.subcommand() {
        ("init", Some(matches)) => app::init(
            matches.value_of("config").unwrap(),
            matches.is_present("debug"),
        ),
        ("list", _) => app::list(),
        ("status", Some(matches)) => app::status(matches.value_of("service").unwrap()),
        ("stop", Some(matches)) => app::stop(matches.value_of("service").unwrap()),
        ("start", Some(matches)) => app::start(matches.value_of("service").unwrap()),
        ("forget", Some(matches)) => app::forget(matches.value_of("service").unwrap()),
        ("monitor", Some(matches)) => app::monitor(matches.value_of("service").unwrap()),
        ("kill", Some(matches)) => app::kill(
            matches.value_of("service").unwrap(),
            matches.value_of("signal").unwrap(),
        ),
        _ => app::list(), // default command
    };

    match result {
        Ok(_) => return,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

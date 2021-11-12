extern crate serde;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate log;
extern crate tokio;
use anyhow::Result;
use clap::{App, Arg, SubCommand};
use git_version::git_version;

mod app;
mod manager;
mod zinit;

const GIT_VERSION: &str = git_version!(args = ["--tags", "--always", "--dirty=-modified"]);

#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new("zinit")
        .author("ThreeFold Tech, https://github.com/threefoldtech")
        .version(GIT_VERSION)
        .about("A runit replacement")
        .arg(Arg::with_name("socket").value_name("SOCKET").short("s").long("socket").default_value("/var/run/zinit.sock").help("path to unix socket"))
        .arg(Arg::with_name("debug").short("d").long("debug").help("run in debug mode"))
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
                .arg(
                    Arg::with_name("buffer")
                    .value_name("BUFFER")
                    .short("b")
                    .long("buffer")
                    .help("buffer size (in lines) to keep services logs")
                    .default_value("2000")
                )
                .arg(Arg::with_name("container").long("container").help("run in container mode, shutdown on signal"))
                .about("run in init mode, start and maintain configured services"),
        )
        .subcommand(
            SubCommand::with_name("list")
                .about("quick view of current known services and their status"),
        )
        .subcommand(
            SubCommand::with_name("shutdown")
                .about("stop all services and exit"),
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
            SubCommand::with_name("log")
                .arg(
                    Arg::with_name("filter")
                        .value_name("FILTER")
                        .required(false)
                        .help("an optional 'exact' service name")
                )
                .about("view services logs from zinit ring buffer"),
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

    let socket = matches.value_of("socket").unwrap();
    let debug = matches.is_present("debug");
    //let debug = true;
    let result = match matches.subcommand() {
        ("init", Some(matches)) => {
            app::init(
                matches.value_of("buffer").unwrap().parse().unwrap(),
                matches.value_of("config").unwrap(),
                socket,
                matches.is_present("container"),
                debug,
            )
            .await
        }
        ("list", _) => app::list(socket).await,
        ("shutdown", _) => app::shutdown(socket).await,
        // ("log", Some(matches)) => app::log(matches.value_of("filter")),
        ("status", Some(matches)) => {
            app::status(socket, matches.value_of("service").unwrap()).await
        }
        ("stop", Some(matches)) => app::stop(socket, matches.value_of("service").unwrap()).await,
        ("start", Some(matches)) => app::start(socket, matches.value_of("service").unwrap()).await,
        ("forget", Some(matches)) => {
            app::forget(socket, matches.value_of("service").unwrap()).await
        }
        ("monitor", Some(matches)) => {
            app::monitor(socket, matches.value_of("service").unwrap()).await
        }
        ("kill", Some(matches)) => {
            app::kill(
                socket,
                matches.value_of("service").unwrap(),
                matches.value_of("signal").unwrap(),
            )
            .await
        }
        ("log", Some(matches)) => app::logs(socket, matches.value_of("filter")).await,
        _ => app::list(socket).await, // default command
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

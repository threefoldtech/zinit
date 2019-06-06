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
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    let matches = App::new("zinit")
        .author("Muhamad Azmy, https://github.com/muhamadazmy")
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
                .about("run in init mode, start and maintain configured services"),
        )
        .get_matches();

    let result = match matches.subcommand() {
        ("init", Some(matches)) => app::init(matches.value_of("config").unwrap()),
        _ => {
            // TODO: replace with a call to default command
            // this now can be `init` but may be a `status` command
            // would be more appropriate
            println!("try help");
            return;
        }
    };

    match result {
        Ok(_) => return,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}

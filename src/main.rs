mod filesystem;
mod commands;
mod engines;
mod helpers;

use commands::{interactive, up, down, create, status};
use std::default::Default;
use clap::{Arg, App, SubCommand, AppSettings, ArgMatches};
use config::{Config, File};
use std::time::Instant;
use std::io::Write;
use console::Term;

#[macro_use]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_async;
extern crate slog_term;
use slog::Drain;

/// Custom timestamp logger.
///
/// Arguments
///
/// * `io` - The writer.
pub fn timestamp_utc(io: &mut dyn Write) -> std::io::Result<()> {
    write!(io, "{}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"))
}

#[derive(Debug, PartialEq)]
pub enum CommandName {
    UP,
    DOWN,
    INTERACTIVE,
    CREATE,
    STATUS,
}

impl Default for CommandName {
    fn default() -> Self { CommandName::UP }
}

#[derive(Debug, PartialEq)]
pub enum EngineName {
    POSTGRESQL,
    MYSQL,
    SQLITE,
}

impl Default for EngineName {
    fn default() -> Self { EngineName::POSTGRESQL }
}

#[derive(Debug, PartialEq)]
pub enum CreateType {
    FOLDER,
    FILE,
    SPLITFILES,
}

impl Default for CreateType {
    fn default() -> Self { CreateType::FOLDER }
}

#[derive(Debug, Default)]
pub struct Configuration {
    // Up, down & interactive
    command: CommandName,
    url: String,
    engine: EngineName,
    host: String,
    port: u32,
    database: String,
    username: String,
    password: String,
    table: String,
    path: String,
    interactive: bool,
    continue_on_error: bool,
    version: String,
    step: u32,
    debug: bool,

    // Specific to interactive
    interactive_days: u32,

    // Specific to create
    create_name: String,
    create_type: CreateType,
}

/// Extract application parameters submitted by user (from configuration file only).
///
/// # Arguments
///
/// * `args` - Program args.
fn read_config_file(args: &ArgMatches) -> Configuration {
    // Get configuration file name
    let filename = if args.is_present("config") {
        args.value_of("config").unwrap_or("migration")
    } else {
        "migration"
    };

    // Loading file...
    let mut settings = Config::default();
    let _config = settings.merge(File::with_name(filename));

    let mut configuration: Configuration = Default::default();

    // Common configuration
    configuration.engine = match settings.get::<String>("engine") {
        Ok(s) => match &s[..] {
            "mysql" => EngineName::MYSQL,
            "sqlite" => EngineName::SQLITE,
            "postgres" | "postgresql" => EngineName::POSTGRESQL,
            // TODO: better error here...
            _ => EngineName::POSTGRESQL
        },
        Err(_) => EngineName::POSTGRESQL
    };

    configuration.host = match settings.get::<String>("host") {
        Ok(s) => s,
        Err(_) => String::from("127.0.0.1")
    };

    configuration.table = match settings.get::<String>("migration_table") {
        Ok(s) => s,
        Err(_) => String::from("_schema_migration")
    };

    if configuration.engine == EngineName::POSTGRESQL {
        configuration.port = match settings.get::<u32>("port") {
            Ok(s) => s,
            Err(_) => 6379
        };
        configuration.database = match settings.get::<String>("database") {
            Ok(s) => s,
            Err(_) => String::from("postgres")
        };
        configuration.username = match settings.get::<String>("username") {
            Ok(s) => s,
            Err(_) => String::from("postgres")
        };
        configuration.password = match settings.get::<String>("password") {
            Ok(s) => s,
            Err(_) => String::from("")
        };
    } else {
        configuration.port = match settings.get::<u32>("port") {
            Ok(s) => s,
            Err(_) => 3306
        };
        configuration.database = match settings.get::<String>("database") {
            Ok(s) => s,
            Err(_) => String::from("mysql")
        };
        configuration.username = match settings.get::<String>("username") {
            Ok(s) => s,
            Err(_) => String::from("root")
        };
        configuration.password = match settings.get::<String>("password") {
            Ok(s) => s,
            Err(_) => String::from("")
        };
    }

    configuration.path = match settings.get::<String>("path") {
        Ok(s) => s,
        Err(_) => String::from("./migrations")
    };

    configuration
}

/// Extract application parameters submitted by user.
///
/// # Arguments
///
/// * `cmd` - Type of command (down or up)
/// * `args` - Program args.
fn extract_parameters(cmd: &str, args: &ArgMatches) -> Configuration {
    let file_configuration = read_config_file(args);

    let mut configuration = Configuration {
        command: CommandName::UP,
        url: args.value_of("url").unwrap_or("").to_string(),
        engine: file_configuration.engine,
        host: args.value_of("host").unwrap_or(&file_configuration.host).to_string(),
        port: args.value_of("port").unwrap_or(&file_configuration.port.to_string()).parse::<u32>().unwrap_or(file_configuration.port),
        database: args.value_of("database").unwrap_or(&file_configuration.database).to_string(),
        username: args.value_of("username").unwrap_or(&file_configuration.username).to_string(),
        password: file_configuration.password,
        table: args.value_of("migration_table").unwrap_or(&file_configuration.table).to_string(),
        path: args.value_of("path").unwrap_or(&file_configuration.path).to_string(),
        interactive: args.is_present("interactive"),
        continue_on_error: args.is_present("continueonerror"),
        version: args.value_of("version").unwrap_or("").to_string(),
        step: 0,
        debug: args.is_present("debug"),
        interactive_days: 0,
        create_name: args.value_of("name").unwrap_or("").to_string(),
        create_type: CreateType::FOLDER,
    };

    if args.is_present("engine") {
        let engine = args.value_of("engine").unwrap_or("postgresql");
        match engine {
            "mysql" => configuration.engine = EngineName::MYSQL,
            "sqlite" => configuration.engine = EngineName::SQLITE,
            _ => configuration.engine = EngineName::POSTGRESQL
        }
    }

    if args.is_present("password") {
        let term = Term::stdout();
        write!(&term, "Password:").unwrap();
        let password = term.read_secure_line().unwrap();
        configuration.password = password;
    }

    // Specific to interactive command
    if cmd == "interactive" || cmd == "status" {
        if cmd == "interactive" {
            configuration.command = CommandName::INTERACTIVE;
        } else {
            configuration.command = CommandName::STATUS;
        }

        if args.is_present("days") {
            configuration.interactive_days = args.value_of("days").unwrap_or("0").parse::<u32>().unwrap_or(0);
        } else if args.is_present("last-month") {
            configuration.interactive_days = 31;
        }
    }

    // Specific to up command
    if cmd == "up" {
        configuration.step = args.value_of("step").unwrap_or("0").parse::<u32>().unwrap_or(0);
    }

    // Specific to down command
    if cmd == "down" {
        configuration.command = CommandName::DOWN;
        if args.is_present("all") {
            configuration.step = 0;
        } else {
            // Default, if nothing is set, will be 1.
            configuration.step = args.value_of("step").unwrap_or("1").parse::<u32>().unwrap_or(1);
        }
    }

    // Specific to create command
    if cmd == "create" {
        configuration.command = CommandName::CREATE;
        let create_type = args.value_of("type").unwrap_or("folder");
        match create_type {
            "file" | "files" => configuration.create_type = CreateType::FILE,
            "split" | "split-file" | "split-files" => configuration.create_type = CreateType::SPLITFILES,
            _ => configuration.create_type = CreateType::FOLDER
        }
    }

    // Url override everything
    if configuration.url.len() > 0 {
        if configuration.url.starts_with("mysql") == true {
            configuration.engine = EngineName::MYSQL;
        } else if configuration.url.starts_with("postgres") == true || configuration.url.contains("host=") == true {
            configuration.engine = EngineName::POSTGRESQL;
        } else {
            configuration.engine = EngineName::SQLITE;
        }
    }

    configuration
}

/// Run the migration
///
/// # Arguments
///
/// * `configuration` - Configuration of the application
fn apply_command(configuration: &Configuration) -> bool {
    match configuration.command {
        CommandName::CREATE => create::process(configuration),
        CommandName::UP => up::process(configuration),
        CommandName::DOWN => down::process(configuration),
        CommandName::INTERACTIVE => interactive::process(configuration),
        CommandName::STATUS => status::process(configuration),
    }
}



fn main() {
    // Compute the whole time to parse & do everything
    let whole_application_time = Instant::now();

    // Starting logger
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).use_custom_timestamp(timestamp_utc).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let guard = slog_scope::set_global_logger(slog::Logger::root(slog::Logger::root(drain, o!()), o![]));

    // Command line arguments & parsing
    let base = SubCommand::with_name("base")
        .setting(AppSettings::DeriveDisplayOrder)
        .about("base")
        .arg(Arg::with_name("url")
            .short("u")
            .long("url")
            .value_name("URL")
            .help("Set the url used to connect to database")
            .conflicts_with_all(&["config", "engine", "host", "port", "database", "username", "password"])
            .takes_value(true))
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Load config file [default: migration.(json|hjson|yml|toml)]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("engine")
            .short("e")
            .long("engine")
            .value_name("ENGINE")
            .help("Define which engine [default: postgresql]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("host")
            .short("h")
            .long("host")
            .value_name("HOST")
            .help("Set the database host [default: 127.0.0.1]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("port")
            .short("p")
            .long("port")
            .value_name("PORT")
            .help("Set the database port [default: 6379 (postgres) | 3306 (mysql)]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("database")
            .short("d")
            .long("database")
            .value_name("DATABASE")
            .help("Set the database name [default: postgres (postgres) | mysql (mysql)]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("username")
            .short("U")
            .long("username")
            .value_name("USERNAME")
            .help("Set the database username [default: postgres (postgres) | root (mysql)]")
            .conflicts_with("url")
            .takes_value(true))
        .arg(Arg::with_name("password")
            .short("W")
            .long("password")
            .help("Set the database password")
            .conflicts_with("url")
            .takes_value(false))
        .arg(Arg::with_name("path")
            .long("path")
            .value_name("PATH")
            .help("Folder to locate migration scripts [default: ./migrations]")
            .takes_value(true))
        .arg(Arg::with_name("debug")
            .long("debug")
            .help("If set, this parameter will only print the configuration and do nothing")
            .takes_value(false));

    // Create command
    let mut create = base.clone();
    create = create.name("create")
        .about("Create a new migration file")
        .arg(Arg::with_name("type")
            .long("type")
            .value_name("TYPE")
            .help("Create a folder containing up and down files [default: folder]")
            .takes_value(true))
        .arg(Arg::with_name("name")
            .value_name("MIGRATION_NAME")
            .help("The migration's name")
            .required(true));

    // Up is a copy of base with the version...
    let mut up = base.clone();
    up = up.name("up")
        .about("migrate database")
        .arg(Arg::with_name("version")
            .long("version")
            .value_name("VERSION")
            .help("Take care of only one specific migration script (based on timestamp)")
            .conflicts_with("step")
            .takes_value(true))
        .arg(Arg::with_name("migration_table")
            .long("migration_table")
            .short("t")
            .value_name("TABLE_NAME")
            .help("Set the default migration table name")
            .takes_value(true))
        .arg(Arg::with_name("step")
            .long("step")
            .value_name("NUMBER_OF_STEP")
            .help("Rollback X step(s) from the last found in database")
            .conflicts_with("version")
            .takes_value(true))
        .arg(Arg::with_name("continueonerror")
            .long("continue-on-error")
            .help("Continue if an error is encoutered (not recommended)"));

    // Interactive also supports version but it's a different thing...
    let mut interactive = base.clone();
    interactive = interactive.name("interactive")
        .about("migrate up/down in an easy way")
        .arg(Arg::with_name("version")
            .long("version")
            .value_name("VERSION")
            .help("Start from the given version (don't care of previous ones)")
            .takes_value(true))
        .arg(Arg::with_name("migration_table")
            .long("migration_table")
            .short("t")
            .value_name("TABLE_NAME")
            .help("Set the default migration table name")
            .takes_value(true))
        .arg(Arg::with_name("days")
            .long("days")
            .value_name("NUMBER_OF_DAYS")
            .help("How many days back we should use for the interactive mode (any migration before X days will not be shown)")
            .takes_value(true))
        .arg(Arg::with_name("last-month")
            .long("last-month")
            .help("Same as days except it automatically takes 31 days")
            .takes_value(false));

    let mut status = interactive.clone();
    status = status.name("status")
        .about("check the database status regarding migrations");

    // Down is a copy of up with the step...
    let mut down = base.clone();
    down = down.name("down")
           .about("rollback database")
        .arg(Arg::with_name("version")
           .long("version")
           .value_name("VERSION")
           .help("Take care of only one specific migration script (based on timestamp)")
           .conflicts_with("step")
           .takes_value(true))
        .arg(Arg::with_name("continueonerror")
           .long("continue-on-error")
           .help("Continue if an error is encoutered (not recommended)"))
        .arg(Arg::with_name("migration_table")
           .long("migration_table")
           .short("t")
           .value_name("TABLE_NAME")
           .help("Set the default migration table name")
           .takes_value(true))
        .arg(Arg::with_name("step")
           .long("step")
           .value_name("NUMBER_OF_STEP")
           .help("Rollback X step(s) from the last found in database")
           .conflicts_with("version")
           .takes_value(true))
        .arg(Arg::with_name("all")
            .long("all")
            .help("If set, will rollback everything (dangerous)")
            .takes_value(false));

    let matches = App::new("Migration")
        .version("0.1.0")
        .about("Handle migration of database schema")
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::SubcommandRequired)
        .subcommand(create)
        .subcommand(up)
        .subcommand(down)
        .subcommand(interactive)
        .subcommand(status)
        .get_matches();

    let mut configuration: Configuration = Default::default();

    // Selecting the right sub-command to run
    match matches.subcommand() {
        ("create", Some(create_matches)) => {
            configuration = extract_parameters("create", &create_matches);
        },
        ("up", Some(up_matches)) => {
            configuration = extract_parameters("up", &up_matches);
        }
        ("down", Some(down_matches)) => {
            configuration = extract_parameters("down", &down_matches);
        }
        ("interactive", Some(interactive_matches)) => {
            configuration = extract_parameters("interactive", &interactive_matches);
        }
        ("status", Some(status_matches)) => {
            configuration = extract_parameters("status", &status_matches);
        }
        ("", None) => info!("Use --help to get started with"),
        _ => unreachable!(), // If all sub-commands are defined above, anything else is unreachable!()
    }

    // Starting the application
    let result = apply_command(&configuration);
    let time_taken = &helpers::readable_time(whole_application_time.elapsed().as_millis());

    match result {
        true => debug!("done, took {}", time_taken),
        false => {
            crit!("failed, took {}", time_taken);
            drop(guard);
            std::process::exit(1);
        },
    }
}

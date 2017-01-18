#[macro_use]
extern crate clap;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate jsonnet;
extern crate yaml_rust;
#[macro_use]
extern crate json;
#[macro_use]
extern crate error_chain;

mod errors {
    error_chain! {
        foreign_links {
            Temp(::std::io::Error);
        }

        errors {
            UnknownOutputFormat(v: String) {
                description("Unknown output format")
                display("Unknown output format: '{}'", v)
            }
        }
    }
}

mod emitters;
mod kutils;

use clap::{Arg, App, SubCommand, AppSettings, Shell, ArgGroup};
use jsonnet::{jsonnet_version,JsonnetVm};

use errors::*;
use emitters::OutputFormat;
use kutils::JsonValueExt;

#[cfg(unix)]
const JPATH_DELIMITER: &'static str = ":";
#[cfg(windows)]
const JPATH_DELIMITER: &'static str = ";";

fn parse_kv(s: &str) -> (&str, &str) {
    match s.find('=') {
        Some(i) => (&s[..i], &s[i+1..]),
        None => (s, ""),
    }
}

#[test]
fn test_parse_kv() {
    assert_eq!(parse_kv("foo=bar"), ("foo", "bar"));
    assert_eq!(parse_kv("foo"), ("foo", ""));
    assert_eq!(parse_kv("foo="), ("foo", ""));
}

fn build_cli<'a>(version: &'a str) -> App<'a, 'a> {
    App::new("Kubecfg")
        .setting(AppSettings::SubcommandRequired)
        .setting(AppSettings::VersionlessSubcommands)
        .version(version)
        .author(crate_authors!())
        .about("Synchronise Kubernetes resources with config files")
        .arg(Arg::with_name("server")
             .short("s")
             .long("server")
             .default_value("localhost:8001")
             .value_name("HOST:PORT")
             .help("The address and port of the Kubernetes API server"))
        .arg(Arg::with_name("namespace")
             .short("n")
             .long("namespace")
             .value_name("NS")
             .help("The namespace for this request"))
        .subcommand(SubCommand::with_name("completions")
                    .about("Generate shell completions")
                    .arg(Arg::with_name("shell")
                         .possible_values(&Shell::variants())
                         .required(true)
                         .help("Shell variant")))
        .subcommand(SubCommand::with_name("show")
                    .about("Show expanded resource definition")
                    .arg(Arg::with_name("format")
                         .long("format")
                         .possible_values(&OutputFormat::variants())
                         .default_value(OutputFormat::default())
                         .value_name("FMT")
                         .help("Output format"))
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .value_delimiter(JPATH_DELIMITER)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("exec")
                         .short("e")
                         .long("exec")
                         .value_name("EXPR")
                         .help("Jsonnet expression"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .help("Input file"))
                    .group(ArgGroup::with_name("value")
                           .args(&["exec", "file"])
                           .required(true)))
        .subcommand(SubCommand::with_name("create")
                    .about("Create resources only if they do not exist")
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .value_delimiter(JPATH_DELIMITER)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
        .subcommand(SubCommand::with_name("delete")
                    .about("Delete named resources"))
        .subcommand(SubCommand::with_name("update")
                    .about("Update existing resources")
                    .arg(Arg::with_name("create")
                         .long("create")
                         .help("Create missing resources")))
        .subcommand(SubCommand::with_name("check")
                    .about("Validate file against jsonschema")
                    .arg(Arg::with_name("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
}

fn init_vm_options<'a>(vm: &mut JsonnetVm, matches: &::clap::ArgMatches<'a>) {
    if let Some(paths) = matches.values_of_os("jpath") {
        for path in paths {
            vm.jpath_add(path);
        }
    }

    if let Some(vars) = matches.values_of("ext-var") {
        for (var, val) in vars.map(parse_kv) {
            vm.ext_var(var, val);
        }
    }
}

fn eval_file_or_snippet<'a, 'b>(vm: &'b mut JsonnetVm, matches: &::clap::ArgMatches<'a>) -> Result<&'b str> {
    let result = if let Some(filename) = matches.value_of_os("file") {
        vm.evaluate_file(filename)
    } else if let Some(expr) = matches.value_of("exec") {
        vm.evaluate_snippet("exec", expr)
    } else {
        unreachable!()
    };

    result
        .map(|v| v.as_str())
        .map_err(|e| e.as_str().to_owned().into())
}

fn main() {
    if let Err(ref e) = main_() {
        use ::std::io::Write;
        let stderr = &mut ::std::io::stderr();
        let errmsg = "Error writing to stderr";

        writeln!(stderr, "error: {}", e).expect(errmsg);

        for e in e.iter().skip(1) {
            writeln!(stderr, "caused by: {}", e).expect(errmsg);
        }

        // Run with RUST_BACKTRACE=1 to generate a backtrace.
        if let Some(backtrace) = e.backtrace() {
            writeln!(stderr, "backtrace: {:?}", backtrace).expect(errmsg);
        }

        ::std::process::exit(1);
    }
}

fn main_() -> Result<()> {
    env_logger::init()
        .chain_err(|| "Error initialising logging")?;

    let version = format!("{} (jsonnet {})", crate_version!(), jsonnet_version());
    let matches = build_cli(&version).get_matches();

    if let Some(ref matches) = matches.subcommand_matches("completions") {
        let shell = value_t!(matches, "shell", Shell)
            .unwrap_or_else(|e| e.exit());
        build_cli(&version).gen_completions_to("kubecfg", shell, &mut std::io::stdout());

    } else if let Some(ref matches) = matches.subcommand_matches("show") {
        let mut vm = JsonnetVm::new();
        init_vm_options(&mut vm, matches);

        let json_text = eval_file_or_snippet(&mut vm, matches)?;

        let json = json::parse(json_text)
            .chain_err(|| "Unable to parse jsonnet output")?;

        let output: OutputFormat = matches.value_of("format").unwrap().parse()?;
        output.emit(&json, ::std::io::stdout())?;

    } else if let Some(ref matches) = matches.subcommand_matches("create") {
        let mut vm = JsonnetVm::new();
        init_vm_options(&mut vm, matches);

        let filename = matches.value_of_os("file").unwrap();
        let json = vm.evaluate_file(filename)
            .map_err(|e| e.as_str().to_owned())?;

        let parsed = json::parse(&json)
            .chain_err(|| "Unable to parse jsonnet output")?;

        let objects = kutils::sort_for(kutils::Operation::Create, &parsed);

        for o in objects {
            println!("name: {} {}", o.k8s_kind(), o.k8s_name());
            println!("object: {:?}", o);
        }

        // next step: hyper

        unimplemented!();
    } else if let Some(ref _matches) = matches.subcommand_matches("delete") {
        unimplemented!();
    } else if let Some(ref _matches) = matches.subcommand_matches("update") {
        unimplemented!();
    } else if let Some(ref _matches) = matches.subcommand_matches("check") {
        unimplemented!();
    } else {
        unreachable!();
    }

    Ok(())
}

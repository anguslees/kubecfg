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
extern crate hyper;
extern crate url;
extern crate hyper_native_tls;

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

            Kubernetes(v: ::json::JsonValue) {
                description("Error from Kubernetes server")
                display("Error from Kubernetes: {}",
                        if v["message"].is_empty() {
                            &v["reason"]
                        } else {
                            &v["message"]
                        })
            }
        }
    }
}

mod emitters;
mod kutils;
mod diff;

use clap::{Arg,App,SubCommand,AppSettings,Shell,ArgGroup,ArgMatches};
use jsonnet::{jsonnet_version,JsonnetVm};
use url::Url;
use hyper::Client;
use hyper::header::{ContentType,Accept};
use hyper::net::HttpsConnector;
use hyper_native_tls::NativeTlsClient;
use json::JsonValue;
use std::ffi::OsStr;
use std::io::{self,Write};
use std::env;

use errors::*;
use emitters::OutputFormat;
use kutils::{JsonValueExt,kube_result};

const JPATH_ENVVAR: &'static str = "KUBECFG_JPATH";

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
             .default_value("http://localhost:8001/")
             .value_name("URL")
             .help("The URL of the Kubernetes API server"))
        .arg(Arg::with_name("namespace")
             .short("n")
             .long("namespace")
             .value_name("STR")
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
                         .short("o")
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
        .subcommand(SubCommand::with_name("diff")
                    .about("Show differences between local files and running service")
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .help("Input file")))
        .subcommand(SubCommand::with_name("create")
                    .about("Create resources only if they do not exist")
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
        .subcommand(SubCommand::with_name("delete")
                    .about("Delete named resources")
                    .arg(Arg::with_name("grace_period")
                         .long("grace-period")
                         .value_name("SECS")
                         .help("Period of time in seconds given to the resource to terminate gracefully."))
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
        .subcommand(SubCommand::with_name("update")
                    .about("Update existing resources")
                    .arg(Arg::with_name("create")
                         .long("create")
                         .help("Create missing resources"))
                    .arg(Arg::with_name("wait")
                         .long("wait")
                         .help("Block until update has completed"))
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
        .subcommand(SubCommand::with_name("check")
                    .about("Validate file against jsonschema")
                    .arg(Arg::with_name("jpath")
                         .short("J")
                         .long("jpath")
                         .value_name("DIR")
                         .multiple(true)
                         .help("Additional jsonnet library search path"))
                    .arg(Arg::with_name("file")
                         .value_name("FILE")
                         .required(true)
                         .help("Input file")))
}

struct Context {
    vm: JsonnetVm,
    server_url: Url,
    client: Client,
}

fn init_vm_options<'a>(vm: &mut JsonnetVm, matches: &ArgMatches<'a>) {
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

fn eval_file_or_snippet<'a, 'b>(vm: &'b mut JsonnetVm, matches: &ArgMatches<'a>) -> Result<&'b str> {
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

fn do_show<'a,W>(c: &mut Context, matches: &ArgMatches<'a>, w: W) -> Result<()>
    where W: Write
{
    init_vm_options(&mut c.vm, matches);

    let json_text = eval_file_or_snippet(&mut c.vm, matches)?;

    let json = json::parse(json_text)
        .chain_err(|| "Unable to parse jsonnet output")?;

    let output: OutputFormat = matches.value_of("format").unwrap().parse()?;
    output.emit(&json, w)
}

fn do_create<'a>(c: &mut Context, matches: &ArgMatches<'a>) -> Result<()> {
    init_vm_options(&mut c.vm, matches);

    let filename = matches.value_of_os("file").unwrap();
    let json = c.vm.evaluate_file(filename)
        .map_err(|e| e.as_str().to_owned())?;

    let parsed = json::parse(&json)
        .chain_err(|| "Unable to parse jsonnet output")?;

    let objects = kutils::sort_for(kutils::Operation::Create, &parsed);

    for o in objects {
        let url_path = o.k8s_api_path();
        let url = c.server_url
            .join(&url_path)
            .chain_err(|| "Unable to create URL")?;
        let body = o.dump();

        // TODO: support --record?

        info!("=> POST {}", url);
        let req = c.client.post(url)
            .header(ContentType::json())
            .header(Accept::json())
            .body(&body);

        let resp = req.send()
            .chain_err(|| "Error sending request")?;
        info!("<= {}", resp.status);

        kube_result(resp)?;
    }

    Ok(())
}

fn do_delete<'a>(c: &mut Context, matches: &ArgMatches<'a>) -> Result<()> {
    init_vm_options(&mut c.vm, matches);

    let filename = matches.value_of_os("file").unwrap();
    let json = c.vm.evaluate_file(filename)
        .map_err(|e| e.as_str().to_owned())?;

    let parsed = json::parse(&json)
        .chain_err(|| "Unable to parse jsonnet output")?;

    let objects = kutils::sort_for(kutils::Operation::Delete, &parsed);

    let options: JsonValue = {
        let mut o = kutils::DeleteOptions::default();

        if let Some(n) = matches.value_of("grace_period") {
            let v = n.parse()
                .chain_err(|| "Invalid --grace-period")?;
            o.grace_period_seconds = Some(v);
        }

        o.into()
    };
    let body = options.dump();

    for o in objects {
        let url_path = o.k8s_api_path_named();
        let url = c.server_url
            .join(&url_path)
            .chain_err(|| "Unable to create URL")?;

        info!("DELETE {}", url);
        let req = c.client.delete(url)
            .header(ContentType::json())
            .header(Accept::json())
            .body(&body);

        let resp = req.send()
            .chain_err(|| "Error sending request")?;

        kube_result(resp)?;
    }

    Ok(())
}

fn do_update<'a>(c: &mut Context, matches: &ArgMatches<'a>) -> Result<()> {
    let creat = matches.is_present("create");

    init_vm_options(&mut c.vm, matches);

    let filename = matches.value_of_os("file").unwrap();
    let json = c.vm.evaluate_file(filename)
        .map_err(|e| e.as_str().to_owned())?;

    let parsed = json::parse(&json)
        .chain_err(|| "Unable to parse jsonnet output")?;

    let objects = kutils::sort_for(kutils::Operation::Update, &parsed);

    for o in objects {
        let url_path = o.k8s_api_path_named();
        let url = c.server_url
            .join(&url_path)
            .chain_err(|| "Unable to create URL")?;

        // TODO: set kubernetes.io/change-cause ?
        let body = o.dump();

        info!("=> PATCH {}", url);
        let req = c.client.patch(url)
            .header(ContentType("application/merge-patch+json".parse().unwrap()))
            .header(Accept::json())
            .body(&body);

        let mut resp = req.send()
            .chain_err(|| "Error sending request")?;
        info!("<= {}", resp.status);

        if creat && resp.status == hyper::NotFound {
            // Not found => create
            info!("Creating {}", o.k8s_tname());
            let url_path = o.k8s_api_path();
            let url = c.server_url
                .join(&url_path)
                .chain_err(|| "Unable to create URL")?;

            info!("=> POST {}", url);
            let req = c.client.post(url)
                .header(ContentType::json())
                .header(Accept::json())
                .body(&body);

            resp = req.send()
                .chain_err(|| "Error sending request")?;
            info!("<= {}", resp.status);
        }

        kube_result(resp)?;

        // TODO: Implement --wait.
        // TODO: (Optionally) Show diff between orig and server response
    }

    Ok(())
}

fn do_check<'a>(c: &mut Context, matches: &ArgMatches<'a>) -> Result<()> {
    init_vm_options(&mut c.vm, matches);

    let filename = matches.value_of_os("file").unwrap();
    let json = c.vm.evaluate_file(filename)
        .map_err(|e| e.as_str().to_owned())?;

    let parsed = json::parse(&json)
        .chain_err(|| "Unable to parse jsonnet output")?;

    // TODO: jsonschema validation
    let _ = parsed;
    warn!("jsonschema validation not yet implemented");

    Ok(())
}

fn do_diff<'a,W>(c: &mut Context, matches: &ArgMatches<'a>, mut w: W) -> Result<()>
    where W: Write
{
    init_vm_options(&mut c.vm, matches);

    let filename = matches.value_of_os("file").unwrap();
    let json = c.vm.evaluate_file(filename)
        .map_err(|e| e.as_str().to_owned())?;

    let parsed = json::parse(&json)
        .chain_err(|| "Unable to parse jsonnet output")?;

    let objects = kutils::sort_for(kutils::Operation::Alpha, &parsed);

    // TODO: optionally find everything else already in the namespace

    for o in objects {
        let url_path = o.k8s_api_path_named();
        let url = c.server_url
            .join(&url_path)
            .chain_err(|| "Unable to create URL")?;

        info!("=> GET {}", url);
        let req = c.client.get(url)
            .header(Accept::json());

        let resp = req.send()
            .chain_err(|| "Error sending request")?;
        info!("<= {}", resp.status);

        let existing = if resp.status == hyper::NotFound {
            JsonValue::Null
        } else {
            let mut v = kube_result(resp)?;
            // TODO: more cleaning. `metadata.selfLink`, etc.
            v.remove("status");
            v
        };

        let diffs = diff::diff_walk(0, &existing, o);
        if !diffs.is_empty() {
            writeln!(w, "--- old {}/{}", o.k8s_namespace(), o.k8s_name())?;
            writeln!(w, "+++ new {}/{}", o.k8s_namespace(), o.k8s_name())?;
            for diff in diffs {
                trace!("Got diff: {:?}", diff);
                writeln!(w, "{}", diff)?;
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(ref e) = main_() {
        let stderr = &mut io::stderr();
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

    let mut context = {
        let mut vm = JsonnetVm::new();
        if let Some(paths) = env::var_os(OsStr::new(JPATH_ENVVAR)) {
            for path in env::split_paths(&paths) {
                vm.jpath_add(path);
            }
        }

        let server_url = Url::parse(matches.value_of("server").unwrap())
            .chain_err(|| "Invalid --server URL")?;

        let ssl = NativeTlsClient::new().unwrap();
        let connector = HttpsConnector::new(ssl);
        let client = Client::with_connector(connector);

        Context {
            vm: vm,
            server_url: server_url,
            client: client,
        }
    };

    if let Some(ref matches) = matches.subcommand_matches("completions") {
        let shell = value_t!(matches, "shell", Shell)
            .unwrap_or_else(|e| e.exit());
        build_cli(&version).gen_completions_to("kubecfg", shell, &mut io::stdout());

    } else if let Some(ref matches) = matches.subcommand_matches("show") {
        do_show(&mut context, matches, io::stdout())?

    } else if let Some(ref matches) = matches.subcommand_matches("create") {
        do_create(&mut context, matches)?

    } else if let Some(ref matches) = matches.subcommand_matches("delete") {
        do_delete(&mut context, matches)?

    } else if let Some(ref matches) = matches.subcommand_matches("update") {
        do_update(&mut context, matches)?

    } else if let Some(ref matches) = matches.subcommand_matches("check") {
        do_check(&mut context, matches)?

    } else if let Some(ref matches) = matches.subcommand_matches("diff") {
        do_diff(&mut context, matches, io::stdout())?

    } else {
        unreachable!();
    }

    Ok(())
}

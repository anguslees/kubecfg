use std::io::Write;
use json::JsonValue;
use yaml_rust::{YamlLoader, YamlEmitter};
use std::str::FromStr;
use std::fmt;

use errors::*;

#[derive(Debug,Clone,Copy,PartialEq)]
pub enum OutputFormat {
    Json,
    Yaml,
}

impl FromStr for OutputFormat {
    type Err = Error;
    fn from_str(s: &str) -> Result<OutputFormat> {
        match s {
            "json" => Ok(OutputFormat::Json),
            "yaml" => Ok(OutputFormat::Yaml),
            _ => Err(ErrorKind::UnknownOutputFormat(s.to_owned()).into()),
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self { OutputFormat::Json }
}

impl OutputFormat {
    pub fn variants() -> [&'static str; 2] {
        ["json", "yaml"]
    }
    pub fn default() -> &'static str {
        let d: OutputFormat = Default::default();
        d.variant()
    }

    pub fn variant(&self) -> &'static str {
        match *self {
            OutputFormat::Json => "json",
            OutputFormat::Yaml => "yaml",
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self.variant(), f)
    }
}

#[test]
fn test_variants() {
    use std::collections::btree_set::BTreeSet;

    let def: OutputFormat = Default::default();
    assert_eq!(def.variant(), OutputFormat::default());

    let set: BTreeSet<_> = OutputFormat::variants().iter()
        .cloned()
        .collect();
    assert_eq!(set.len(), OutputFormat::variants().len());
    assert!(set.contains(OutputFormat::default()));
}

fn emit_json<W>(content: &JsonValue, mut w: W) -> Result<()>
    where W: Write
{
    let indent = 4;
    content.write_pretty(&mut w, indent)
        .map_err(|e| e.into())
}

// `YamlEmitter::EmitError` doesn't implement `::std::error::Error` :(
#[derive(Debug, Clone)]
struct YamlEmitError(pub ::yaml_rust::EmitError);

impl From<::yaml_rust::EmitError> for YamlEmitError {
    fn from(e: ::yaml_rust::EmitError) -> Self { YamlEmitError(e) }
}

impl ::std::error::Error for YamlEmitError {
    fn description(&self) -> &str {
        use yaml_rust::EmitError;
        match self.0 {
            EmitError::FmtError(ref e) => e.description(),
            EmitError::BadHashmapKey => "Bad Hashmap Key",
        }
    }

    fn cause(&self) -> Option<&::std::error::Error> {
        use yaml_rust::EmitError;
        match self.0 {
            EmitError::FmtError(ref e) => Some(e),
            EmitError::BadHashmapKey => None,
        }
    }
}

impl ::std::fmt::Display for YamlEmitError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        use yaml_rust::EmitError;
        use ::std::error::Error;
        match self.0 {
            EmitError::FmtError(ref e) => write!(f, "Formatting error: {}", e),
            EmitError::BadHashmapKey => write!(f, "{}", self.description()),
        }
    }
}

fn emit_yaml<W>(content: &JsonValue, mut w: W) -> Result<()>
    where W: Write
{
    let mut buf = String::new();
    {
        let mut yaml_writer = YamlEmitter::new(&mut buf);

        let json = content.dump();
        let docs = YamlLoader::load_from_str(&json)
            .chain_err(|| "Error parsing YAML from JSON")?;
        for doc in docs {
            yaml_writer.dump(&doc)
                .map_err(YamlEmitError)
                .chain_err(|| "Error generating YAML")?;
        }
    }

    w.write_all(buf.as_ref())?;

    Ok(())
}

impl OutputFormat {
    pub fn emit<W>(&self, content: &JsonValue, w: W) -> Result<()>
        where W: Write
    {
        match *self {
            OutputFormat::Json => emit_json(content, w),
            OutputFormat::Yaml => emit_yaml(content, w),
        }
    }
}

#[test]
fn test_json() {
    let v = object!{
        "foo" => 42,
        "bar" => object!{
            "baz" => false
        }
    };

    let emitter = OutputFormat::Json;
    let mut buf = vec![];
    emitter.emit(&v, &mut buf).unwrap();

    let buf_str = String::from_utf8(buf).unwrap();
    assert_eq!(v, ::json::parse(&buf_str).unwrap());
}

#[test]
fn test_yaml() {
    let v = object!{
        "foo" => 42,
        "bar" => object!{
            "baz" => false
        }
    };

    let emitter = OutputFormat::Yaml;
    let mut buf = vec![];
    emitter.emit(&v, &mut buf).unwrap();

    let buf_str = String::from_utf8(buf).unwrap();

    assert_eq!(YamlLoader::load_from_str(&v.to_string()).unwrap(),
               YamlLoader::load_from_str(&buf_str).unwrap());
}

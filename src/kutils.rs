use std::io::{Read,BufRead,BufReader};
use std::fmt;
use json::JsonValue;
use hyper::client::Response;

use errors::*;

#[derive(Debug,PartialEq,Clone,PartialOrd,Eq,Ord)]
pub struct ApiVersion {
    pub group: String,
    pub version: String,
}

impl ApiVersion {
    pub fn is_core(&self) -> bool {
        self.group == "" && self.version == "v1"
    }

    pub fn path_segments(&self, path: &mut ::url::PathSegmentsMut) {
        // TODO: OpenShift V1 API also gets a legacy path too
        // OAPI_V1 => ["oapi", ".", "v1"],
        if self.is_core() {
            path.push("api");
        } else {
            path.push("apis");
            path.push(&self.group);
        }
        path.push(&self.version);
    }
}

impl<'a> From<&'a str> for ApiVersion {
    fn from(s: &'a str) -> Self {
        let (g, v) = match s.find('/') {
            Some(i) => (&s[0..i], &s[i + 1 ..]),
            None => ("", s),
        };
        ApiVersion { group: g.to_owned(), version: v.to_owned() }
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_core() {
            // legacy core API
            write!(f, "{}", self.version)
        } else {
            write!(f, "{}/{}", self.group, self.version)
        }
    }
}

#[derive(Debug,Clone,PartialEq)]
pub struct ApiResource {
    pub name: String,
    pub kind: String,
    pub namespaced: bool,
}

impl ApiResource {
    pub fn new_from_json(v: &JsonValue) -> Result<Self> {
        Ok(ApiResource {
            name: v["name"].as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| ErrorKind::MalformedObject(v.to_owned()))?,
            kind: v["kind"].as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| ErrorKind::MalformedObject(v.to_owned()))?,
            namespaced: v["namespaced"].as_bool()
                .unwrap_or(false),
        })
    }
}

#[derive(Clone,Debug,PartialEq,PartialOrd,Eq,Ord)]
pub struct K8sKind {
    pub api_version: ApiVersion,
    pub kind: String,
}

impl K8sKind {
    pub fn new(api_version: &str, kind: &str) -> Self
    {
        K8sKind{ api_version: api_version.into(), kind: kind.to_owned() }
    }
}

impl fmt::Display for K8sKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.api_version, self.kind)
    }
}

pub const V1_LIST: (&'static str, &'static str) = ("v1", "List");
pub const V1_NAMESPACE: (&'static str, &'static str) = ("v1", "Namespace");
pub const V1_SERVICE: (&'static str, &'static str) = ("v1", "Service");
pub const V1_CONFIGMAP: (&'static str, &'static str) = ("v1", "ConfigMap");
pub const V1_SECRET: (&'static str, &'static str) = ("v1", "Secret");
pub const V1_PVC: (&'static str, &'static str) = ("v1", "PersistentVolumeClaim");
pub const V1BETA1_DEPLOYMENT: (&'static str, &'static str) = ("extensions/v1beta1", "Deployment");

pub trait JsonValueExt {
    fn is_k8s_kind(&self, kind: (&str, &str)) -> bool;
    fn k8s_kind(&self) -> K8sKind;
    fn k8s_name(&self) -> Option<&str>;
    fn k8s_namespace(&self) -> Option<&str>;

    /// Type and name
    fn k8s_tname(&self) -> String {
        let kind = self.k8s_kind();
        format!("{}/{}", kind.kind.to_lowercase(), self.k8s_name().unwrap_or_default())
    }
}

impl JsonValueExt for JsonValue {
    fn is_k8s_kind(&self, kind: (&str, &str)) -> bool {
        self["apiVersion"].as_str() == Some(kind.0) &&
            self["kind"].as_str() == Some(kind.1)
    }

    fn k8s_kind(&self) -> K8sKind {
        K8sKind{
            api_version: self["apiVersion"].as_str().unwrap_or_default().into(),
            kind: self["kind"].as_str().unwrap_or_default().into(),
        }
    }

    fn k8s_name(&self) -> Option<&str> { self["metadata"]["name"].as_str() }

    fn k8s_namespace(&self) -> Option<&str> { self["metadata"]["namespace"].as_str() }
}

fn is_potential_pod_dependency(v: &JsonValue) -> bool {
    v.is_k8s_kind(V1_SERVICE) ||
        v.is_k8s_kind(V1_CONFIGMAP) ||
        v.is_k8s_kind(V1_SECRET) ||
        v.is_k8s_kind(V1_PVC)
}

/// Flatten v1.List objects into a Vec of non-list items
pub fn flatten_list(v: &JsonValue) -> Vec<&JsonValue> {
    ::std::iter::once(v)
        .flat_map(|item| {
            if item.is_k8s_kind(V1_LIST) {
                item["items"].members().collect()
            } else {
                vec![item]
            }
        }).collect()
}

/// Sort key for dependency-first sorting
#[inline]
pub fn dep_first(v: &JsonValue) -> u8 {
    enum Rank {
        First,
        Early,
        Normal,
    }

    let rank = if v.is_k8s_kind(V1_NAMESPACE) { Rank::First }
    else if is_potential_pod_dependency(v) { Rank::Early }
    else { Rank::Normal };

    rank as u8
}

#[derive(Default,Debug)]
pub struct DeleteOptions {
    pub orphan_dependents: bool,
    pub grace_period_seconds: Option<u32>,
    pub preconditions: Vec<String>,
}

impl From<DeleteOptions> for JsonValue {
    fn from(o: DeleteOptions) -> Self {
        let mut res = object!{
            "apiVersion" => "v1",
            "kind" => "DeleteOptions",
            "orphanDependents" => o.orphan_dependents,
            "preconditions" => o.preconditions
        };

        if let Some(n) = o.grace_period_seconds {
            res["gracePeriodSeconds"] = n.into();
        }

        res
    }
}

fn parse_json(s: &str) -> Result<JsonValue> {
    if s.is_empty() {
        Ok(JsonValue::Null)
    } else {
        ::json::parse(s)
            .chain_err(|| "Unable to parse JSON response")
    }
}

pub fn kube_result(mut resp: Response) -> Result<JsonValue> {
    use hyper::mime::{Mime,TopLevel,SubLevel};
    use hyper::header::{ContentType};

    let json = match resp.headers.get::<ContentType>() {
        Some(&ContentType(Mime(TopLevel::Application, SubLevel::Json, _))) => {
            let mut body = String::new();
            resp.read_to_string(&mut body)?;

            parse_json(&body)?
        },
        _ => {
            JsonValue::String(format!("{}", resp.status))
        },
    };

    if resp.status.is_success() {
        Ok(json)
    } else {
        Err(ErrorKind::Kubernetes(json).into())
    }
}

pub fn is_rollout_done(v: &JsonValue) -> bool {
    let observed_gen = v["status"]["observedGeneration"].as_i64().unwrap_or_default();
    let generation = v["metadata"]["generation"].as_i64().unwrap_or_default();
    let updated_replicas = v["status"]["updatedReplicas"].as_i32().unwrap_or_default();
    let replicas = v["spec"]["replicas"].as_i32().unwrap_or_default();

    let is_available = v["status"]["conditions"].members()
        .filter(|c| c["type"] == "Available")
        .all(|c| c["status"] == "True");

    info!("Updated {}/{} replicas (at min availability={})",
          updated_replicas, replicas, is_available);

    observed_gen >= generation &&
        updated_replicas >= replicas &&
        is_available
}

/// `f` returns false to stop watch iteration.
pub fn kube_watch<F>(mut resp: Response, mut f: F) -> Result<bool>
    where F: FnMut(JsonValue) -> Result<bool>
{
    if resp.status.is_success() {
        let chunk_reader = BufReader::new(resp);
        for line in chunk_reader.lines() {
            let json = parse_json(&line?)?;
            if !f(json)? {
                // successful early exit
                return Ok(false);
            }
        }

        // keep going
        Ok(true)
    } else {
        let mut body = String::new();
        resp.read_to_string(&mut body)?;
        let json = parse_json(&body)?;

        Err(ErrorKind::Kubernetes(json).into())
    }
}

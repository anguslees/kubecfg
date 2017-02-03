use std::io::Read;
use std::fmt;
use json::JsonValue;
use hyper::client::Response;

use errors::*;

#[derive(Clone,Copy,Debug,PartialEq)]
pub struct K8sKind<'a> {
    pub api_version: &'a str,
    pub kind: &'a str,
}
impl<'a> fmt::Display for K8sKind<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.api_version, self.kind)
    }
}

const V1_LIST: K8sKind<'static> = K8sKind{api_version: "v1", kind: "List"};
const V1_NAMESPACE: K8sKind<'static> = K8sKind{api_version: "v1", kind: "Namespace"};
const V1_SERVICE: K8sKind<'static> = K8sKind{api_version: "v1", kind: "Service"};
const V1_CONFIGMAP: K8sKind<'static> = K8sKind{api_version: "v1", kind: "ConfigMap"};
const V1_SECRET: K8sKind<'static> = K8sKind{api_version: "v1", kind: "Secret"};
const V1_PVC: K8sKind<'static> = K8sKind{api_version: "v1", kind: "PersistentVolumeClaim"};

pub trait JsonValueExt {
    fn is_k8s_kind(&self, kind: K8sKind) -> bool;
    fn k8s_kind<'a>(&'a self) -> K8sKind<'a>;
    fn k8s_name(&self) -> &JsonValue;
    fn k8s_namespace(&self) -> &JsonValue;

    /// Type and name
    fn k8s_tname(&self) -> String {
        let kind = self.k8s_kind();
        format!("{}/{}", kind.kind.to_lowercase(), self.k8s_name())
    }

    fn k8s_api_path(&self) -> String {
        if self.is_k8s_kind(V1_NAMESPACE) {
            String::from("/api/v1/namespaces")
        } else {
            let kind = self.k8s_kind();
            // TODO: there is some autodiscovery I'm meant to be doing here
            if kind.api_version.starts_with("extension") {
                format!("/apis/{}/namespaces/{}/{}s",
                        kind.api_version.to_lowercase(),
                        self.k8s_namespace(),
                        kind.kind.to_lowercase())
            } else {
                format!("/api/{}/namespaces/{}/{}s",
                        kind.api_version.to_lowercase(),
                        self.k8s_namespace(),
                        kind.kind.to_lowercase())
            }
        }
    }

    fn k8s_api_path_named(&self) -> String {
        format!("{}/{}", self.k8s_api_path(), self.k8s_name())
    }
}
impl JsonValueExt for JsonValue {
    fn is_k8s_kind(&self, kind: K8sKind) -> bool {
        self["apiVersion"] == kind.api_version && self["kind"] == kind.kind
    }

    fn k8s_kind<'a>(&'a self) -> K8sKind<'a> {
        K8sKind{
            api_version: self["apiVersion"].as_str().unwrap_or_default(),
            kind: self["kind"].as_str().unwrap_or_default(),
        }
    }

    fn k8s_name(&self) -> &JsonValue { &self["metadata"]["name"] }

    fn k8s_namespace(&self) -> &JsonValue { &self["metadata"]["namespace"] }
}

fn is_potential_pod_dependency(v: &JsonValue) -> bool {
    v.is_k8s_kind(V1_SERVICE) ||
        v.is_k8s_kind(V1_CONFIGMAP) ||
        v.is_k8s_kind(V1_SECRET) ||
        v.is_k8s_kind(V1_PVC)
}

pub enum Operation {
    Alpha, // "alphabetically" sort, presumably for display
    Create,
    Update,
    Delete,
}

// Flatten v1.List objects, and potentially sort
pub fn sort_for(op: Operation, v: &JsonValue) -> Vec<&JsonValue> {
    // flatten v1.Lists
    let mut v: Vec<_> = ::std::iter::once(v).flat_map(|item| {
        if item.is_k8s_kind(V1_LIST) {
            item["items"].members().collect()
        } else {
            vec![item]
        }
    }).collect();

    let v = match op {
        Operation::Delete => v,
        // Sort PV/PVC last?

        Operation::Create | Operation::Update => {
            enum Rank {
                First,
                Early,
                Normal,
            }
            v.sort_by_key(|item| {
                let rank = if item.is_k8s_kind(V1_NAMESPACE) { Rank::First }
                else if is_potential_pod_dependency(item) { Rank::Early }
                else { Rank::Normal };
                rank as u8
            });
            v
        },

        Operation::Alpha => {
            v.sort_by_key(|item| {
                item["metadata"]["name"].as_str()
            });
            v
        }
    };

    v
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

pub fn kube_result(mut resp: Response) -> Result<JsonValue> {
    let json = {
        let mut body = String::new();
        resp.read_to_string(&mut body)?;
        if body.is_empty() {
            JsonValue::Null
        } else {
            ::json::parse(&body)
                .chain_err(|| "Unable to parse JSON response")?
        }
    };

    if resp.status.is_success() {
        Ok(json)
    } else {
        Err(ErrorKind::Kubernetes(json).into())
    }
}

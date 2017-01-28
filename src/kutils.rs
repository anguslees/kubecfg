use std::fmt;
use json::JsonValue;

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

    fn k8s_name(&self) -> &JsonValue {
        &self["metadata"]["name"]
    }
}

fn is_potential_pod_dependency(v: &JsonValue) -> bool {
    v.is_k8s_kind(V1_SERVICE) ||
        v.is_k8s_kind(V1_CONFIGMAP) ||
        v.is_k8s_kind(V1_SECRET) ||
        v.is_k8s_kind(V1_PVC)
}

pub enum Operation {
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
        Operation::Update | Operation::Delete => v,

        Operation::Create => {
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
    };

    v
}

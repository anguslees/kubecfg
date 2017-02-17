// This file is in `jsonnet` syntax (https://jsonnet.org/).
// Sort of like JSON, but with added lambda calculus.

// Jsonnet can import other files!
// This imports a library of useful Kubernetes-related things.
local kube = import "kube.libsonnet";

local squid = {
  // Jsonnet syntax: Double-colon means this doesn't appear in the
  // generated JSON output.  Think of this like "protected" class
  // properties in C++.
  namespace:: "squid",

  // Jsonnet syntax: `A { ... }` is the same as `A + { ... }` and
  // means "merge".  ie: "copy A, but override some key/values with
  // the following object"
  squid_service: kube.Service("proxy") {
    // Jsonnet can refer to other objects!
    // Jsonnet syntax: `$` means "the current top-level object", so is
    // used to lazily refer to other pieces in this file.
    metadata+: { namespace: $.namespace },
    target_pod: $.squid.spec.template,
    port: 80,
  },

  squid_data: kube.PersistentVolumeClaim("proxy") {
    metadata+: { namespace: $.namespace },
    storage: "10G",
  },

  squid: kube.Deployment("proxy") {
    metadata+: { namespace: $.namespace },

    // Jsonnet syntax: `foo+: { ... }` is short hand for:
    //  foo: super.foo + { ... }
    // (think of it like `+=` in other languages).
    // ie: "merge with the metadata object from kube.Service(..)".
    // Just `foo: { ... }` (no plus) would completely override the
    // `foo` value.
    spec+: {
      template+: {
        spec+: {
          // kube.libsonnet convention: `foo_` is a more "jsonnet
          // native" representation of `foo`.  Typically used to
          // represent K8s "sets" (JSON arrays) as key/value objects,
          // so the merge operation is meaningful.  You can just set
          // `foo` directly if you actually want the K8s-native form.
          containers_+: {
            squid: kube.Container("squid") {
              local container = self,
              image: "jpetazzo/squid-in-a-can",
              env_+: {
                // Jsonnet can do maths!
                // As the squid docs say: "Do NOT put the size of your
                // disk drive here.  Instead, if you want Squid to use
                // the entire disk drive, subtract 20% and use that
                // value."  (in MB)
                DISK_CACHE_SIZE: "%d" % (kube.siToNum($.squid_data.storage) * 0.8 / 1e6),
              },
              ports_+: {
                proxy: { containerPort: 3128 },
              },
              volumeMounts_+: {
                cache: { mountPath: "/var/cache/squid3" },
              },
              livenessProbe: {
                tcpSocket: { port: "proxy" },
              },
              // Jsonnet syntax: `self` refers to the current object (lazily).
              readinessProbe: self.livenessProbe,
            },
          },
          volumes_+: {
            cache: kube.PersistentVolumeClaimVolume($.squid_data),
          },
        },
      },
    },
  },
};

// Jsonnet requires each file to evaluate to a single object.  The
// following idiom allows the generated JSON to be directly understood
// by the K8s server, while also allowing composition with other
// jsonnet files by importing this file and accessing `items_`.
kube.List() { items_+: squid }

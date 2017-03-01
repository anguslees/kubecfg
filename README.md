# kubecfg

A tool for managing complex enterprise Kubernetes environments as code.

kubecfg allows you to express the patterns across your infrastructure
and reuse these powerful "templates" across many services.  The more
complex you infrastructure is, the more you will gain from using
kubecfg.

Status: Basic functionality works, but there are still unimplemented
features and arguments.  If the functionality you want works now, it
should continue to work going forward.

Yes, Google employees will recognise this as being very similar to a
similarly-named internal tool ;)

## Install

Install Rust and cargo.  See https://www.rust-lang.org/install.html

```sh
# `cargo install` installs here, without `--root` arg
PATH=$PATH:$HOME/.cargo/bin

cargo install --git https://github.com/anguslees/kubecfg.git
```

## Quickstart

**kubecfg currently relies on a local `kubectl proxy` to access the
cluster.** It defaults to `http://localhost:8001/` and doesn't support
kubernetes authentication options (yet).

```console
% kubecfg proxy &

# Set kubecfg/jsonnet library search path.  Can also use `-J` args everywhere.
% export KUBECFG_JPATH=$PWD/examples/lib

# Show generated YAML
% kubecfg show -f examples/squid.jsonnet -o yaml

# Create squid (in namespace `squid`)
% kubecfg create -f examples/squid.jsonnet

# (modify squid.jsonnet)
% sed -ie 's/port: 80,/port: 8080,/' examples/squid.jsonnet
# Show differences vs the running job
% kubecfg diff -f examples/squid.jsonnet
# Update to new config
% kubecfg update -f examples/squid.jsonnet
```

## Infrastructure-as-code Philosophy

The idea is to describe *as much as possible* about your configuration
as files in version control (eg: git).

You make changes to the configuration and review, approve, merge, etc
using your regular code change workflow (github pull-requests,
phabricator diffs, etc).  At any point, the config in version control
captures the entire desired-state, so you can easily recreate the
system in a QA cluster or recover from disaster.

Because the configuration is an absolute description (and not some
commands relative to a particular starting condition), you can
create/recreate/upgrade *and downgrade* using the same description[1].
In particular, this means that recovering from a bad change is as
simple as reverting the change in version control and then updating
the cluster to the new (ie: old) configuration.

This is a big deal, with many advantages when maintaining complex
infrastructure with a team of people.  I encourage you to read more
complete discussions of this topic elsewhere.

[1] At least in most cases.  There are still situations involving
schema changes to persistent data, etc that require manual care when
changing versions.

### Recommended Automated Pipeline

An example ideal automated workflow with `kubecfg` using github and
Jenkins' multibranch pipeline plugin would be:

On each pull-request, run `kubecfg check -f $file` on every top-level
file.  Optionally, run `jsonnet fmt --test -f $file` if you want to
enforce local code style guidelines.

On each integration into the `master` branch, run `kubecfg update
--create --wait -f $file` on every top-level file.

## Jsonnet

Kubecfg relies heavily on [jsonnet](http://jsonnet.org/) to describe
Kubernetes resources, and is really just a thin Kubernetes-specific
wrapper around jsonnet evaluation.  You should read the jsonnet
tutorial, and skim the functions available in the jsonnet `std`
library.

### Why jsonnet?

Kubernetes configurations involve a lot of repeated patterns, and
complex deployments will typically have their own local conventions on
top of that.  Jsonnet can import other files, has a strong "merge"
operation, and carefully considered "composition" properties, which
all allow for complex configurations to be managed without getting out
of control.

Jsonnet allows configuration values to be derived from other
configuration values, reducing duplication and avoiding configuration
becoming inconsistent.

Jsonnet contains an `assert` statement, and produces stack traces on
errors, allowing for faster local-turnaround when developing complex
configurations.  Many trivial errors can be caught immediately without
needing to attempt a deployment.

Jsonnet natively produces JSON structures. This removes the quoting
and indenting challenges from hybrid solutions like go-templated YAML.

## Suggested jsonnet Repo Layout

You are welcome to use kubecfg/jsonnet in any way that works for you,
and please tell others about it so they can learn from your
experience.

My suggested configuration layout is (below any particular
subdirectory):

`/lib/*.libsonnet`: Jsonnet utility files that don't represent real
Kubernetes resources. `/lib` should be in `KUBECFG_JPATH` environment
variable (or explicit `-J` args).

`/common/*.jsonnet`: A file for each major component of your
infrastructure, in the style of `examples/squid.jsonnet`.  Most of
your config is here.  In particular, note the idiom of ending each
file with a `kube.List() { ... }` construct.

`/$cluster_name/*.jsonnet`: Specific instantiations of files in
`/common/*.jsonnet` for each cluster.  These are your "top-level"
files for each cluster.  These just import the "common" files and
merge any tweaks required for this specific cluster deployment (eg:
production clusters might need more resources than testing, or a
different `--web.external-url` arg value, etc). These should be as
thin as possible because (eg) anything specific to your production
cluster isn't getting tested in your QA cluster.

With this structure, all `*.libsonnet` and `*.jsonnet` can be passed
through `jsonnet fmt` and `jsonnet eval` lint checks (if desired).
All `*/*.jsonnet` files should be valid Kubernetes objects and satisfy
`kubecfg check`.  The files in `$cluster/*.jsonnet` can be
automatically deployed to `$cluster` as appropriate for your workflow.

Note in particular that in this structure the "objects" being passed
from `kube.libsonnet` -> `common` -> `$cluster` are the actual
Kubernetes JSON objects and not some higher-level (and lossy)
intermediate description.  This structure allows any Kubernetes option
to be tweaked at any level in the "inheritance" tree, without having
to
[explicitly expose every option](https://github.com/kubernetes/charts/blob/master/stable/prometheus/values.yaml) or
make some options unavailable.  Embrace the merge operation.

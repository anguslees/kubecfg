use std::cmp;
use std::collections::BTreeSet;
use std::fmt;
use json::JsonValue;

//  frob:
// +  xyzzy
//    foo:
// +    bar:
// +     subbar
// -    baz

#[derive(Debug,PartialEq)]
pub enum Node<'a> {
    Intermediate(ContextEntry<'a>),
    Leaf(&'a JsonValue),
}

impl<'a> fmt::Display for Node<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Node::Intermediate(ref ctx) => fmt::Display::fmt(ctx, f),
            Node::Leaf(v) => fmt::Display::fmt(v, f),
        }
    }
}

#[derive(Debug,PartialEq)]
pub enum ContextEntry<'a> {
    /// Array index
    Index(usize),
    /// Object key
    Name(&'a str),
}

impl<'a> fmt::Display for ContextEntry<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ContextEntry::Index(i) => write!(f, "{}:", i),
            ContextEntry::Name(n) => write!(f, "{}:", n),
        }
    }
}

#[derive(Debug,PartialEq)]
pub enum Diff<'a> {
    AOnly(usize, Node<'a>),
    BOnly(usize, Node<'a>),
    Both(usize, ContextEntry<'a>),
}

impl<'a> fmt::Display for Diff<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Diff::AOnly(depth, ref node) => {
                write!(f, "- ")?;
                for _ in 0..depth {
                    write!(f, "  ")?;
                }
                write!(f, "{}", node)
            },
            Diff::BOnly(depth, ref node) => {
                write!(f, "+ ")?;
                for _ in 0..depth {
                    write!(f, "  ")?;
                }
                write!(f, "{}", node)
            },
            Diff::Both(depth, ref ctx) => {
                write!(f, "  ")?;
                for _ in 0..depth {
                    write!(f, "  ")?;
                }
                write!(f, "{}", ctx)
            },
        }
    }
}

pub fn diff_walk<'a>(depth: usize, a: &'a JsonValue, b: &'a JsonValue) -> Vec<Diff<'a>> {
    let mut diffs = Vec::new();
    if a.is_array() && b.is_array() {
        for i in 0 .. cmp::min(a.len(), b.len()) {
            let d = diff_walk(depth + 1, &a[i], &b[i]);
            if !d.is_empty() {
                diffs.push(Diff::Both(depth, ContextEntry::Index(i)));
                diffs.extend(d);
            }
        }
        if a.len() > b.len() {
            for i in b.len() .. a.len() {
                diffs.push(Diff::AOnly(depth, Node::Intermediate(ContextEntry::Index(i))));
                diffs.push(Diff::AOnly(depth+1, Node::Leaf(&a[i])));
            }
        } else {
            for i in a.len() .. b.len() {
                diffs.push(Diff::BOnly(depth, Node::Intermediate(ContextEntry::Index(i))));
                diffs.push(Diff::BOnly(depth+1, Node::Leaf(&b[i])));
            }
        }
    } else if a.is_object() && b.is_object() {
        let keys: BTreeSet<_> = a.entries()
            .chain(b.entries())
            .map(|v| v.0)
            .collect();
        let mut keys: Vec<_> = keys.into_iter().collect();
        keys.sort();

        for k in keys {
            if !a.has_key(k) {
                diffs.push(Diff::BOnly(
                    depth, Node::Intermediate(ContextEntry::Name(k))));
                diffs.push(Diff::BOnly(
                    depth+1, Node::Leaf(&b[k])));
            } else if !b.has_key(k) {
                diffs.push(Diff::AOnly(
                    depth, Node::Intermediate(ContextEntry::Name(k))));
                diffs.push(Diff::AOnly(
                    depth+1, Node::Leaf(&a[k])));
            } else {
                let d = diff_walk(depth + 1, &a[k], &b[k]);
                if !d.is_empty() {
                    diffs.push(Diff::Both(depth, ContextEntry::Name(k)));
                    diffs.extend(d);
                }
            }
        }
    } else if a != b {
        diffs.push(Diff::AOnly(depth, Node::Leaf(a)));
        diffs.push(Diff::BOnly(depth, Node::Leaf(b)));
    } else {
        // a == b => No diffs
    }
    diffs
}

#[test]
fn test_diff() {
    let a = "foo".into();
    let b = "bar".into();

    assert_eq!(diff_walk(0, &a, &a), vec![]);
    assert_eq!(diff_walk(0, &a, &b),
               vec![Diff::AOnly(0, Node::Leaf(&a)),
                    Diff::BOnly(0, Node::Leaf(&b))]);

    let x = object!{"x" => "foo"};
    let y = object!{"x" => "bar"};
    assert_eq!(diff_walk(0, &x, &y),
               vec![Diff::Both(0, ContextEntry::Name("x")),
                    Diff::AOnly(1, Node::Leaf(&"foo".into())),
                    Diff::BOnly(1, Node::Leaf(&"bar".into()))]);
}

//! Port of data-schema's lazy.test.ts, one test per case.

use rmx_data_schema::{self as s, BoxSchema, PathSegment, Schema, Value};

#[derive(Debug, Default)]
struct NodeOutput {
    id: String,
    children: Vec<NodeOutput>,
}

fn node() -> BoxSchema<'static, NodeOutput> {
    s::lazy(|| s::object(|o| NodeOutput { id: o.field("id", s::string()), children: o.field("children", s::array(node())) })).boxed()
}

#[test]
fn validates_recursive_schemas() {
    let child = Value::object([("id", Value::from("child")), ("children", Value::Array(Vec::new()))]);
    let root = Value::object([("id", Value::from("root")), ("children", Value::Array(vec![child]))]);
    let value = s::parse(&node(), root).unwrap();
    assert_eq!(value.children.len(), 1);
    assert_eq!((value.id.as_str(), value.children[0].id.as_str()), ("root", "child"));
}

#[test]
fn fails_when_recursive_node_is_invalid() {
    let child = Value::object([("id", Value::from(123)), ("children", Value::Array(Vec::new()))]);
    let root = Value::object([("id", Value::from("root")), ("children", Value::Array(vec![child]))]);
    let issues = s::parse(&node(), root).unwrap_err().issues;
    assert_eq!(issues[0].path, [PathSegment::from("children"), PathSegment::from(0), PathSegment::from("id")]);
}

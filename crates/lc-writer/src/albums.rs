//! Finds albums and album folders that the library already has, for
//! collections missing from the mapping file (for example after the file
//! was lost), so they are adopted instead of created a second time.
//!
//! Rules:
//! - `albums.list {}` answers with a bare array of nodes `{id, name, folder,
//!   children}` (more fields are ignored); the parent of a node is the node
//!   that encloses it; any other shape is a protocol error;
//! - smart albums (`smart: true`) and other nodes never match each other:
//!   a smart collection is only adopted as a smart album, with the same
//!   name, the same parent and `rules` equal (JSON equality) to its
//!   translation;
//! - nodes already recorded in the mapping file for another collection
//!   never match, so a node is adopted at most once;
//! - a collection matches a node with the same name (the Lightroom name
//!   trimmed as the app does on create, then compared byte for byte), the
//!   same folder flag and the same parent (both on top level, or the
//!   parent's LightCraft id);
//! - exactly one matching node and exactly one collection to place with
//!   that key: adopt; no matching node: create; otherwise: ambiguous,
//!   neither created nor adopted.

use crate::control::WriteError;
use serde_json::Value;

/// One node of the library's album tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub id: Value,
    pub name: String,
    pub folder: bool,
    pub smart: bool,
    /// The `rules` of a smart album as listed; `None` for other nodes.
    pub rules: Option<Value>,
    /// The id of the enclosing folder; `None` on top level.
    pub parent: Option<Value>,
}

/// What to do with a collection that is not in the mapping file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Decision {
    /// No node matches: create it.
    Create,
    /// Exactly one node matches and nothing else claims it: adopt this id.
    Adopt(Value),
    /// Several nodes match, or several collections want the one node: the
    /// number of matching nodes.
    Ambiguous(usize),
}

/// The album nodes that may still be adopted.
#[derive(Debug, Default)]
pub(crate) struct Existing {
    /// Album folders and albums.
    nodes: Vec<Node>,
    /// Smart albums.
    smart: Vec<Node>,
}

impl Existing {
    /// The nodes of `list` whose id is not in `claimed` (the ids already in
    /// the mapping file), smart albums apart from the others.
    pub fn new(list: Vec<Node>, claimed: &[Value]) -> Self {
        let (smart, nodes) = list
            .into_iter()
            .filter(|n| !claimed.contains(&n.id))
            .partition(|n| n.smart);
        Existing { nodes, smart }
    }

    /// Decides for a smart collection named `name` (as in Lightroom) below
    /// `parent` (a LightCraft id, `None` on top level) whose translated
    /// rules are `rules`. `wanting` is the number of smart collections to
    /// place with the same trimmed name, parent and rules, this one
    /// included.
    pub fn decide_smart(
        &self,
        name: &str,
        parent: Option<&Value>,
        rules: &Value,
        wanting: usize,
    ) -> Decision {
        let name = name.trim();
        let hits: Vec<&Node> = self
            .smart
            .iter()
            .filter(|n| {
                n.name == name && n.parent.as_ref() == parent && n.rules.as_ref() == Some(rules)
            })
            .collect();
        pick(&hits, wanting)
    }

    /// Decides for a collection named `name` (as in Lightroom) with the
    /// folder flag `folder` below `parent` (a LightCraft id, `None` on top
    /// level). `wanting` is the number of collections to place with the same
    /// trimmed name, folder flag and parent, this one included.
    pub fn decide(
        &self,
        name: &str,
        folder: bool,
        parent: Option<&Value>,
        wanting: usize,
    ) -> Decision {
        let name = name.trim();
        let hits: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|n| n.name == name && n.folder == folder && n.parent.as_ref() == parent)
            .collect();
        pick(&hits, wanting)
    }
}

/// Adopts the one hit if only one collection wants it.
fn pick(hits: &[&Node], wanting: usize) -> Decision {
    match hits {
        [] => Decision::Create,
        [one] if wanting <= 1 => Decision::Adopt(one.id.clone()),
        _ => Decision::Ambiguous(hits.len()),
    }
}

/// Deepest `children` nesting accepted from `albums.list`. The app's album
/// tree is only a few levels deep; a hostile or broken answer must not drive
/// the recursive walk into a stack overflow.
const MAX_ALBUM_DEPTH: usize = 64;

/// The nodes of an `albums.list` answer, parents before children.
pub(crate) fn parse_album_list(value: &Value) -> Result<Vec<Node>, WriteError> {
    let Value::Array(items) = value else {
        return Err(list_error());
    };
    let mut out = Vec::new();
    collect(items, None, 0, &mut out)?;
    Ok(out)
}

fn collect(
    items: &[Value],
    parent: Option<&Value>,
    depth: usize,
    out: &mut Vec<Node>,
) -> Result<(), WriteError> {
    if depth > MAX_ALBUM_DEPTH {
        return Err(WriteError::Protocol(
            "albums.list is nested too deeply".into(),
        ));
    }
    for item in items {
        let Value::Object(map) = item else {
            return Err(list_error());
        };
        let id = match map.get("id") {
            Some(id @ Value::Number(_)) => id.clone(),
            Some(Value::String(s)) if !s.is_empty() => Value::String(s.clone()),
            _ => return Err(list_error()),
        };
        let name = map
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(list_error)?;
        let folder = match map.get("folder") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(list_error()),
        };
        let smart = match map.get("smart") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(list_error()),
        };
        // Rules of an unknown shape simply never match.
        let rules = if smart {
            map.get("rules").cloned()
        } else {
            None
        };
        out.push(Node {
            id: id.clone(),
            name: name.to_string(),
            folder,
            smart,
            rules,
            parent: parent.cloned(),
        });
        match map.get("children") {
            None | Some(Value::Null) => {}
            Some(Value::Array(children)) => collect(children, Some(&id), depth + 1, out)?,
            Some(_) => return Err(list_error()),
        }
    }
    Ok(())
}

fn list_error() -> WriteError {
    WriteError::Protocol("albums.list answered in an unknown format".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: u64, name: &str, folder: bool, parent: Option<u64>) -> Node {
        Node {
            id: json!(id),
            name: name.to_string(),
            folder,
            smart: false,
            rules: None,
            parent: parent.map(|p| json!(p)),
        }
    }

    fn smart(id: u64, name: &str, parent: Option<u64>, rules: Value) -> Node {
        Node {
            smart: true,
            rules: Some(rules),
            ..node(id, name, false, parent)
        }
    }

    #[test]
    fn one_equal_smart_album_is_adopted() {
        let r = json!({"ruleSet": {"match": "all", "rules": []}});
        let e = Existing::new(vec![smart(5, "S", None, r.clone())], &[]);
        assert_eq!(
            e.decide_smart(" S ", None, &r, 1),
            Decision::Adopt(json!(5))
        );
    }

    #[test]
    fn smart_name_parent_and_rules_must_match() {
        let r = json!({"ruleSet": {"match": "all"}});
        let other = json!({"ruleSet": {"match": "any"}});
        let e = Existing::new(vec![smart(5, "S", Some(1), r.clone())], &[]);
        assert_eq!(
            e.decide_smart("s", Some(&json!(1)), &r, 1),
            Decision::Create
        );
        assert_eq!(e.decide_smart("S", None, &r, 1), Decision::Create);
        assert_eq!(
            e.decide_smart("S", Some(&json!(1)), &other, 1),
            Decision::Create
        );
        assert_eq!(
            e.decide_smart("S", Some(&json!(1)), &r, 1),
            Decision::Adopt(json!(5))
        );
    }

    #[test]
    fn smart_and_plain_nodes_never_match_each_other() {
        let r = json!({});
        let e = Existing::new(
            vec![node(5, "A", false, None), smart(6, "B", None, r.clone())],
            &[],
        );
        assert_eq!(e.decide_smart("A", None, &r, 1), Decision::Create);
        assert_eq!(e.decide("B", false, None, 1), Decision::Create);
    }

    #[test]
    fn several_smart_hits_or_claims_are_ambiguous_and_claimed_ones_never_match() {
        let r = json!({});
        let e = Existing::new(
            vec![
                smart(5, "S", None, r.clone()),
                smart(6, "S", None, r.clone()),
                smart(7, "T", None, r.clone()),
            ],
            &[json!(7)],
        );
        assert_eq!(e.decide_smart("S", None, &r, 1), Decision::Ambiguous(2));
        assert_eq!(e.decide_smart("T", None, &r, 1), Decision::Create);
        let one = Existing::new(vec![smart(5, "S", None, r.clone())], &[]);
        assert_eq!(one.decide_smart("S", None, &r, 2), Decision::Ambiguous(1));
    }

    #[test]
    fn smart_rules_are_parsed_only_for_smart_nodes() {
        let v = json!([
            {"id": 1, "name": "A", "rules": {"x": 1}},
            {"id": 2, "name": "S", "smart": true, "rules": {"x": 1}},
        ]);
        let nodes = parse_album_list(&v).unwrap();
        assert_eq!(nodes[0].rules, None);
        assert_eq!(nodes[1].rules, Some(json!({"x": 1})));
    }

    #[test]
    fn parses_the_nested_tree_with_parents() {
        let v = json!([
            {"id": 1, "name": "F", "folder": true, "count": 0, "cover": null, "children": [
                {"id": 2, "name": "A", "folder": false, "count": 3, "cover": 7, "children": []}
            ]},
            {"id": 3, "name": "S", "folder": false, "smart": true, "rules": {}, "children": []}
        ]);
        let nodes = parse_album_list(&v).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0], node(1, "F", true, None));
        assert_eq!(nodes[1], node(2, "A", false, Some(1)));
        assert!(nodes[2].smart);
    }

    /// A tree nested `levels` deep: each level wraps the previous array in
    /// one folder node with a `children` array.
    fn nested(levels: usize) -> Value {
        let mut value = json!([{"id": 0, "name": "leaf", "folder": false}]);
        for i in 1..=levels {
            value = json!([{"id": i, "name": "n", "folder": true, "children": value}]);
        }
        value
    }

    #[test]
    fn nesting_up_to_the_limit_is_accepted() {
        assert!(parse_album_list(&nested(MAX_ALBUM_DEPTH)).is_ok());
    }

    #[test]
    fn deeply_nested_answer_is_rejected() {
        let deep = nested(MAX_ALBUM_DEPTH + 1);
        assert!(
            matches!(parse_album_list(&deep), Err(WriteError::Protocol(_))),
            "a stack-overflowing answer must be a protocol error"
        );
    }

    #[test]
    fn missing_children_are_no_children() {
        let v = json!([{"id": "x", "name": "A", "folder": false}]);
        assert_eq!(parse_album_list(&v).unwrap().len(), 1);
    }

    #[test]
    fn rejects_other_shapes() {
        for v in [
            json!({"albums": "none"}),
            json!({"albums": []}),
            json!(null),
            json!(["a"]),
            json!([{"name": "a", "folder": false}]),
            json!([{"id": "", "name": "a"}]),
            json!([{"id": 1, "folder": false}]),
            json!([{"id": 1, "name": "a", "folder": "yes"}]),
            json!([{"id": 1, "name": "a", "children": {}}]),
            json!([{"id": 1, "name": "a", "children": [1]}]),
        ] {
            assert!(
                matches!(parse_album_list(&v), Err(WriteError::Protocol(_))),
                "{v}"
            );
        }
    }

    #[test]
    fn one_node_and_one_collection_is_adopted() {
        let e = Existing::new(vec![node(5, "A", false, None)], &[]);
        assert_eq!(e.decide(" A\t", false, None, 1), Decision::Adopt(json!(5)));
    }

    #[test]
    fn no_node_creates_even_two_equal_collections() {
        let e = Existing::new(vec![node(5, "A", false, None)], &[]);
        assert_eq!(e.decide("Twin", false, None, 2), Decision::Create);
    }

    #[test]
    fn two_collections_for_one_node_are_ambiguous() {
        let e = Existing::new(vec![node(5, "Twin", false, None)], &[]);
        assert_eq!(e.decide("Twin", false, None, 2), Decision::Ambiguous(1));
    }

    #[test]
    fn two_nodes_are_ambiguous() {
        let e = Existing::new(
            vec![node(5, "A", false, Some(1)), node(6, "A", false, Some(1))],
            &[],
        );
        assert_eq!(
            e.decide("A", false, Some(&json!(1)), 1),
            Decision::Ambiguous(2)
        );
    }

    #[test]
    fn kind_parent_and_case_must_match() {
        let e = Existing::new(
            vec![node(5, "A", true, None), node(6, "A", false, Some(1))],
            &[],
        );
        assert_eq!(e.decide("A", false, None, 1), Decision::Create);
        assert_eq!(e.decide("a", true, None, 1), Decision::Create);
        assert_eq!(e.decide("A", false, Some(&json!(2)), 1), Decision::Create);
    }

    #[test]
    fn claimed_and_smart_nodes_never_match() {
        let mut smart = node(7, "A", false, None);
        smart.smart = true;
        let e = Existing::new(
            vec![node(5, "A", false, None), node(6, "A", false, None), smart],
            &[json!(5)],
        );
        assert_eq!(e.decide("A", false, None, 1), Decision::Adopt(json!(6)));
    }
}

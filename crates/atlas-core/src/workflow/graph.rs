//! The shape rules a workflow graph has to satisfy before it can be stored or run.
//!
//! A graph is one trigger, one output and at least one action wired into a directed
//! acyclic graph where every action sits on a path from the trigger to the output.
//! [`validate`] answers with the action ids in the order a run executes them, so the
//! runner never has to reason about the edges again.

use crate::models::{Graph, Node, NodeData, NodeKind};
use crate::{AtlasError, Result};
use std::collections::{HashMap, HashSet, VecDeque};

fn invalid(msg: impl Into<String>) -> AtlasError {
    AtlasError::Invalid(msg.into())
}

/// The node kind a `data` object is shaped like, for an error message that names both
/// halves of the mismatch.
fn data_kind(d: &NodeData) -> &'static str {
    match d {
        NodeData::Trigger(_) => "trigger",
        NodeData::Action { .. } => "action",
        NodeData::Output { .. } => "output",
    }
}

/// Checks `g` and returns the ids of its action nodes in execution order.
///
/// Rejects: a node without an id, two nodes sharing one, a `data` object that does not
/// match its node's kind, anything other than exactly one trigger and one output, a
/// graph with no actions, two actions whose names match once trimmed and lowercased, an
/// edge naming a node that is not there, a self edge, an edge into the trigger or out of
/// the output, a cycle, an action the trigger cannot reach, and an action that cannot
/// reach the output.
///
/// Order is a topological sort with ties broken by position, top to bottom and then left
/// to right, so two independent branches run in the order the canvas shows them rather
/// than in whatever order the nodes happen to be stored.
pub fn validate(g: &Graph) -> Result<Vec<String>> {
    let mut by_id: HashMap<&str, &Node> = HashMap::new();
    for n in &g.nodes {
        if n.id.trim().is_empty() {
            return Err(invalid("every node needs an id"));
        }
        if by_id.insert(n.id.as_str(), n).is_some() {
            return Err(invalid(format!("two nodes share the id '{}'", n.id)));
        }
        let matches = matches!(
            (n.kind, &n.data),
            (NodeKind::Trigger, NodeData::Trigger(_)) | (NodeKind::Action, NodeData::Action { .. }) | (NodeKind::Output, NodeData::Output { .. })
        );
        if !matches {
            return Err(invalid(format!("node '{}' is a {} node but carries {} data", n.id, n.kind, data_kind(&n.data))));
        }
    }

    let of_kind = |k: NodeKind| g.nodes.iter().filter(move |n| n.kind == k).collect::<Vec<_>>();
    let triggers = of_kind(NodeKind::Trigger);
    let outputs = of_kind(NodeKind::Output);
    let actions = of_kind(NodeKind::Action);
    if triggers.len() != 1 {
        return Err(invalid(format!("a workflow needs exactly one trigger node, found {}", triggers.len())));
    }
    if outputs.len() != 1 {
        return Err(invalid(format!("a workflow needs exactly one output node, found {}", outputs.len())));
    }
    if actions.is_empty() {
        return Err(invalid("a workflow needs at least one action node"));
    }
    let trigger_id = triggers[0].id.as_str();
    let output_id = outputs[0].id.as_str();

    let mut seen_names: HashSet<String> = HashSet::new();
    for n in &actions {
        let NodeData::Action { name, .. } = &n.data else { unreachable!("checked above") };
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return Err(invalid(format!("action '{}' needs a name", n.id)));
        }
        if !seen_names.insert(key) {
            return Err(invalid(format!("two actions are named '{}'", name.trim())));
        }
    }

    let mut edge_ids: HashSet<&str> = HashSet::new();
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut into: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indegree: HashMap<&str, usize> = g.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
    for e in &g.edges {
        if !edge_ids.insert(e.id.as_str()) {
            return Err(invalid(format!("two edges share the id '{}'", e.id)));
        }
        for end in [&e.source, &e.target] {
            if !by_id.contains_key(end.as_str()) {
                return Err(invalid(format!("edge '{}' names a node that is not in the graph: '{end}'", e.id)));
            }
        }
        if e.source == e.target {
            return Err(invalid(format!("edge '{}' loops node '{}' back to itself", e.id, e.source)));
        }
        if e.target == trigger_id {
            return Err(invalid(format!("edge '{}' runs into the trigger; the trigger starts the workflow", e.id)));
        }
        if e.source == output_id {
            return Err(invalid(format!("edge '{}' leaves the output; the output ends the workflow", e.id)));
        }
        out.entry(e.source.as_str()).or_default().push(e.target.as_str());
        into.entry(e.target.as_str()).or_default().push(e.source.as_str());
        *indegree.get_mut(e.target.as_str()).expect("target checked above") += 1;
    }

    // Kahn's algorithm. Among the nodes that are ready at each step the one highest on
    // the canvas wins, then the one furthest left, then the lower id, so the order is a
    // function of the graph alone and never of `Vec` or `HashMap` iteration order.
    let before = |a: &str, b: &str| {
        let (pa, pb) = (by_id[a].position, by_id[b].position);
        pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x)).then_with(|| a.cmp(b))
    };
    let mut ready: Vec<&str> = indegree.iter().filter(|(_, d)| **d == 0).map(|(id, _)| *id).collect();
    let mut order: Vec<&str> = Vec::with_capacity(g.nodes.len());
    while !ready.is_empty() {
        let pick = (0..ready.len()).min_by(|a, b| before(ready[*a], ready[*b])).expect("not empty");
        let id = ready.remove(pick);
        order.push(id);
        for next in out.get(id).into_iter().flatten().copied() {
            let d = indegree.get_mut(next).expect("target checked above");
            *d -= 1;
            if *d == 0 {
                ready.push(next);
            }
        }
    }
    if order.len() != g.nodes.len() {
        return Err(invalid("the graph has a cycle"));
    }

    let forward = reachable(trigger_id, &out);
    let backward = reachable(output_id, &into);
    if !forward.contains(output_id) {
        return Err(invalid("the output node cannot be reached from the trigger"));
    }
    for n in &actions {
        let id = n.id.as_str();
        if !forward.contains(id) {
            return Err(invalid(format!("action '{}' cannot be reached from the trigger", action_name(n))));
        }
        if !backward.contains(id) {
            return Err(invalid(format!("action '{}' does not lead to the output", action_name(n))));
        }
    }

    Ok(order.into_iter().filter(|id| by_id[*id].kind == NodeKind::Action).map(str::to_string).collect())
}

fn action_name(n: &Node) -> &str {
    match &n.data {
        NodeData::Action { name, .. } => name.trim(),
        _ => n.id.as_str(),
    }
}

/// Every node `from` can get to along `adjacency`, `from` included.
fn reachable<'a>(from: &'a str, adjacency: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut seen: HashSet<&str> = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(id) = queue.pop_front() {
        for next in adjacency.get(id).into_iter().flatten().copied() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
}

use std::collections::{BTreeMap, BTreeSet};

use crate::model::GraphEdge;
pub(crate) fn graph_order(
    node_names: &[String],
    edges: &[GraphEdge],
) -> Result<Vec<usize>, Vec<String>> {
    let mut indices = BTreeMap::new();
    let mut errors = Vec::new();
    for (index, name) in node_names.iter().enumerate() {
        if let Some(first) = indices.insert(name.clone(), index) {
            errors.push(format!(
                "Scafra graph has duplicate output `{name}` at node indexes {first} and {index}"
            ));
        }
    }
    let mut dependency_count = vec![0usize; node_names.len()];
    let mut dependents = vec![Vec::new(); node_names.len()];
    for edge in edges {
        let Some(&consumer) = indices.get(&edge.consumer) else {
            errors.push(format!(
                "Scafra graph edge at {} has unknown consumer `{}`",
                edge.source, edge.consumer
            ));
            continue;
        };
        let Some(&dependency) = indices.get(&edge.dependency) else {
            errors.push(format!(
                "Scafra graph is missing dependency `{}` required by `{}` at {}",
                edge.dependency, edge.consumer, edge.source
            ));
            continue;
        };
        dependency_count[consumer] += 1;
        dependents[dependency].push(consumer);
    }
    let mut consumers_by_dependency = BTreeMap::<usize, BTreeSet<usize>>::new();
    for edge in edges {
        let (Some(&consumer), Some(&dependency)) =
            (indices.get(&edge.consumer), indices.get(&edge.dependency))
        else {
            continue;
        };
        let consumers = consumers_by_dependency.entry(dependency).or_default();
        if !consumers.insert(consumer) {
            errors.push(format!(
                "Scafra graph dependency `{}` is consumed more than once by `{}` at {}",
                edge.dependency, edge.consumer, edge.source
            ));
        }
        if consumers.len() > 1 {
            errors.push(format!(
                "Scafra graph dependency `{}` has multiple consumers; owned graph values must have one consumer at {}",
                edge.dependency, edge.source
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for list in &mut dependents {
        list.sort_by_key(|index| node_names[*index].clone());
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, count) in dependency_count.iter().enumerate() {
        if *count == 0 {
            ready.insert((node_names[index].clone(), index));
        }
    }
    let mut order = Vec::new();
    while let Some((_, index)) = ready.pop_first() {
        order.push(index);
        for dependent in &dependents[index] {
            dependency_count[*dependent] -= 1;
            if dependency_count[*dependent] == 0 {
                ready.insert((node_names[*dependent].clone(), *dependent));
            }
        }
    }
    if order.len() == node_names.len() {
        return Ok(order);
    }
    let indices = node_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let cycle = graph_cycle_path(node_names, edges, &indices, &dependency_count);
    let path = cycle
        .iter()
        .map(|index| node_names[*index].as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    let sources = cycle
        .windows(2)
        .filter_map(|pair| {
            edges
                .iter()
                .find(|edge| {
                    indices.get(&edge.consumer) == Some(&pair[0])
                        && indices.get(&edge.dependency) == Some(&pair[1])
                })
                .map(|edge| edge.source.as_str())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    Err(vec![format!(
        "Scafra graph contains a dependency cycle: {path} (declarations: {sources})"
    )])
}

fn graph_cycle_path(
    node_names: &[String],
    edges: &[GraphEdge],
    indices: &BTreeMap<String, usize>,
    remaining: &[usize],
) -> Vec<usize> {
    let remaining = remaining
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count > 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut adjacency = vec![Vec::<usize>::new(); node_names.len()];
    for edge in edges {
        let (Some(&consumer), Some(&dependency)) =
            (indices.get(&edge.consumer), indices.get(&edge.dependency))
        else {
            continue;
        };
        if remaining.contains(&consumer) && remaining.contains(&dependency) {
            adjacency[consumer].push(dependency);
        }
    }
    for neighbors in &mut adjacency {
        neighbors.sort_by_key(|index| node_names[*index].clone());
    }

    fn visit(
        current: usize,
        adjacency: &[Vec<usize>],
        state: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        state[current] = 1;
        stack.push(current);
        for neighbor in &adjacency[current] {
            match state[*neighbor] {
                0 => {
                    if let Some(cycle) = visit(*neighbor, adjacency, state, stack) {
                        return Some(cycle);
                    }
                }
                1 => {
                    let start = stack.iter().position(|index| index == neighbor)?;
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(*neighbor);
                    return Some(cycle);
                }
                _ => {}
            }
        }
        stack.pop();
        state[current] = 2;
        None
    }

    let mut state = vec![0u8; node_names.len()];
    let mut stack = Vec::new();
    for index in &remaining {
        if state[*index] == 0 {
            if let Some(cycle) = visit(*index, &adjacency, &mut state, &mut stack) {
                return cycle;
            }
        }
    }
    remaining.into_iter().collect()
}

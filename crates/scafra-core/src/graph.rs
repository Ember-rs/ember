use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Write as _},
};

use crate::errors::GraphError;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphNodeKind {
    Bean,
    Service,
    Component,
    Repository,
    Controller,
}

/// Static metadata for one typed graph provider.
///
/// The `output` label is used only for validation and reports. Generated
/// composition calls concrete Rust functions and constructors directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphNodeDescriptor {
    pub provider: &'static str,
    pub output: &'static str,
    pub source: &'static str,
    pub kind: GraphNodeKind,
}

/// Static metadata for one dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphEdgeDescriptor {
    pub consumer: &'static str,
    pub dependency: &'static str,
    pub source: &'static str,
}

/// The complete static graph emitted by an opted-in build script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphDescriptor {
    pub nodes: &'static [GraphNodeDescriptor],
    pub edges: &'static [GraphEdgeDescriptor],
}

impl GraphDescriptor {
    /// Validates the graph and computes its deterministic construction order.
    pub fn plan(&self) -> Result<GraphPlan<'_>, GraphError> {
        GraphPlan::build(self)
    }
}

/// The validated, deterministic view of a graph descriptor.
pub struct GraphPlan<'a> {
    descriptor: &'a GraphDescriptor,
    order: Vec<usize>,
}

impl<'a> fmt::Debug for GraphPlan<'a> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphPlan")
            .field("ordered_nodes", &self.ordered_nodes().collect::<Vec<_>>())
            .finish()
    }
}

impl<'a> GraphPlan<'a> {
    fn build(descriptor: &'a GraphDescriptor) -> Result<Self, GraphError> {
        let mut nodes = BTreeMap::new();
        let mut node_indices = (0..descriptor.nodes.len()).collect::<Vec<_>>();
        node_indices.sort_by_key(|index| {
            let node = &descriptor.nodes[*index];
            (node.output, node.provider, node.source)
        });
        for index in node_indices {
            let node = &descriptor.nodes[index];
            if let Some(first_index) = nodes.insert(node.output, index) {
                let first = &descriptor.nodes[first_index];
                return Err(GraphError::DuplicateOutput {
                    output: node.output,
                    first_provider: first.provider,
                    first_source: first.source,
                    second_provider: node.provider,
                    second_source: node.source,
                });
            }
        }

        let mut dependencies = vec![0usize; descriptor.nodes.len()];
        let mut dependents = vec![Vec::<usize>::new(); descriptor.nodes.len()];
        let mut edges = descriptor.edges.iter().collect::<Vec<_>>();
        edges.sort_by_key(|edge| (edge.consumer, edge.dependency, edge.source));
        for edge in edges {
            let Some(&consumer) = nodes.get(edge.consumer) else {
                return Err(GraphError::UnknownConsumer {
                    consumer: edge.consumer,
                    dependency: edge.dependency,
                    source: edge.source,
                });
            };
            let Some(&dependency) = nodes.get(edge.dependency) else {
                return Err(GraphError::MissingDependency {
                    consumer: edge.consumer,
                    dependency: edge.dependency,
                    source: edge.source,
                });
            };
            dependencies[consumer] += 1;
            dependents[dependency].push(consumer);
        }

        for dependent_list in &mut dependents {
            dependent_list.sort_by_key(|index| descriptor.nodes[*index].output);
        }

        let mut ready = BTreeSet::new();
        for (index, dependency_count) in dependencies.iter().enumerate() {
            if *dependency_count == 0 {
                ready.insert((descriptor.nodes[index].output, index));
            }
        }

        let mut order = Vec::with_capacity(descriptor.nodes.len());
        while let Some((_, index)) = ready.pop_first() {
            order.push(index);
            for dependent in &dependents[index] {
                dependencies[*dependent] -= 1;
                if dependencies[*dependent] == 0 {
                    ready.insert((descriptor.nodes[*dependent].output, *dependent));
                }
            }
        }

        if order.len() != descriptor.nodes.len() {
            let (path, sources) = find_cycle(descriptor, &nodes, &dependencies);
            return Err(GraphError::Cycle { path, sources });
        }

        Ok(Self { descriptor, order })
    }

    /// Returns nodes in the order in which their owned values can be created.
    pub fn ordered_nodes(&self) -> impl Iterator<Item = &'a GraphNodeDescriptor> + '_ {
        self.order
            .iter()
            .map(|index| &self.descriptor.nodes[*index])
    }

    /// Renders a stable, human-readable graph report.
    pub fn render(&self) -> String {
        let mut report = String::from("graph:\n");
        let mut nodes = self.descriptor.nodes.iter().collect::<Vec<_>>();
        nodes.sort_by_key(|node| (node.output, node.provider, node.source));
        for node in nodes {
            let _ = writeln!(
                report,
                "  node {} -> {} ({:?}) [{}]",
                node.provider, node.output, node.kind, node.source
            );
        }
        let mut edges = self.descriptor.edges.iter().collect::<Vec<_>>();
        edges.sort_by_key(|edge| (edge.consumer, edge.dependency, edge.source));
        for edge in edges {
            let _ = writeln!(
                report,
                "  edge {} -> {} [{}]",
                edge.consumer, edge.dependency, edge.source
            );
        }
        report.push_str("  order:");
        for node in self.ordered_nodes() {
            let _ = write!(report, " {}", node.output);
        }
        report.push('\n');
        report
    }
}

fn find_cycle(
    descriptor: &GraphDescriptor,
    nodes: &BTreeMap<&'static str, usize>,
    remaining_dependencies: &[usize],
) -> (Vec<&'static str>, Vec<&'static str>) {
    let remaining = remaining_dependencies
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count > 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut adjacency = vec![Vec::<usize>::new(); descriptor.nodes.len()];
    for edge in descriptor.edges {
        if let (Some(&consumer), Some(&dependency)) =
            (nodes.get(edge.consumer), nodes.get(edge.dependency))
        {
            if remaining.contains(&consumer) && remaining.contains(&dependency) {
                adjacency[consumer].push(dependency);
            }
        }
    }
    for neighbors in &mut adjacency {
        neighbors.sort_by_key(|index| descriptor.nodes[*index].output);
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

    let mut state = vec![0u8; descriptor.nodes.len()];
    let mut stack = Vec::new();
    for index in remaining {
        if state[index] == 0 {
            if let Some(cycle) = visit(index, &adjacency, &mut state, &mut stack) {
                let path = cycle
                    .iter()
                    .map(|index| descriptor.nodes[*index].output)
                    .collect::<Vec<_>>();
                let sources = cycle
                    .windows(2)
                    .filter_map(|pair| {
                        descriptor
                            .edges
                            .iter()
                            .find(|edge| {
                                nodes.get(edge.consumer) == Some(&pair[0])
                                    && nodes.get(edge.dependency) == Some(&pair[1])
                            })
                            .map(|edge| edge.source)
                    })
                    .collect::<Vec<_>>();
                return (path, sources);
            }
        }
    }
    (vec!["<unknown cycle>"], Vec::new())
}

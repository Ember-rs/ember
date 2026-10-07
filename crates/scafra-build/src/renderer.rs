use scafra_core::GraphNodeKind;

use crate::model::{GraphModel, ModuleNode};
pub(crate) fn render_graph(model: &GraphModel, output: &mut String) {
    output.push_str("\npub mod __scafra_graph {\n");
    output.push_str("    static NODES: &[::scafra::core::GraphNodeDescriptor] = &[\n");
    for node in &model.nodes {
        output.push_str(&format!(
            "        ::scafra::core::GraphNodeDescriptor {{ provider: {}, output: {}, source: {}, kind: {} }},\n",
            rust_string(&node.provider),
            rust_string(&node.output),
            rust_string(&node.source),
            graph_kind_tokens(node.kind)
        ));
    }
    output.push_str("    ];\n");
    output.push_str("    static EDGES: &[::scafra::core::GraphEdgeDescriptor] = &[\n");
    for edge in &model.edges {
        output.push_str(&format!(
            "        ::scafra::core::GraphEdgeDescriptor {{ consumer: {}, dependency: {}, source: {} }},\n",
            rust_string(&edge.consumer),
            rust_string(&edge.dependency),
            rust_string(&edge.source)
        ));
    }
    output.push_str("    ];\n");
    output.push_str(
        "    static DESCRIPTOR: ::scafra::core::GraphDescriptor = ::scafra::core::GraphDescriptor { nodes: NODES, edges: EDGES };\n\n",
    );
    output.push_str(
        "    pub fn descriptor() -> &'static ::scafra::core::GraphDescriptor { &DESCRIPTOR }\n\n",
    );

    output.push_str("    pub struct Graph {\n");
    for (index, node) in model.nodes.iter().enumerate() {
        if model
            .edges
            .iter()
            .any(|edge| edge.dependency == node.output)
        {
            continue;
        }
        output.push_str(&format!(
            "        pub node_{index}: {},\n",
            node.output_type
        ));
    }
    output.push_str("    }\n\n");
    output.push_str("    #[derive(Default)]\n    pub struct Overrides {\n");
    for (index, node) in model.nodes.iter().enumerate() {
        output.push_str(&format!(
            "        pub node_{index}: Option<{}>,\n",
            node.output_type
        ));
    }
    output.push_str("    }\n\n");
    output.push_str(
        "\n    pub fn compose() -> Result<Graph, ::scafra::core::GraphError> { compose_with(Overrides::default()) }\n\n",
    );
    output.push_str(
        "    pub fn compose_with(mut overrides: Overrides) -> Result<Graph, ::scafra::core::GraphError> {\n",
    );
    output.push_str("        let _plan = descriptor().plan()?;\n");

    for index in &model.order {
        let node = &model.nodes[*index];
        let arguments = node
            .dependencies
            .iter()
            .map(|dependency| {
                let dependency_index = model
                    .nodes
                    .iter()
                    .position(|candidate| candidate.output == *dependency)
                    .expect("validated graph dependency");
                format!("node_{dependency_index}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!("{}({arguments})", node.provider_expression);
        if !node.fallible && node.dependencies.is_empty() {
            output.push_str(&format!(
                "        let node_{index} = overrides.node_{index}.take().unwrap_or_else({});\n",
                node.provider_expression
            ));
            continue;
        }
        output.push_str(&format!(
            "        let node_{index} = match overrides.node_{index}.take() {{\n            Some(value) => value,\n            None => {{\n"
        ));
        if node.fallible {
            output.push_str(&format!(
                "                {call}.map_err(|error| ::scafra::core::GraphError::provider_failure({}, ::scafra::core::GraphPhase::Construction, {}, error))?\n",
                rust_string(&node.provider),
                rust_string(&node.source)
            ));
        } else {
            output.push_str(&format!("                {call}\n"));
        }
        output.push_str("            }\n        };\n");
    }

    output.push_str("        Ok(Graph {\n");
    for index in 0..model.nodes.len() {
        if model
            .edges
            .iter()
            .any(|edge| edge.dependency == model.nodes[index].output)
        {
            continue;
        }
        output.push_str(&format!("            node_{index},\n"));
    }
    output.push_str("        })\n    }\n}\n");
}

fn graph_kind_tokens(kind: GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::Bean => "::scafra::core::GraphNodeKind::Bean",
        GraphNodeKind::Service => "::scafra::core::GraphNodeKind::Service",
        GraphNodeKind::Component => "::scafra::core::GraphNodeKind::Component",
        GraphNodeKind::Repository => "::scafra::core::GraphNodeKind::Repository",
    }
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

pub(crate) fn render_node(node: &ModuleNode, indent: usize, output: &mut String) {
    let padding = "    ".repeat(indent);
    for (module_name, path) in &node.files {
        output.push_str(&format!(
            "{padding}#[path = {:?}]\n{padding}pub mod {module_name};\n",
            path.display().to_string()
        ));
    }
    for (module_name, child) in &node.children {
        output.push_str(&format!("{padding}pub mod {module_name} {{\n"));
        render_node(child, indent + 1, output);
        output.push_str(&format!("{padding}}}\n"));
    }
}

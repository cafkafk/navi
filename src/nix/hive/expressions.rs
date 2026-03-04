use super::SerializedNixExpression;
use crate::nix::expression::NixExpression;
use const_format::formatcp;

/// The version of the Hive schema we are compatible with.
pub const HIVE_SCHEMA: &str = "v0.5";

/// The snippet to be used for `nix eval --apply`.
pub const FLAKE_APPLY_SNIPPET: &str = formatcp!(
    r#"with builtins; hive: assert (hive.__schema == "{}" || throw ''
    The naviHive output (schema ${{hive.__schema}}) isn't compatible with this version of Navi.

    Hint: Use the same version of Navi as in the Flake input.
''); "#,
    HIVE_SCHEMA
);

pub fn deployment_config_single(node: &str) -> String {
    format!("hive.nodes.\"{}\".config.deployment or null", node)
}

pub fn deployment_config_selected(nodes_expr: &str) -> String {
    format!("hive.deploymentConfigSelected {}", nodes_expr)
}

pub fn eval_selected_drv_paths(nodes_expr: &str) -> String {
    format!("hive.evalSelectedDrvPaths {}", nodes_expr)
}

pub fn introspect(expression: &str) -> String {
    format!("hive.introspect ({})", expression)
}

pub fn introspect_json(expression: &str) -> String {
    format!("toJSON (hive.introspect ({}))", expression)
}

pub fn eval_selected(base_expression: &str, nodes_expr: &str) -> String {
    format!("{} hive.evalSelected {}", base_expression, nodes_expr,)
}

pub fn repl_flake(flake_uri: &str, base_expression: &str) -> String {
    let verifier = format!("{} hive", base_expression);
    format!(
        "with builtins; let rawHive = (getFlake \"{}\").outputs.naviHive; hive = ({}) rawHive; in hive.introspect (x: x)",
        flake_uri, verifier
    )
}

pub fn repl_legacy(base_expression: &str) -> String {
    format!("{} hive.introspect (x: x)", base_expression)
}

pub fn build_config_chunks(
    base: &str,
    flake_uri: Option<&str>,
    is_flake: bool,
    chunks: &[impl AsRef<[crate::nix::NodeName]>],
) -> String {
    let hive_source = if is_flake {
        let flake_uri = flake_uri.expect("Flake URI required for flake hives");
        // We use `builtins.getFlake` to load the flake in this standalone expression.
        let source = format!("(builtins.getFlake \"{}\").outputs.naviHive", flake_uri);
        // For flakes, base is the validator snippet function `hive: assert ...;`
        // We append "hive" to apply the function to the hive.
        format!("({}) ({})", format!("{} hive", base), source)
    } else {
        // Legacy path: base defines `hive` variable in scope.
        base.to_string()
    };

    let mut expr = format!(
        "with builtins; let hive = {}; f = hive.exportSelectedConfig; in {{",
        hive_source
    );

    for (i, chunk) in chunks.iter().enumerate() {
        let nodes_expr = SerializedNixExpression::new(chunk.as_ref()).expression();
        expr.push_str(&format!("\"chunk-{}\" = f {}; ", i, nodes_expr));
    }
    expr.push('}');
    expr
}

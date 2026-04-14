#![allow(clippy::all)]
use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use substring::Substring;
use toml::Value;

#[derive(Serialize, Deserialize, Debug)]
pub struct Cargo {
    package: Package,
    dependencies: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Package {
    name: String,
}

/// Returns module names in topological order: dependencies before dependents.
/// Within the same depth level, names are sorted alphabetically for stability.
fn topological_sort(deps: &HashMap<String, Vec<String>>) -> Vec<String> {
    // Build adjacency list (dep → dependents) and track in-degrees.
    let mut graph: HashMap<&str, Vec<&str>> = deps.keys().map(|k| (k.as_str(), vec![])).collect();
    let mut in_degree: HashMap<&str, usize> = deps.keys().map(|k| (k.as_str(), 0)).collect();

    for (module, module_deps) in deps {
        for dep in module_deps {
            if let Some(neighbors) = graph.get_mut(dep.as_str()) {
                neighbors.push(module.as_str());
                *in_degree.entry(module.as_str()).or_default() += 1;
            }
        }
    }

    // Seed the queue with nodes that have no dependencies.
    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(k, _)| *k)
        .collect();
    queue.sort_unstable();

    let mut result = Vec::with_capacity(deps.len());
    while !queue.is_empty() {
        let node = queue.remove(0);
        result.push(node.to_string());

        if let Some(neighbors) = graph.get(node) {
            let mut next: Vec<&str> = neighbors
                .iter()
                .filter(|&&nb| {
                    let deg = in_degree.get_mut(nb).expect("nb not found");
                    *deg -= 1;
                    *deg == 0
                })
                .copied()
                .collect();
            next.sort_unstable();
            queue.extend(next);
        }
    }
    result
}

#[allow(clippy::unwrap_used)]
fn main() {
    let prefix = "tibba-";
    let mut deps = HashMap::new();
    for entry in glob(&format!("{prefix}*/Cargo.toml")).unwrap() {
        let e = entry.unwrap();
        let data = fs::read_to_string(e).unwrap();
        let c = toml::from_str::<Cargo>(&data).unwrap();
        let package_name = c.package.name;
        let mut modules: Vec<String> = vec![];
        for name in c.dependencies.keys() {
            if !name.starts_with(prefix) {
                continue;
            }
            modules.push(name.substring(prefix.len(), name.len()).to_string());
        }

        deps.insert(
            package_name
                .substring(prefix.len(), package_name.len())
                .to_string(),
            modules,
        );
    }
    println!("deps: {deps:?}");
    let mut arr = vec![];
    for name in topological_sort(&deps).iter() {
        let mut modules = deps.get(name).unwrap().clone();
        if modules.is_empty() {
            continue;
        }
        modules.sort();
        for module in modules.iter() {
            arr.push(format!("    {name} --> {module}"));
        }
        arr.push("".to_string());
    }

    let mermaid = format!(
        r#"```mermaid
graph TD
{}```"#,
        arr.join("\n")
    );
    let re = Regex::new(r#"```mermaid[\s\S]*?```"#).unwrap();
    let file = "docs/modules.md";
    let mut content = fs::read_to_string(file).unwrap();
    content = re.replace(&content, &mermaid).to_string();
    fs::write(file, content).unwrap();
}

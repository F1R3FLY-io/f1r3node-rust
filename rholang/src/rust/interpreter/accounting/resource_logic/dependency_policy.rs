use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

type Graph = BTreeMap<String, Vec<String>>;

fn closure<'a>(
    graph: &'a Graph,
    roots: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>, String> {
    let mut complete = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut work: Vec<_> = roots.into_iter().map(|id| (id, false)).collect();
    while let Some((id, leaving)) = work.pop() {
        if leaving {
            active.remove(id);
            complete.insert(id);
            continue;
        }
        if active.contains(id) {
            return Err(format!("dependency cycle at {id}"));
        }
        if complete.contains(id) {
            continue;
        }
        let children = graph
            .get(id)
            .ok_or_else(|| format!("missing resolved package {id}"))?;
        active.insert(id);
        work.push((id, true));
        work.extend(children.iter().map(|child| (child.as_str(), false)));
    }
    Ok(complete)
}

fn validate_boundary(
    graph: &Graph,
    application: &str,
    bridge: &str,
    core: &BTreeSet<String>,
    mettail: &BTreeSet<String>,
) -> Result<(), String> {
    closure(graph, graph.keys().map(String::as_str))?;
    if !mettail.contains(bridge) || core.contains(application) || !core.is_disjoint(mettail) {
        return Err("invalid component partition".into());
    }
    if !graph
        .get(application)
        .is_some_and(|deps| deps.iter().any(|id| id == bridge))
    {
        return Err("composition feature did not resolve the application bridge edge".into());
    }
    for id in core {
        let reachable = closure(graph, [id.as_str()])?;
        if reachable.contains(application)
            || reachable.iter().any(|target| mettail.contains(*target))
        {
            return Err(format!(
                "core package {id} reaches the application or MeTTaIL"
            ));
        }
    }
    for id in mettail {
        if closure(graph, [id.as_str()])?.contains(application) {
            return Err(format!("MeTTaIL package {id} reaches the application"));
        }
    }
    for (source, targets) in graph {
        for target in targets {
            if mettail.contains(target)
                && !mettail.contains(source)
                && !(source == application && target == bridge)
            {
                return Err(format!(
                    "unapproved MeTTaIL entry edge {source} -> {target}"
                ));
            }
        }
    }
    Ok(())
}

fn resolved_graph(metadata: &Value) -> Graph {
    metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolved Cargo nodes")
        .iter()
        .map(|node| {
            let id = node["id"].as_str().expect("resolved package ID").to_owned();
            let dependencies = node["deps"]
                .as_array()
                .expect("resolved dependency edges")
                .iter()
                .filter(|dependency| {
                    dependency["dep_kinds"]
                        .as_array()
                        .expect("dependency kinds")
                        .iter()
                        .any(|kind| {
                            kind["kind"].is_null() || kind["kind"].as_str() == Some("build")
                        })
                })
                .map(|dependency| {
                    dependency["pkg"]
                        .as_str()
                        .expect("target package ID")
                        .to_owned()
                })
                .collect();
            (id, dependencies)
        })
        .collect()
}

fn validate_bridge_manifest(approved_root: &Path, actual_manifest: &Path) -> Result<(), String> {
    match actual_manifest == approved_root.join("rholang-runtime/Cargo.toml") {
        true => Ok(()),
        false => Err("bridge manifest is outside the independently approved MeTTaIL root".into()),
    }
}

#[test]
fn resolved_composition_preserves_core_independence() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--all-features",
        ])
        .current_dir(workspace)
        .output()
        .expect("run Cargo metadata");
    assert!(
        output.status.success(),
        "Cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("Cargo metadata JSON");
    let packages = metadata["packages"].as_array().expect("Cargo packages");
    let application_manifest = workspace
        .join("node/Cargo.toml")
        .canonicalize()
        .expect("application manifest");
    let package_at = |manifest: &Path| {
        let matches: Vec<_> = packages
            .iter()
            .filter(|package| {
                Path::new(package["manifest_path"].as_str().expect("package manifest"))
                    .canonicalize()
                    .expect("canonical package manifest")
                    == manifest
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "one resolved package per designated manifest"
        );
        matches[0]
    };
    let application = package_at(&application_manifest);
    let declared_bridge: Vec<_> = application["dependencies"]
        .as_array()
        .expect("application dependencies")
        .iter()
        .filter(|dependency| dependency["name"] == "rholang-runtime")
        .collect();
    assert_eq!(
        declared_bridge.len(),
        1,
        "one explicitly declared MeTTaIL runtime"
    );
    assert_eq!(declared_bridge[0]["optional"], true);
    assert_eq!(declared_bridge[0]["uses_default_features"], false);
    let mettail_root = workspace
        .parent()
        .expect("sibling workspace directory")
        .join("mettail-module-dev/mettail-rust")
        .canonicalize()
        .expect("approved MeTTaIL root");
    let bridge_manifest = Path::new(
        declared_bridge[0]["path"]
            .as_str()
            .expect("pinned bridge path"),
    )
    .join("Cargo.toml")
    .canonicalize()
    .expect("canonical bridge manifest");
    validate_bridge_manifest(&mettail_root, &bridge_manifest).expect("approved bridge identity");
    let bridge = package_at(&bridge_manifest);
    let application_id = application["id"].as_str().expect("application ID");
    let bridge_id = bridge["id"].as_str().expect("bridge ID");
    let mettail: BTreeSet<_> = packages
        .iter()
        .filter(|package| {
            Path::new(package["manifest_path"].as_str().expect("package manifest"))
                .canonicalize()
                .expect("canonical manifest")
                .starts_with(&mettail_root)
        })
        .map(|package| package["id"].as_str().expect("package ID").to_owned())
        .collect();
    let core: BTreeSet<_> = metadata["workspace_members"]
        .as_array()
        .expect("workspace members")
        .iter()
        .map(|id| id.as_str().expect("workspace package ID"))
        .filter(|id| *id != application_id)
        .map(str::to_owned)
        .collect();
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == application_id)
        .expect("resolved application");
    assert!(node["features"]
        .as_array()
        .expect("selected features")
        .iter()
        .any(|feature| feature == "mettail-frontend"));
    validate_boundary(
        &resolved_graph(&metadata),
        application_id,
        bridge_id,
        &core,
        &mettail,
    )
    .expect("acyclic upper composition with independent node core");
}

fn fixture(edges: &[(&str, &[&str])]) -> Graph {
    edges
        .iter()
        .map(|(source, targets)| {
            (
                source.to_string(),
                targets.iter().map(|target| target.to_string()).collect(),
            )
        })
        .collect()
}

fn check_fixture(graph: &Graph) -> Result<(), String> {
    validate_boundary(
        graph,
        "application",
        "bridge",
        &BTreeSet::from(["core".into()]),
        &BTreeSet::from(["bridge".into()]),
    )
}

#[test]
fn application_bridge_core_composition_is_allowed() {
    check_fixture(&fixture(&[
        ("application", &["bridge", "core"]),
        ("bridge", &["core"]),
        ("core", &[]),
    ]))
    .expect("allowed composition");
}

#[test]
fn direct_and_transitive_core_back_edges_are_rejected() {
    for dependencies in [&["bridge"][..], &["helper"][..]] {
        let graph = fixture(&[
            ("application", &["bridge"]),
            ("bridge", &[]),
            ("core", dependencies),
            ("helper", &["bridge"]),
        ]);
        assert!(check_fixture(&graph)
            .expect_err("core back edge")
            .contains("core package"));
    }
}

#[test]
fn reverse_application_edges_and_cycles_are_rejected() {
    for (source, target) in [
        ("bridge", "application"),
        ("core", "application"),
        ("core", "core"),
    ] {
        let mut graph = fixture(&[
            ("application", &["bridge", "core"]),
            ("bridge", &[]),
            ("core", &[]),
        ]);
        graph
            .get_mut(source)
            .expect("fixture source")
            .push(target.into());
        assert!(check_fixture(&graph)
            .expect_err("cycle")
            .contains("dependency cycle"));
    }
}

#[test]
fn renamed_build_dependency_keeps_its_resolved_identity() {
    let metadata = serde_json::json!({"resolve": {"nodes": [
        {"id":"core", "deps":[{"name":"innocent_alias", "pkg":"bridge", "dep_kinds":[{"kind":"build"}]}]},
        {"id":"bridge", "deps":[]},
        {"id":"application", "deps":[{"name":"compiler_alias", "pkg":"bridge", "dep_kinds":[{"kind":null}]}]}
    ]}});
    assert!(check_fixture(&resolved_graph(&metadata))
        .expect_err("aliased build back edge")
        .contains("core package"));
}

#[test]
fn incomplete_and_unselected_composition_are_rejected() {
    let mut graph = fixture(&[
        ("application", &["bridge"]),
        ("bridge", &[]),
        ("core", &["missing"]),
    ]);
    assert!(check_fixture(&graph)
        .expect_err("missing node")
        .contains("missing resolved package"));
    graph.get_mut("core").expect("core").clear();
    graph.get_mut("application").expect("application").clear();
    assert!(check_fixture(&graph)
        .expect_err("disabled composition")
        .contains("composition feature"));
}

#[test]
fn decoy_checkout_cannot_redefine_the_protected_component() {
    let approved = Path::new("approved-mettail");
    validate_bridge_manifest(approved, &approved.join("rholang-runtime/Cargo.toml"))
        .expect("approved manifest");
    validate_bridge_manifest(
        approved,
        Path::new("decoy-mettail/rholang-runtime/Cargo.toml"),
    )
    .expect_err("another checkout cannot select the protected component");
    let graph = fixture(&[
        ("application", &["decoy"]),
        ("decoy", &[]),
        ("core", &["bridge"]),
        ("bridge", &[]),
    ]);
    assert!(validate_boundary(
        &graph,
        "application",
        "decoy",
        &BTreeSet::from(["core".into()]),
        &BTreeSet::from(["bridge".into()])
    )
    .is_err());
}

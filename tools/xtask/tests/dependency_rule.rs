//! The Dependency Rule holds for the actual workspace.

use cargo_metadata::MetadataCommand;
use xtask::{
    ADAPTERS, APP, CrateNode, ENTITIES, USE_CASES, XTASK, check_dependency_rule,
    check_inner_rings_are_pure, workspace_from_metadata,
};

fn workspace() -> Vec<CrateNode> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("cargo metadata should resolve the workspace");
    workspace_from_metadata(&metadata)
}

fn node<'a>(crates: &'a [CrateNode], name: &str) -> &'a CrateNode {
    crates
        .iter()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not a workspace member"))
}

#[test]
fn dependency_rule_holds() {
    if let Err(violations) = check_dependency_rule(&workspace()) {
        panic!("dependency rule violated: {violations:#?}");
    }
}

#[test]
fn the_entities_are_a_leaf() {
    let crates = workspace();
    assert!(node(&crates, ENTITIES).deps.is_empty());
}

#[test]
fn the_use_cases_know_the_entities_and_nothing_else_of_ours() {
    let crates = workspace();
    assert_eq!(node(&crates, USE_CASES).deps, vec![ENTITIES.to_string()]);
}

#[test]
fn the_adapters_know_the_kernel_and_never_the_app() {
    let crates = workspace();
    let adapters = node(&crates, ADAPTERS);
    for expected in [ENTITIES, USE_CASES] {
        assert!(
            adapters.deps.iter().any(|d| d == expected),
            "{:?}",
            adapters.deps
        );
    }
    assert!(!adapters.deps.iter().any(|d| d == APP));
}

#[test]
fn the_app_and_xtask_are_sinks() {
    let crates = workspace();
    for sink in [APP, XTASK] {
        for other in &crates {
            assert!(
                !other.deps.iter().any(|d| d == sink),
                "`{}` depends on the sink `{sink}`",
                other.name
            );
        }
    }
    assert!(node(&crates, XTASK).deps.is_empty());
}

#[test]
fn the_inner_rings_do_no_io() {
    if let Err(violations) = check_inner_rings_are_pure(&workspace()) {
        panic!("an inner ring is not pure: {violations:#?}");
    }
}

//! This module applies the rules from `super::dsl` to a `SyntaxNode`, to
//! get a `FmtDiff`.
mod fmt_model;
mod indentation;
mod spacing;
mod fixes;

use rnix::{SmolStr, SyntaxKind, SyntaxNode, TextRange};
use std::collections::HashMap;

use crate::{
    dsl::{IndentDsl, RuleName, SpacingDsl},
    engine::fmt_model::{BlockPosition, FmtModel, SpaceBlock, SpaceBlockOrToken},
    pattern::PatternSet,
    tree_utils::walk_non_whitespace,
    AtomEdit, FmtDiff,
};

/// The main entry point for formatting
pub(crate) fn format(
    spacing_dsl: &SpacingDsl,
    indent_dsl: &IndentDsl,
    root: &SyntaxNode,
) -> FmtDiff {
    let mut model = FmtModel::new(root.clone());

    // First, adjust spacing rules between the nodes.
    // This can force some newlines.
    let spacing_rule_set = PatternSet::new(spacing_dsl.rules.iter());
    for element in walk_non_whitespace(root) {
        for rule in spacing_rule_set.matching(element.clone()) {
            rule.apply(&element, &mut model)
        }
    }

    // Next, for each node which starts the newline, adjust the indent.
    let anchor_set = PatternSet::new(indent_dsl.anchors.iter());
    for element in walk_non_whitespace(root) {
        let block = model.block_for(&element, BlockPosition::Before);
        if !block.has_newline() {
            // No need to indent an element if it doesn't start a line
            continue;
        }

        // In cases like
        //
        // ```nix
        //   param:
        //     body
        // ```
        //
        // we only indent top-level node (lambda), and not it's first child (parameter)
        if element.parent().map(|it| it.text_range().start()) == Some(element.text_range().start())
        {
            continue;
        }

        let mut matching = indent_dsl.rules.iter().filter(|it| it.matches(&element));
        if let Some(rule) = matching.next() {
            rule.apply(&element, &mut model, &anchor_set);
            assert!(matching.next().is_none(), "more that one indent rule matched");
        } else {
            indentation::default_indent(&element, &mut model, &anchor_set)
        }
    }

    // Finally, do custom touch-ups like re-indenting of string literals and
    // replacing URLs with string literals.
    for element in walk_non_whitespace(root) {
        fixes::fix(element, &mut model, &anchor_set);
    }

    let mut my_model = FmtModel::new(root.clone());
    for element in walk_non_whitespace(root) {
        element.as_node().and_then(|x| get_node_attr_set(&x)).map(|node| {
            let set = attr_set_binds_to_hashmap(&node);
            println!("attr set: {:?}", set);

            // example edit
            let range = node.clone().text_range();
            let delete = TextRange::offset_len(range.start(), range.len());
            let insert = "replacement".into();
            my_model.raw_edit(AtomEdit { delete, insert });
        });
    }

    dbg!(my_model.into_diff().edits);

    model.into_diff()
}

fn attr_set_binds_to_hashmap(node: &SyntaxNode) -> HashMap<String, String> {
    let mut hm = HashMap::new();

    let binds = node.children();
    for bind in binds {
        if let Some(pair) = bind_to_option_pair(&bind) {
            hm.insert(pair.key, pair.value);
        }
    }

    hm
}

#[derive(Debug)]
struct Pair {
    key: String,
    value: String,
}

fn bind_to_option_pair(node: &SyntaxNode) -> Option<Pair> {
    let mut children = node.children();
    let key = children.next().and_then(|x| get_key_ident_string(&x))?;
    let value = children.next().and_then(|x| get_string_string(&x))?;

    Some(Pair { key, value })
}

fn get_node_attr_set(node: &SyntaxNode) -> Option<&SyntaxNode> {
    match node.kind() {
        SyntaxKind::NODE_ATTR_SET => Some(node),
        _ => None,
    }
}

fn get_node_key(node: &SyntaxNode) -> Option<&SyntaxNode> {
    match node.kind() {
        SyntaxKind::NODE_KEY => Some(node),
        _ => None,
    }
}

fn get_node_string(node: &SyntaxNode) -> Option<&SyntaxNode> {
    match node.kind() {
        SyntaxKind::NODE_STRING => Some(node),
        _ => None,
    }
}

fn node_to_string(node: &SyntaxNode) -> String {
    node.text().to_string()
}

fn get_key_ident_string(node: &SyntaxNode) -> Option<String> {
    get_node_key(node).map(node_to_string)
}

fn get_string_string(node: &SyntaxNode) -> Option<String> {
    get_node_string(node).map(node_to_string)
}

impl FmtDiff {
    fn replace(&mut self, range: TextRange, text: SmolStr, reason: Option<RuleName>) {
        self.edits.push((AtomEdit { delete: range, insert: text }, reason))
    }
}

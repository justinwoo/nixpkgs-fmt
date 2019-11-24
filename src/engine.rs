//! This module applies the rules from `super::dsl` to a `SyntaxNode`, to
//! get a `FmtDiff`.
mod fmt_model;
mod indentation;
mod spacing;
mod fixes;

use rnix::{NodeOrToken, SmolStr, SyntaxKind, SyntaxNode, TextRange};
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
        match element {
            NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::NODE_ATTR_SET => {
                    attr_set_binds_to_hashmap(&node);

                    let range = node.clone().text_range();
                    let delete = TextRange::offset_len(range.start(), range.len());
                    let insert = "replacement".into();
                    my_model.raw_edit(AtomEdit { delete, insert });
                    // model.raw_edit(AtomEdit { delete, insert});
                    ()
                }
                _ => (),
            },
            _ => (),
        }
    }

    dbg!(my_model.into_diff().edits);

    model.into_diff()
}

fn attr_set_binds_to_hashmap(node: &SyntaxNode) -> HashMap<String, String> {
    let mut hm = HashMap::new();

    let binds = node.children();

    for bind in binds {
        for inner in bind.children() {
            bind_to_option_pair(&inner);
        }
    }

    hm
}

struct Pair {
    key: String,
    value: String,
}

fn bind_to_option_pair(node: &SyntaxNode) -> Option<Pair> {
    let mut children = node.children();

    let identifier = children.nth(0).and_then(|x| get_ident_string(&x));

    let value = children.nth(1).and_then(|x| Some(x));

    // if child 0 is identifier, extract identifier string from it
    // if child 1 is string, extract string value from it

    // if (some conds and shit) {
    //     Just(Pair { key, value });
    // } else {
    // None
    // }
    None
}

fn get_ident_string(node: &SyntaxNode) -> Option<String> {
    dbg!(node);
    match node.kind() {
        SyntaxKind::NODE_IDENT => node.children().nth(0).and_then(|inner| match inner.kind() {
            SyntaxKind::NODE_STRING => {
                println!("what do i do with inner: {:?}", inner);
                None
            }
            _ => None,
        }),
        _ => None,
    }
}

impl FmtDiff {
    fn replace(&mut self, range: TextRange, text: SmolStr, reason: Option<RuleName>) {
        self.edits.push((AtomEdit { delete: range, insert: text }, reason))
    }
}

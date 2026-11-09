//! The rules' suffixes in a trie, written backwards, so the rules that apply
//! to a word are found by walking back from its last character instead of
//! testing every rule.
//!
//! For 食べなかった the walk goes た → っ → か → な and stops at べ, which no
//! suffix continues with. Each node passed holds the rules whose whole suffix
//! it spells: た→る, then the three った, then かった→い, then なかった→る.

use std::sync::OnceLock;

use crate::rules::{Rule, RULES};

struct Node {
    /// Next character back → child node. A handful at most, so a list.
    children: Vec<(char, u32)>,
    /// The rules whose suffix ends here, as indices into the rule list.
    rules: Vec<u16>,
}

pub(crate) struct SuffixTrie {
    /// The root is node 0.
    nodes: Vec<Node>,
}

impl SuffixTrie {
    fn build(rules: &[Rule]) -> Self {
        let mut trie = SuffixTrie {
            nodes: vec![Node {
                children: vec![],
                rules: vec![],
            }],
        };
        for (i, rule) in rules.iter().enumerate() {
            let mut at = 0usize;
            for c in rule.suffix_in.chars().rev() {
                at = match trie.child(at, c) {
                    Some(next) => next,
                    None => {
                        let next = trie.nodes.len();
                        trie.nodes.push(Node {
                            children: vec![],
                            rules: vec![],
                        });
                        if let Some(node) = trie.nodes.get_mut(at) {
                            node.children.push((c, next as u32));
                        }
                        next
                    }
                };
            }
            if let Some(node) = trie.nodes.get_mut(at) {
                node.rules.push(i as u16);
            }
        }
        trie
    }

    fn child(&self, at: usize, c: char) -> Option<usize> {
        self.nodes
            .get(at)?
            .children
            .iter()
            .find(|&&(k, _)| k == c)
            .map(|&(_, next)| next as usize)
    }

    /// The rules whose suffix `word` ends with, in rule-list order, into
    /// `out` (cleared first).
    pub(crate) fn matches(&self, word: &str, out: &mut Vec<u16>) {
        out.clear();
        let mut at = 0usize;
        for c in word.chars().rev() {
            let Some(next) = self.child(at, c) else {
                break;
            };
            at = next;
            if let Some(node) = self.nodes.get(at) {
                out.extend_from_slice(&node.rules);
            }
        }
        // The walk finds short suffixes first; the rule list's own order is
        // what decides the order of the candidates.
        out.sort_unstable();
    }
}

/// The trie over [`RULES`], built on first use.
pub(crate) fn rules_trie() -> &'static SuffixTrie {
    static TRIE: OnceLock<SuffixTrie> = OnceLock::new();
    TRIE.get_or_init(|| SuffixTrie::build(RULES))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trie must find exactly the rules a plain scan finds, in the same
    /// order, for every suffix in the list and for words around them.
    #[test]
    fn matches_the_same_rules_as_a_scan() {
        let trie = rules_trie();
        let mut words: Vec<String> = vec![
            "食べさせられなかった".into(),
            "書いていなかった".into(),
            "テスト".into(),
            "hello".into(),
            String::new(),
        ];
        for r in RULES {
            words.push(r.suffix_in.to_string());
            words.push(format!("食べ{}", r.suffix_in));
            // One character off the start: must not match this rule.
            words.push(r.suffix_in.chars().skip(1).collect());
        }
        let mut found = Vec::new();
        for w in &words {
            trie.matches(w, &mut found);
            let scanned: Vec<u16> = RULES
                .iter()
                .enumerate()
                .filter(|(_, r)| w.ends_with(r.suffix_in))
                .map(|(i, _)| i as u16)
                .collect();
            assert_eq!(found, scanned, "rules for {w:?}");
        }
    }
}

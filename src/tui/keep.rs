//! Kept groups: the groups of each grouping the user keeps with `+`, and the switch that shows
//! only them (`f+`). A group is known by its labels, never an id, so it survives a restart.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

#[derive(Default, Clone, PartialEq, Debug)]
pub struct Keep {
    /// Each grouping (`session`, `host`, `field:<id>`) with kept groups, and their labels.
    pub groups: BTreeMap<String, BTreeSet<String>>,
    pub only: bool,
}

impl Keep {
    pub fn has(&self, grouping: &str, label: &str) -> bool {
        self.groups
            .get(grouping)
            .is_some_and(|labels| labels.contains(label))
    }

    pub fn add(&mut self, grouping: &str, label: &str) {
        self.groups
            .entry(grouping.to_owned())
            .or_default()
            .insert(label.to_owned());
    }

    /// Keeps the group, or drops it when it was kept; whether it is kept now.
    pub fn toggle(&mut self, grouping: &str, label: &str) -> bool {
        if !self.has(grouping, label) {
            self.add(grouping, label);
            return true;
        }
        if let Some(labels) = self.groups.get_mut(grouping) {
            labels.remove(label);
            if labels.is_empty() {
                self.groups.remove(grouping);
            }
        }
        false
    }

    pub fn count(&self) -> usize {
        self.groups.values().map(BTreeSet::len).sum()
    }

    /// Drops every kept group and the switch; how many groups went.
    pub fn clear(&mut self) -> usize {
        let dropped = self.count();
        *self = Keep::default();
        dropped
    }

    /// board.json's `keep` and `only_kept`; a value of the wrong shape is left out.
    pub fn from_json(object: &Map<String, Value>) -> Keep {
        let mut keep = Keep {
            only: object.get("only_kept").and_then(Value::as_bool) == Some(true),
            ..Keep::default()
        };
        let groups = object.get("keep").and_then(Value::as_object);
        for (grouping, labels) in groups.into_iter().flatten() {
            for label in labels.as_array().into_iter().flatten() {
                if let Some(label) = label.as_str().filter(|label| !label.is_empty()) {
                    keep.add(grouping, label);
                }
            }
        }
        keep
    }

    /// The value of board.json's `keep` key; None when nothing is kept, so the key goes.
    pub fn groups_json(&self) -> Option<Value> {
        (!self.groups.is_empty()).then(|| serde_json::json!(self.groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(text: &str) -> Map<String, Value> {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_group_is_kept_and_dropped_by_the_same_call_and_an_empty_grouping_goes() {
        let mut keep = Keep::default();
        assert!(keep.toggle("session", "api@studio"));
        assert!(keep.toggle("session", "web@studio"));
        assert_eq!(keep.count(), 2);
        assert!(!keep.toggle("session", "api@studio"));
        assert!(!keep.toggle("session", "web@studio"));
        assert!(keep.groups.is_empty(), "no empty set is left behind");
    }

    #[test]
    fn the_file_reads_back_what_it_wrote_and_ignores_what_has_the_wrong_shape() {
        let mut keep = Keep::default();
        keep.add("session", "api@studio");
        keep.add("host", "mini");
        keep.only = true;
        let written = serde_json::json!({"keep": keep.groups_json(), "only_kept": true});
        assert_eq!(Keep::from_json(written.as_object().unwrap()), keep);
        let odd =
            object(r#"{"keep":{"session":["a@b",3,""],"host":"mini","x":{}},"only_kept":"yes"}"#);
        let read = Keep::from_json(&odd);
        assert_eq!(read.count(), 1);
        assert!(read.has("session", "a@b") && !read.only);
        assert_eq!(Keep::from_json(&object(r#"{"keep":[1]}"#)), Keep::default());
    }

    #[test]
    fn clear_says_how_many_groups_went_and_turns_the_switch_off() {
        let mut keep = Keep::default();
        keep.add("session", "a@b");
        keep.add("host", "mini");
        keep.only = true;
        assert_eq!(keep.clear(), 2);
        assert_eq!(keep, Keep::default());
    }
}

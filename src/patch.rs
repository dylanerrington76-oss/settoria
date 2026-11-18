//! Patches between two JSON values: what a client that asked for them is sent instead of the whole view.
//!
//! `diff(prev, next, &mut ops)` lists what to do to `prev` to make it `next`. An op is a JSON array:
//!
//! * `[path, value]` sets what is at `path` to `value`;
//! * `[path]` removes the key at `path` (an object's key; nothing in a game view is ever removed, but a diff
//!   has to be able to say it).
//!
//! A `path` is an array of steps from the top: a string is a key of an object, a number an index into an array.
//! The client applies the ops in order (`applyPatch` in web/src/patch.ts), and nothing else about the two
//! values has to be known to it, which is the point: this module knows nothing about the game.
//!
//! The diff goes down into objects, and into the arrays that are worth it. The two big arrays of a view,
//! `buildings` and `roads`, are mostly `null`, and a turn changes a few entries of them, so they are patched
//! entry by entry. A short array of numbers (`hand`, `bank`) is cheaper to send whole than as ops, and so is
//! an array of which most entries changed. Arrays of different lengths are always sent whole.

use serde_json::Value;

/// An array this short, or shorter, is sent whole unless its entries are objects (as a seat's `PlayerView` is: one
/// of its fields changing should not send the others).
const SMALL_ARRAY: usize = 12;

/// Append to `ops` what turns `prev` into `next`. Appends nothing if they are equal.
pub fn diff(prev: &Value, next: &Value, ops: &mut Vec<Value>) {
    walk(&mut Vec::new(), prev, next, ops);
}

fn op(path: &[Value], value: Option<&Value>) -> Value {
    let mut parts = vec![Value::Array(path.to_vec())];
    if let Some(v) = value {
        parts.push(v.clone());
    }
    Value::Array(parts)
}

fn walk(path: &mut Vec<Value>, prev: &Value, next: &Value, ops: &mut Vec<Value>) {
    if prev == next {
        return;
    }
    match (prev, next) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, new) in b {
                path.push(Value::from(key.as_str()));
                match a.get(key) {
                    Some(old) => walk(path, old, new, ops),
                    None => ops.push(op(path, Some(new))),
                }
                path.pop();
            }
            for key in a.keys() {
                if !b.contains_key(key) {
                    path.push(Value::from(key.as_str()));
                    ops.push(op(path, None));
                    path.pop();
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            let changed = a.iter().zip(b).filter(|(x, y)| x != y).count();
            let by_entry = if b.len() <= SMALL_ARRAY {
                b.iter().any(Value::is_object)
            } else {
                // Patching one entry costs about as much as a quarter of the array does to send, so past
                // that sending the lot is no dearer, and it is simpler for the client.
                changed * 4 <= b.len()
            };
            if by_entry {
                for (i, (old, new)) in a.iter().zip(b).enumerate() {
                    path.push(Value::from(i));
                    walk(path, old, new, ops);
                    path.pop();
                }
            } else {
                ops.push(op(path, Some(next)));
            }
        }
        _ => ops.push(op(path, Some(next))),
    }
}

/// Apply ops to a document, the way a client does. Only the tests need this; they use it to show that a patch
/// really turns the old value into the new one.
#[cfg(test)]
pub fn apply(doc: &mut Value, ops: &[Value]) {
    for o in ops {
        let o = o.as_array().expect("an op is an array");
        let path = o[0].as_array().expect("an op starts with its path");
        let (last, parents) = path.split_last().expect("a path is never empty");
        let mut node = &mut *doc;
        for step in parents {
            node = match step {
                Value::String(key) => node.get_mut(key.as_str()),
                n => node.get_mut(n.as_u64().expect("a number step") as usize),
            }
            .expect("the path leads somewhere");
        }
        match last {
            Value::String(key) => {
                let object = node.as_object_mut().expect("a key step is into an object");
                match o.get(1) {
                    Some(value) => {
                        object.insert(key.clone(), value.clone());
                    }
                    None => {
                        object.remove(key.as_str());
                    }
                }
            }
            n => {
                let at = n.as_u64().expect("a number step") as usize;
                node.as_array_mut().expect("an index step is into an array")[at] = o[1].clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The patch from `a` to `b`, checked to do what it says.
    fn patch(a: &Value, b: &Value) -> Vec<Value> {
        let mut ops = vec![];
        diff(a, b, &mut ops);
        let mut doc = a.clone();
        apply(&mut doc, &ops);
        assert_eq!(&doc, b, "ops {ops:?} turn {a} into {doc}, not {b}");
        ops
    }

    #[test]
    fn equal_values_need_no_ops() {
        let v = json!({ "a": [1, 2, 3], "b": { "c": null }, "d": "x" });
        assert!(patch(&v, &v.clone()).is_empty());
    }

    #[test]
    fn only_the_fields_that_changed_are_sent() {
        let a = json!({ "phase": "roll", "current": 0, "hand": [1, 0, 2, 0, 1], "dice": null });
        let b = json!({ "phase": "main", "current": 0, "hand": [1, 0, 2, 0, 1], "dice": [3, 4] });
        let ops = patch(&a, &b);
        assert_eq!(ops, vec![json!([["dice"], [3, 4]]), json!([["phase"], "main"])]);
    }

    #[test]
    fn a_short_array_of_numbers_is_sent_whole() {
        let ops = patch(&json!({ "hand": [1, 0, 2, 0, 1] }), &json!({ "hand": [1, 0, 3, 0, 1] }));
        assert_eq!(ops, vec![json!([["hand"], [1, 0, 3, 0, 1]])]);
    }

    #[test]
    fn an_array_of_objects_is_patched_entry_by_entry_and_field_by_field() {
        let a = json!({ "players": [{ "cards": 3, "vp": 2, "name": "Ada" }, { "cards": 5, "vp": 2, "name": "Bo" }] });
        let b = json!({ "players": [{ "cards": 3, "vp": 2, "name": "Ada" }, { "cards": 4, "vp": 3, "name": "Bo" }] });
        let ops = patch(&a, &b);
        assert_eq!(ops, vec![json!([["players", 1, "cards"], 4]), json!([["players", 1, "vp"], 3])]);
    }

    #[test]
    fn a_long_mostly_unchanged_array_costs_one_op_per_changed_entry() {
        let mut roads = vec![Value::Null; 100];
        let a = json!({ "roads": roads.clone() });
        roads[17] = json!(1);
        roads[60] = json!(0);
        let ops = patch(&a, &json!({ "roads": roads }));
        assert_eq!(ops, vec![json!([["roads", 17], 1]), json!([["roads", 60], 0])]);
    }

    #[test]
    fn a_building_going_from_nothing_to_something_to_a_city_is_patched_in_place() {
        let a = json!({ "buildings": [null, null, null, null, null, null, null, null, null, null, null, null, null, null] });
        let mut b = a.clone();
        b["buildings"][5] = json!({ "owner": 1, "city": false });
        assert_eq!(patch(&a, &b), vec![json!([["buildings", 5], { "owner": 1, "city": false }])]);
        let mut c = b.clone();
        c["buildings"][5]["city"] = json!(true);
        assert_eq!(patch(&b, &c), vec![json!([["buildings", 5, "city"], true])]);
    }

    #[test]
    fn a_long_array_that_mostly_changed_is_sent_whole() {
        let a = json!({ "legal": (0..40).collect::<Vec<_>>() });
        let b = json!({ "legal": (0..40).map(|i| i + 1).collect::<Vec<_>>() });
        assert_eq!(patch(&a, &b).len(), 1);
    }

    #[test]
    fn arrays_of_different_lengths_are_sent_whole() {
        let a = json!({ "trades": [{ "id": 1 }] });
        let b = json!({ "trades": [{ "id": 1 }, { "id": 2 }] });
        assert_eq!(patch(&a, &b), vec![json!([["trades"], [{ "id": 1 }, { "id": 2 }]])]);
        assert_eq!(patch(&b, &a), vec![json!([["trades"], [{ "id": 1 }]])]);
    }

    #[test]
    fn a_value_that_changes_type_is_replaced() {
        let a = json!({ "timer": null, "volcano": { "hex": 1 } });
        let b = json!({ "timer": { "kind": "turn" }, "volcano": null });
        assert_eq!(patch(&a, &b), vec![json!([["timer"], { "kind": "turn" }]), json!([["volcano"], null])]);
    }

    #[test]
    fn keys_that_appear_and_disappear_are_handled() {
        let a = json!({ "keep": 1, "gone": 2 });
        let b = json!({ "keep": 1, "new": 3 });
        assert_eq!(patch(&a, &b), vec![json!([["new"], 3]), json!([["gone"]])]);
    }

    #[test]
    fn a_changing_nested_object_is_patched_inside() {
        let a = json!({ "timer": { "kind": "turn", "player": 1, "remaining_ms": 9000, "total_ms": 90000 } });
        let b = json!({ "timer": { "kind": "turn", "player": 1, "remaining_ms": 8000, "total_ms": 90000 } });
        assert_eq!(patch(&a, &b), vec![json!([["timer", "remaining_ms"], 8000])]);
    }

    #[test]
    fn random_edits_always_round_trip() {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        for _ in 0..300 {
            let n = rng.gen_range(0..60);
            let mut doc = json!({
                "buildings": (0..n).map(|_| Value::Null).collect::<Vec<_>>(),
                "players": (0..rng.gen_range(1..5)).map(|i| json!({ "id": i, "cards": 0, "vp": 0 })).collect::<Vec<_>>(),
                "hand": [0, 0, 0, 0, 0],
                "phase": "main",
            });
            for _ in 0..20 {
                let before = doc.clone();
                for _ in 0..rng.gen_range(0..6) {
                    match rng.gen_range(0..4) {
                        0 if n > 0 => doc["buildings"][rng.gen_range(0..n)] = json!({ "owner": rng.gen_range(0..4), "city": rng.gen_bool(0.5) }),
                        1 => {
                            let len = doc["players"].as_array().unwrap().len();
                            doc["players"][rng.gen_range(0..len)]["cards"] = json!(rng.gen_range(0..20));
                        }
                        2 => doc["hand"][rng.gen_range(0..5)] = json!(rng.gen_range(0..5)),
                        _ => {
                            let phase = ["roll", "main", "discard"][rng.gen_range(0..3)];
                            doc["phase"] = json!(phase);
                        }
                    }
                }
                patch(&before, &doc);
            }
        }
    }
}

use std::rc::Rc;

/// Determines the minimum leaf length when concatenating. I.e., any string with
/// fewer than MIN_LEAF_LENGTH graphemes is considered "short"
const MIN_LEAF_LENGTH: u32 = 4096;

enum Rope {
    Concat {
        left: Rc<Rope>,
        right: Rc<Rope>,
        weight: u32,
    },
    Leaf(Rc<str>),
}

use Rope::*;
impl Rope {
    fn concat(rope1: Self, rope2: Self) -> Self {
        use Rope::*;
        match rope2 {
            Leaf(ref s1) if s1.len() < MIN_LEAF_LENGTH as usize => match rope1 {
                Leaf(s2) if s2.len() < MIN_LEAF_LENGTH as usize => {
                    let mut new_leaf = String::with_capacity(s1.len() + s2.len());
                    new_leaf.push_str(&s1);
                    new_leaf.push_str(&s2);
                    return new_leaf.into();
                }
                _ => (),
            },
            _ => (),
        }
        return Concat {
            weight: rope1.weight(),
            left: Rc::new(rope1),
            right: Rc::new(rope2),
        };
    }

    fn weight(&self) -> u32 {
        match self {
            Concat { weight, .. } => *weight,
            Leaf(s) => s.len() as u32,
        }
    }
}

impl From<String> for Rope {
    fn from(other: String) -> Self {
        Self::Leaf(other.into())
    }
}

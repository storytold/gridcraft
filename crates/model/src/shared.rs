//! A part of a sheet or workbook shared between copies of it until one of them changes it.
//!
//! Every edit keeps the workbook as it was for Undo, so each undo step is a copy. The cells are
//! shared band by band ([`crate::CellStore`]); [`Shared`] does the same for the other large parts
//! (row heights, spilled values, notes, links, rules, names, styles), so typing in a cell no longer
//! copies all of them into every step. Reading goes straight through; the first change to a copy
//! that shares the part gives that copy a part of its own (`Arc::make_mut`).

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Default, PartialEq)]
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Shared(Arc::new(value))
    }

    /// Whether two copies still share one part (no change since one was copied from the other).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Shared::new(value)
    }
}

/// A shared list compares with a plain one, either way round, as the list it holds: code and
/// tests that compare a part with a `Vec` read as they did before the part was shared.
impl<T: PartialEq> PartialEq<Vec<T>> for Shared<Vec<T>> {
    fn eq(&self, other: &Vec<T>) -> bool {
        *self.0 == *other
    }
}

impl<T: PartialEq> PartialEq<Shared<Vec<T>>> for Vec<T> {
    fn eq(&self, other: &Shared<Vec<T>>) -> bool {
        *self == *other.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a, T> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&*self.0).into_iter()
    }
}

impl<T: Clone + IntoIterator> IntoIterator for Shared<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;
    /// Takes the part out: without a copy when nothing else shares it.
    fn into_iter(self) -> Self::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}

impl<A, T: FromIterator<A>> FromIterator<A> for Shared<T> {
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Shared::new(iter.into_iter().collect())
    }
}

impl<T: Serialize> Serialize for Shared<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Shared<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Shared::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_share_until_one_changes() {
        let a: Shared<Vec<u32>> = vec![1, 2, 3].into();
        let mut b = a.clone();
        assert!(Shared::ptr_eq(&a, &b));
        assert_eq!(b.len(), 3, "reading doesn't separate them");
        assert!(Shared::ptr_eq(&a, &b));
        b.push(4);
        assert!(!Shared::ptr_eq(&a, &b));
        assert_eq!(*a, vec![1, 2, 3]);
        assert_eq!(*b, vec![1, 2, 3, 4]);
        assert_eq!((&b).into_iter().sum::<u32>(), 10);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "[1,2,3,4]");
        assert_eq!(serde_json::from_str::<Shared<Vec<u32>>>(&json).unwrap(), b);
        // Compared with a plain list either way round, as the list it holds.
        assert_eq!(b, vec![1, 2, 3, 4]);
        assert_eq!(vec![1, 2, 3], a);
        assert_ne!(a, vec![1, 2]);
    }
}

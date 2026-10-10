use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A string-keyed map that keeps insertion order, serialized as a JSON object.
///
/// Question names and choice labels are sent in the order they were added.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedMap<V> {
    entries: Vec<(String, V)>,
}

impl<V> OrderedMap<V> {
    /// Create an empty map.
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Insert a value, replacing the value of an existing key in place.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// The value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Keys in insertion order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Into<String>, V> FromIterator<(K, V)> for OrderedMap<V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl<V> IntoIterator for OrderedMap<V> {
    type Item = (String, V);
    type IntoIter = std::vec::IntoIter<(String, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<V: Serialize> Serialize for OrderedMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for MapVisitor<V> {
            type Value = OrderedMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut map = OrderedMap::new();
                while let Some((k, v)) = access.next_entry::<String, V>()? {
                    map.insert(k, v);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(MapVisitor(PhantomData))
    }
}

/// Deserialize an object keyed `"0"`..`"n-1"` into a vector ordered by key.
pub(crate) fn indexed<'de, D, V>(deserializer: D) -> Result<Vec<V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    use serde::de::Error;

    let map = OrderedMap::<V>::deserialize(deserializer)?;
    let mut slots: Vec<Option<V>> = (0..map.len()).map(|_| None).collect();
    for (key, value) in map {
        let index = key
            .parse::<usize>()
            .ok()
            .filter(|i| *i < slots.len())
            .ok_or_else(|| D::Error::custom(format!("unexpected score key {key:?}")))?;
        slots[index] = Some(value);
    }
    // Keys are unique and in range, so every slot is filled.
    Ok(slots.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_insertion_order_and_replaces_in_place() {
        let mut map: OrderedMap<i32> = [("b", 1), ("a", 2)].into_iter().collect();
        map.insert("b", 3);
        assert_eq!(serde_json::to_string(&map).unwrap(), r#"{"b":3,"a":2}"#);
    }

    #[test]
    fn indexed_orders_by_numeric_key() {
        #[derive(Deserialize)]
        struct W {
            #[serde(deserialize_with = "indexed")]
            v: Vec<f64>,
        }
        let w: W = serde_json::from_str(r#"{"v":{"1":0.75,"0":0.25}}"#).unwrap();
        assert_eq!(w.v, vec![0.25, 0.75]);

        assert!(serde_json::from_str::<W>(r#"{"v":{"0":1,"2":0}}"#).is_err());
        assert!(serde_json::from_str::<W>(r#"{"v":{"x":1}}"#).is_err());
    }
}

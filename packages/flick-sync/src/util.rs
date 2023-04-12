use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::HashMap, hash::Hash, result};

pub(crate) trait ListItem<T> {
    fn id(&self) -> T;
}

macro_rules! derive_list_item {
    ($typ:ident) => {
        impl ListItem<u32> for $typ {
            fn id(&self) -> u32 {
                self.id
            }
        }
    };
}

pub(crate) use derive_list_item;

pub(crate) fn from_list<'de, D, K, V>(deserializer: D) -> result::Result<HashMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Hash + Eq,
    V: ListItem<K> + Deserialize<'de>,
{
    Ok(Vec::<V>::deserialize(deserializer)?
        .into_iter()
        .map(|v| (v.id(), v))
        .collect())
}

pub(crate) fn into_list<S, K, V>(
    map: &HashMap<K, V>,
    serializer: S,
) -> result::Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize,
{
    let list: Vec<&V> = map.values().collect();
    list.serialize(serializer)
}

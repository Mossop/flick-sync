use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::HashMap, hash::Hash, result};

pub(crate) trait ListItem<T> {
    fn id(&self) -> T;
}

macro_rules! derive_list_item {
    ($typ:ident) => {
        impl ListItem<String> for $typ {
            fn id(&self) -> String {
                self.id.clone()
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

pub(crate) fn safe<S: AsRef<str>>(str: S) -> String {
    str.as_ref()
        .chars()
        .map(|x| match x {
            '#' | '%' | '{' | '}' | '\\' | '/' | '<' | '>' | '*' | '?' | '$' | '!' | '"' | '\''
            | ':' | '@' | '+' | '`' | '|' | '=' => '_',
            _ => x,
        })
        .collect()
}

use std::{
    io::ErrorKind,
    iter::{empty, once},
    path::Path,
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeOwned, Error as _, Unexpected},
};
use serde_json::{Map, Value, from_str, from_value, to_string_pretty};
use tokio::fs::{read_to_string, write};
use tracing::error;

use crate::Error;

pub(crate) type JsonObject = Map<String, Value>;

#[derive(Default, Clone, Debug)]
pub(crate) struct SchemaVersion<const V: u64>;

impl<'de, const V: u64> Deserialize<'de> for SchemaVersion<V> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;

        if value == V {
            Ok(Self)
        } else {
            Err(D::Error::invalid_value(
                Unexpected::Unsigned(value),
                &format!("schema version {V}").as_str(),
            ))
        }
    }
}

impl<const V: u64> Serialize for SchemaVersion<V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(V)
    }
}

pub(crate) trait MigratableStore: Serialize + DeserializeOwned + Default {
    fn migrate(data: &mut JsonObject) -> Result<bool, Error>;

    async fn read_or_default(path: &Path) -> Result<Self, Error> {
        let str = match read_to_string(path).await {
            Ok(s) => s,
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    let val = Self::default();
                    let str = to_string_pretty(&val)?;
                    write(path, str).await?;
                    return Ok(val);
                } else {
                    error!(error = ?e);
                    return Ok(Default::default());
                }
            }
        };

        let (obj, migrated) = match from_str::<JsonObject>(&str) {
            Ok(mut obj) => {
                let migrated = match Self::migrate(&mut obj) {
                    Ok(m) => m,
                    Err(e) => {
                        error!(error = ?e);
                        return Ok(Default::default());
                    }
                };

                (obj, migrated)
            }
            Err(e) => {
                error!(error = ?e);
                return Ok(Default::default());
            }
        };

        let store = match from_value::<Self>(serde_json::Value::Object(obj)) {
            Ok(s) => s,
            Err(e) => {
                error!(error = ?e);
                return Ok(Default::default());
            }
        };

        if migrated {
            let str = to_string_pretty(&store)?;
            write(path, str).await?;
        }

        Ok(store)
    }
}

struct OptionIterator<T> {
    inner: Option<T>,
}

impl<T> Iterator for OptionIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.inner.take()
    }
}

pub(crate) trait JsonUtils {
    fn prop<'a>(&'a mut self, key: &'static str) -> Box<dyn Iterator<Item = &'a mut Value> + 'a>;
    fn values<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut Value> + 'a>;
    fn as_object<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut JsonObject> + 'a>;
}

impl JsonUtils for JsonObject {
    fn prop<'a>(&'a mut self, key: &'static str) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        Box::new(OptionIterator {
            inner: self.get_mut(key),
        })
    }

    fn values<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        Box::new(self.values_mut())
    }

    fn as_object<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut JsonObject> + 'a> {
        Box::new(once(self))
    }
}

impl JsonUtils for Value {
    fn prop<'a>(&'a mut self, key: &'static str) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        if let Value::Object(obj) = self {
            obj.prop(key)
        } else {
            Box::new(empty())
        }
    }

    fn values<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        match self {
            Value::Object(obj) => Box::new(obj.values_mut()),
            Value::Array(obj) => Box::new(obj.iter_mut()),
            _ => Box::new(empty()),
        }
    }

    fn as_object<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut JsonObject> + 'a> {
        match self {
            Value::Object(obj) => Box::new(once(obj)),
            _ => Box::new(empty()),
        }
    }
}

impl<'b> JsonUtils for Box<dyn Iterator<Item = &'b mut Value> + 'b> {
    fn prop<'a>(&'a mut self, key: &'static str) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        Box::new(self.flat_map(move |v| v.prop(key)))
    }

    fn values<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut Value> + 'a> {
        Box::new(self.flat_map(|v| v.values()))
    }

    fn as_object<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut JsonObject> + 'a> {
        Box::new(self.flat_map(|v| v.as_object()))
    }
}

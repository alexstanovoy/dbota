/// A decoded KV3 document: the format GUID its writer stamped (generic, modeldoc, ...) and the
/// root value, usually an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub format: [u8; 16],
    pub root: Value,
}

/// A KV3 value. The binary forms collapse into these: every integer width into `Int` or
/// `UInt`, `float` and `double` into `Float`, typed arrays into `Array`. Flags (resource,
/// sound event, ...) are read and dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Blob(Vec<u8>),
    Array(Vec<Value>),
    Object(Object),
}

/// A KV3 object's members in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    members: Vec<(String, Value)>,
}

impl Object {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            members: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn push(&mut self, key: String, value: Value) {
        self.members.push((key, value));
    }

    /// The member `key`; when a key repeats, the last one wins, as in Valve's reader.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.members
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.members.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl Value {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => Some(i),
            Value::UInt(u) => i64::try_from(u).ok(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::Int(i) => u64::try_from(i).ok(),
            Value::UInt(u) => Some(u),
            _ => None,
        }
    }

    /// Any number as `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Float(f) => Some(f),
            Value::Int(i) => Some(i as f64),
            Value::UInt(u) => Some(u as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
}

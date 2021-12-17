use intmap::IntMap;

fn serialize_int_map<S>(im: &IntMap<()>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(im.len()))?;
    for (k, v) in im.iter() {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

struct IntMapVisitor<V> {
    marker: PhantomData<fn() -> IntMap<V>>,
}

impl<V> IntMapVisitor<V> {
    fn new() -> Self {
        IntMapVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, V> Visitor<'de> for IntMapVisitor<V>
where
    V: Deserialize<'de>,
{
    type Value = IntMap<V>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a very special map")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = IntMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }

        Ok(map)
    }
}

fn deserialize_int_map<'de, D>(deserializer: D) -> Result<IntMap<()>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_map(IntMapVisitor::new())
}
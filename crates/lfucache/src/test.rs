use super::*;

#[derive(Debug)]
#[allow(dead_code)]
struct EntryData<'a, K, V> {
    freq: u16,
    last_tick: u32,
    key: &'a K,
    value: &'a V,
}

impl<'a, K, V> EntryData<'a, K, V> {
    fn new(item: &'a Entry<K, V>) -> Self {
        Self {
            freq: *item.freq.borrow(),
            last_tick: *item.last_tick.borrow(),
            key: &item.key,
            value: &item.value,
        }
    }
}

fn frequency_order<K, V, S>(cache: &LfuCache<K, V, S>) -> Vec<EntryData<'_, K, V>> {
    let mut entries = vec![];
    for item in cache.frequency_index.iter() {
        entries.push(EntryData::new(item));
    }
    entries
}

fn recency_order<K, V, S>(cache: &LfuCache<K, V, S>) -> Vec<EntryData<'_, K, V>> {
    let mut entries = vec![];
    for item in cache.recency_index.iter() {
        entries.push(EntryData::new(item));
    }
    entries
}

#[test]
fn decay() {
    let mut cache = LfuCacheU64::with_capacity(4);
    for i in 0..4 {
        cache.put(i, i);
        for _ in 0..i * 2 {
            cache.get(&i);
        }
    }
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 0,
        last_tick: 1,
        key: 0,
        value: 0,
    },
    EntryData {
        freq: 2,
        last_tick: 4,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 4,
        last_tick: 9,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
    );

    cache.get(&1);
    cache.get(&2);
    cache.put(10, 10);

    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 0,
        last_tick: 19,
        key: 10,
        value: 10,
    },
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
    );

    cache.get(&10);
    cache.put(11, 11);
    // bump up freq of 11 so that we can displace 1 on the next put
    cache.get(&11);
    cache.get(&11);
    cache.get(&11);
    cache.get(&11);
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 3,
        last_tick: 17,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 4,
        last_tick: 25,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
    );

    cache.put(12, 12);
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 0,
        last_tick: 26,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 4,
        last_tick: 25,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 5,
        last_tick: 18,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
]
"
    );

    // Ensure that we're all non-zero
    for _ in 0..5 {
        cache.get(&2);
        cache.get(&11);
        cache.get(&12);
    }

    // and bump up the ticks so that we trigger decay for 3
    for _ in 0..10 {
        cache.get(&11);
    }

    // Note that key: 3 has freq 6 in this snapshot
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 5,
        last_tick: 41,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 6,
        last_tick: 16,
        key: 3,
        value: 3,
    },
    EntryData {
        freq: 10,
        last_tick: 39,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 19,
        last_tick: 51,
        key: 11,
        value: 11,
    },
]
"
    );

    // trigger an eviction. This will decay key 3's freq
    // and it will be evicted, even though key 12 in
    // the snapshot above had freq 5 when key 3 had freq 6.
    cache.put(42, 42);
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 0,
        last_tick: 52,
        key: 42,
        value: 42,
    },
    EntryData {
        freq: 5,
        last_tick: 41,
        key: 12,
        value: 12,
    },
    EntryData {
        freq: 10,
        last_tick: 39,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 19,
        last_tick: 51,
        key: 11,
        value: 11,
    },
]
"
    );
}

#[test]
fn eviction() {
    let mut cache = LfuCacheU64::with_capacity(8);
    for i in 0..8 {
        cache.put(i, i);
        for _ in 0..i {
            cache.get(&i);
        }
    }

    k9::assert_equal!(cache.len(), 8);
    cache.put(8, 8);
    k9::assert_equal!(cache.len(), 8);

    let freq = frequency_order(&cache);
    k9::assert_equal!(*freq[0].key, 8, "0 got evicted, so 8 is first");
    k9::snapshot!(
        freq,
        "
[
    EntryData {
        freq: 0,
        last_tick: 37,
        key: 8,
        value: 8,
    },
    EntryData {
        freq: 1,
        last_tick: 3,
        key: 1,
        value: 1,
    },
    EntryData {
        freq: 2,
        last_tick: 6,
        key: 2,
        value: 2,
    },
    EntryData {
        freq: 3,
        last_tick: 10,
        key: 3,
        value: 3,
    },
    EntryData {
        freq: 4,
        last_tick: 15,
        key: 4,
        value: 4,
    },
    EntryData {
        freq: 5,
        last_tick: 21,
        key: 5,
        value: 5,
    },
    EntryData {
        freq: 6,
        last_tick: 28,
        key: 6,
        value: 6,
    },
    EntryData {
        freq: 7,
        last_tick: 36,
        key: 7,
        value: 7,
    },
]
"
    );

    for i in 9..12 {
        cache.put(i, i);
        cache.get(&i);
    }
    k9::snapshot!(
        frequency_order(&cache),
        "
[
    EntryData {
        freq: 1,
        last_tick: 39,
        key: 9,
        value: 9,
    },
    EntryData {
        freq: 1,
        last_tick: 41,
        key: 10,
        value: 10,
    },
    EntryData {
        freq: 1,
        last_tick: 10,
        key: 3,
        value: 3,
    },
    EntryData {
        freq: 1,
        last_tick: 43,
        key: 11,
        value: 11,
    },
    EntryData {
        freq: 4,
        last_tick: 15,
        key: 4,
        value: 4,
    },
    EntryData {
        freq: 5,
        last_tick: 21,
        key: 5,
        value: 5,
    },
    EntryData {
        freq: 6,
        last_tick: 28,
        key: 6,
        value: 6,
    },
    EntryData {
        freq: 7,
        last_tick: 36,
        key: 7,
        value: 7,
    },
]
"
    );
}

#[test]
fn basic() {
    let mut cache = LfuCacheU64::<&'static str>::with_capacity(8);
    cache.put(1, "hello");
    cache.put(2, "there");

    k9::snapshot!(
        frequency_order(&cache),
        r#"
[
    EntryData {
        freq: 0,
        last_tick: 1,
        key: 1,
        value: "hello",
    },
    EntryData {
        freq: 0,
        last_tick: 2,
        key: 2,
        value: "there",
    },
]
"#
    );

    cache.get(&1);
    cache.get(&1);
    cache.get(&1);
    cache.get(&2);

    k9::snapshot!(
        frequency_order(&cache),
        r#"
[
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
    EntryData {
        freq: 3,
        last_tick: 5,
        key: 1,
        value: "hello",
    },
]
"#
    );

    k9::snapshot!(
        recency_order(&cache),
        r#"
[
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
    EntryData {
        freq: 3,
        last_tick: 5,
        key: 1,
        value: "hello",
    },
]
"#
    );

    cache.get(&1);
    k9::snapshot!(
        recency_order(&cache),
        r#"
[
    EntryData {
        freq: 4,
        last_tick: 7,
        key: 1,
        value: "hello",
    },
    EntryData {
        freq: 1,
        last_tick: 6,
        key: 2,
        value: "there",
    },
]
"#
    );
}

#![warn(clippy::undocumented_unsafe_blocks)]
use ahash::AHasher;
use config::ConfigHandle;
use intrusive_collections::{
    intrusive_adapter, Bound, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::cmp::Eq;
use std::fmt::Debug;
use std::hash::{BuildHasher, BuildHasherDefault, Hash};
use std::rc::Rc;

struct Entry<K, V> {
    hash_link: LinkedListLink,
    recency_link: LinkedListLink,
    frequency_link: RBTreeLink,
    freq: RefCell<u16>,
    last_tick: RefCell<u32>,
    key: K,
    value: V,
}

intrusive_adapter!(RecencyAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { recency_link: LinkedListLink });
intrusive_adapter!(FrequenceAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { frequency_link: RBTreeLink });
intrusive_adapter!(HashAdapter<K,V> = Rc<Entry<K,V>>: Entry<K,V> { hash_link: LinkedListLink });

/// Key by Entry::freq
impl<'a, K, V> KeyAdapter<'a> for FrequenceAdapter<K, V> {
    type Key = u16;
    fn get_key(&self, entry: &'a Entry<K, V>) -> u16 {
        *entry.freq.borrow()
    }
}

pub type CapFunc = fn(&ConfigHandle) -> usize;

/// A cache using a Least-Frequently-Used eviction policy.
/// If K is u64 you should use LfuCacheU64 instead as it has
/// a more optimal hasher for integer keys.
pub struct LfuCache<K, V, S = BuildHasherDefault<AHasher>> {
    // Resolved once (in `new`/`with_capacity`) rather than on every
    // `get()`, since `metrics::histogram!(name)` re-resolves the handle
    // through the recorder's `register_histogram` -- a global mutex plus a
    // string-keyed hashmap lookup -- every time it's invoked. `get()` runs
    // once per cache lookup, i.e. once per rendered line per frame, so
    // paying that cost per call was measurable in profiling.
    hit: metrics::Histogram,
    miss: metrics::Histogram,
    cap: usize,
    cap_func: CapFunc,
    hasher: S,

    /// hash buckets for key-based lookup
    buckets: Vec<LinkedList<HashAdapter<K, V>>>,
    /// frequency-keyed rb-tree
    frequency_index: RBTree<FrequenceAdapter<K, V>>,
    /// the back is the least-recently-used whereas the front is the
    /// most-recently-used
    recency_index: LinkedList<RecencyAdapter<K, V>>,
    /// Number of items in the cache
    len: usize,
    /// tracks number of operations that affect the frequency/age of entries
    tick: u32,
}

impl<K: Hash + Eq + Clone + Debug, V, S: Default + BuildHasher> LfuCache<K, V, S> {
    #[cfg(test)]
    fn with_capacity(cap: usize) -> Self {
        let mut buckets = vec![];
        let num_buckets = (cap / 10).next_power_of_two();
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }

        let hasher = S::default();

        fn dummy_cap_func(_: &ConfigHandle) -> usize {
            8
        }

        Self {
            hit: metrics::histogram!("hit"),
            miss: metrics::histogram!("miss"),
            cap,
            cap_func: dummy_cap_func,
            buckets,
            frequency_index: RBTree::new(FrequenceAdapter::new()),
            recency_index: LinkedList::new(RecencyAdapter::new()),
            len: 0,
            tick: 0,
            hasher,
        }
    }

    pub fn new(
        hit: &'static str,
        miss: &'static str,
        cap_func: CapFunc,
        config: &ConfigHandle,
    ) -> Self {
        let cap = cap_func(config);
        let mut buckets = vec![];
        let num_buckets = (cap / 10).next_power_of_two();
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }

        let hasher = S::default();

        Self {
            hit: metrics::histogram!(hit),
            miss: metrics::histogram!(miss),
            cap,
            cap_func,
            buckets,
            frequency_index: RBTree::new(FrequenceAdapter::new()),
            recency_index: LinkedList::new(RecencyAdapter::new()),
            len: 0,
            tick: 0,
            hasher,
        }
    }

    fn bucket_for_key<Q: Hash>(&self, k: &Q) -> usize {
        (self.hasher.hash_one(k) as usize) % self.buckets.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Grow the hash buckets in the pursuit of reducing potential
    /// key collisions in any given bucket
    fn grow_hash(&mut self) {
        let num_buckets = self.buckets.len() * 2;
        let mut buckets = vec![];
        for _ in 0..num_buckets {
            buckets.push(LinkedList::new(HashAdapter::new()));
        }
        std::mem::swap(&mut buckets, &mut self.buckets);

        for mut old_bucket in buckets {
            while let Some(entry) = old_bucket.pop_front() {
                let bucket = self.bucket_for_key(&entry.key);
                self.buckets[bucket].push_front(entry);
            }
        }
    }

    pub fn update_config(&mut self, config: &ConfigHandle) {
        let new_cap = (self.cap_func)(config);
        if new_cap != self.cap {
            self.cap = new_cap;
            while self.len > self.cap {
                self.evict_one();
            }
        }
    }

    /// In order to mitigate previously-very-hot entries that are
    /// not currently being accessed from occupying the bulk of the
    /// table, this function finds the least-recently-accessed item
    /// and decreases its frequency value based on the number of ticks
    /// that have occurred since its last use.
    fn decay_least_recent(&mut self) {
        let mut cursor = self.recency_index.back_mut();
        if let Some(entry) = cursor.get() {
            if *entry.freq.borrow() == 0 {
                // No point removing/reinserting in the rbtree if the freq
                // is already 0
                return;
            }

            let delta = ((self.tick - *entry.last_tick.borrow()) / 10) as u16;
            if delta <= 1 {
                // No point removing/reinserting in the rbtree if there is no change
                return;
            }

            // Adjust lfu
            // SAFETY: `entry` is a live `Rc<Entry>` currently linked in
            // `frequency_index` (every entry is inserted into all three indices
            // by `put`). We hold it via the recency cursor and no other mutable
            // cursor into `frequency_index` is outstanding, so removing it by
            // pointer is sound; the returned `Rc` is re-inserted immediately.
            unsafe {
                let lfu_entry = self
                    .frequency_index
                    .cursor_mut_from_ptr(entry)
                    .remove()
                    .unwrap();
                {
                    let mut freq = entry.freq.borrow_mut();
                    *freq /= delta;
                }
                self.frequency_index.insert(lfu_entry);
            }

            // Adjust lru so that we don't immediately revisit this one
            // on the next decay_least_recent() call
            let lru_entry = cursor.remove().unwrap();
            self.recency_index.push_front(lru_entry);
        }
    }

    /// Remove the entry with the smallest frequency value
    fn evict_one(&mut self) {
        self.decay_least_recent();

        let mut cursor = self.frequency_index.lower_bound_mut(Bound::Included(&0));
        if let Some(entry) = cursor.remove() {
            let bucket = self.bucket_for_key(&entry.key);
            // SAFETY: `entry` was just removed from `frequency_index` and is
            // still linked in `buckets[bucket]` and `recency_index`. The
            // pointer is valid and belongs to each respective collection, and
            // no other mutable cursor into either is outstanding.
            unsafe {
                self.buckets
                    .get_mut(bucket)
                    .unwrap()
                    .cursor_mut_from_ptr(&*entry)
                    .remove();
                self.recency_index.cursor_mut_from_ptr(&*entry).remove();
            }
            self.len -= 1;
        }
    }

    pub fn clear(&mut self) {
        self.frequency_index.clear();
        self.recency_index.clear();
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.len = 0;
    }

    pub fn get<'a, Q: ?Sized + Debug + Hash + Eq>(&'a mut self, k: &Q) -> Option<&'a V>
    where
        K: Borrow<Q>,
    {
        let bucket = self.bucket_for_key(&k);
        let mut cursor = self.buckets.get_mut(bucket)?.front_mut();
        while let Some(entry) = cursor.get() {
            if entry.key.borrow() == k {
                // Adjust lru
                // SAFETY: `entry` is the matched entry, currently linked in
                // `recency_index` (inserted by `put`). No other mutable cursor
                // into `recency_index` is outstanding; the removed `Rc` is
                // immediately re-pushed to the front.
                unsafe {
                    let lru_entry = self
                        .recency_index
                        .cursor_mut_from_ptr(entry)
                        .remove()
                        .unwrap();
                    self.recency_index.push_front(lru_entry);
                }

                let entry = cursor.into_ref()?;
                self.hit.record(1.);

                self.tick += 1;

                *entry.last_tick.borrow_mut() = self.tick;

                // Adjust lfu
                // SAFETY: `entry` is the matched entry, currently linked in
                // `frequency_index`. No other mutable cursor into it is
                // outstanding; the removed `Rc` is mutated then re-inserted.
                unsafe {
                    let lfu_entry = self
                        .frequency_index
                        .cursor_mut_from_ptr(entry)
                        .remove()
                        .unwrap();
                    {
                        let mut freq = lfu_entry.freq.borrow_mut();
                        *freq = freq.saturating_add(1);
                    }
                    self.frequency_index.insert(lfu_entry);
                }

                return Some(&entry.value);
            }

            cursor.move_next();
        }
        self.miss.record(1.);
        None
    }

    pub fn put(&mut self, k: K, v: V) {
        let bucket = self.bucket_for_key(&k);

        self.tick += 1;

        // Remove any prior value
        {
            let mut cursor = self
                .buckets
                .get_mut(bucket)
                .expect("valid bucket index")
                .front_mut();
            while let Some(entry) = cursor.get() {
                if entry.key == k {
                    // SAFETY: `entry` is a live member of both `frequency_index`
                    // and `recency_index` (inserted by the original `put`). The
                    // pointer belongs to each collection and no other mutable
                    // cursor into either is outstanding. After removal the entry
                    // is dropped below via `cursor.remove()`.
                    unsafe {
                        self.frequency_index.cursor_mut_from_ptr(entry).remove();
                        self.recency_index.cursor_mut_from_ptr(entry).remove();
                    }
                    cursor.remove();
                    self.len -= 1;
                    break;
                }
                cursor.move_next();
            }
        }

        while self.len >= self.cap {
            self.evict_one();
        }

        let entry = Rc::new(Entry {
            key: k,
            value: v,
            freq: RefCell::new(0),
            recency_link: LinkedListLink::new(),
            frequency_link: RBTreeLink::new(),
            hash_link: LinkedListLink::new(),
            last_tick: RefCell::new(self.tick),
        });
        self.buckets[bucket].push_front(Rc::clone(&entry));
        self.frequency_index.insert(Rc::clone(&entry));
        self.recency_index.push_front(entry);
        self.len += 1;
        if self.buckets.len() < self.cap && self.len > self.buckets.len() / 2 {
            self.grow_hash();
        }
    }
}

/// A cache using a Least-Frequently-Used eviction policy,
/// where the cache keys are u64
pub type LfuCacheU64<V> = LfuCache<u64, V, fnv::FnvBuildHasher>;

#[cfg(test)]
mod test;

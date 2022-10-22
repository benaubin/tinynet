use std::{sync::atomic::{AtomicUsize, Ordering}, ops::{Deref, DerefMut}};

use parking_lot::{Mutex, MutexGuard};

enum Slot<T> {
    Occupied(T),
    Vacant { next: usize },
}

pub struct SharedSlots<T> {
    slots: Vec<Mutex<Slot<T>>>,
    next_free: AtomicUsize,
}

struct SlotRef<'a, T> {
    slots: &'a SharedSlots<T>,
    slot: MutexGuard<'a, Slot<T>>,
    key: usize,
}

impl<T> Drop for SlotRef<'_, T> {
    fn drop(&mut self) {
        match &mut *self.slot {
            Slot::Vacant { next } => {
                *next = self.slots.next_free.swap(self.key, Ordering::Relaxed);
            },
            _ => {}
        };
    }
}

pub struct Reserved<'a, T>(SlotRef<'a, T>);

impl<'a, T> Reserved<'a, T> {
    pub fn key(&self) -> usize {
        self.0.key
    }
    pub fn insert(mut self, item: T) -> Occupied<'a, T> {
        *self.0.slot = Slot::Occupied(item);
        Occupied(self.0)
    }
}

pub struct Occupied<'a, T>(SlotRef<'a, T>);

impl<'a, T> Occupied<'a, T> {
    pub fn key(&self) -> usize {
        self.0.key
    }
    pub fn take(self) -> (T, Reserved<'a, T>) {
        let mut inner = self.0;
        let item = match std::mem::replace(&mut *inner.slot, Slot::Vacant { next: usize::MAX }) {
            Slot::Occupied(item) => item,
            _ => unreachable!()
        };
        (item, Reserved(inner))
    }
}

impl<T> Deref for Occupied<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match &*self.0.slot {
            Slot::Occupied(item) => item,
            _ => unreachable!()
        }
    }
}

impl<T> DerefMut for Occupied<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut *self.0.slot {
            Slot::Occupied(item) => item,
            _ => unreachable!()
        }
    }
}

impl<T> SharedSlots<T> {
    pub fn new(capacity: usize) -> Self {
        let slots = std::iter::repeat(()).enumerate().map(|(i,_)| {
            Mutex::new(Slot::Vacant { next: i + 1 })
        }).take(capacity).collect();

        Self {
            slots,
            next_free: AtomicUsize::new(0)
        }
    }

    fn lock_slot(&self, key: usize) -> Option<SlotRef<'_, T>> {
        let slot = self.slots.get(key)?.lock();
        Some(SlotRef { slots: self, slot, key })
    }

    pub fn reserve(&self) -> Option<Reserved<'_, T>> {
        loop {
            let key = self.next_free.load(Ordering::Relaxed);
            let slot = self.lock_slot(key)?;
            let next_free = match &*slot.slot {
                Slot::Vacant { next } => *next,
                _ => continue,
            };
            self.next_free.store(next_free, Ordering::Relaxed);
            return Some(Reserved( slot ));
        }
    }

    pub fn get(&self, key: usize) -> Option<Occupied<'_, T>> {
        let slot = self.lock_slot(key)?;
        if let Slot::Vacant { .. } = &*slot.slot {
            return None;
        };
        Some(Occupied(slot))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use rand::Rng;

    use super::*;

    #[test]
    fn take() {
        let slots = SharedSlots::<i32>::new(2);
        let slot1 = slots.reserve().unwrap();
        let slot2 = slots.reserve().unwrap();
        assert!(slots.reserve().is_none());
        let key1 = slot1.insert(1).key();
        drop(slot2);
        let slot2 = slots.reserve().unwrap();
        assert!(slots.reserve().is_none());
        assert_eq!(*slots.get(key1).unwrap(), 1);
        let key2 = slot2.insert(2).key();
        assert!(slots.reserve().is_none());
        let slot2 = slots.get(key2).unwrap();
        let (val, vac) = slot2.take();
        assert_eq!(key2, vac.key());
        assert_eq!(val, 2);
        drop(vac);
        let slot2 = slots.reserve().unwrap();
        assert!(slots.reserve().is_none());
        assert_eq!(key2, slot2.key());
    }

    #[test]
    fn simple() {
        let slots = SharedSlots::<i32>::new(5);

        for i in 0..5 {
            slots.reserve().unwrap().insert(i);
        }
        assert!(slots.reserve().is_none());
        
        for i in 0..5 {
            assert_eq!(*slots.get(i as usize).unwrap(), i)
        }
    }
    #[test]
    fn threaded() {
        let slots = SharedSlots::<i32>::new (100);
        let mut values = vec![0i32; 100];
        rand::thread_rng().fill(&mut values[..]);
        let values = HashSet::from_iter(values.into_iter());

        std::thread::scope(|s| {
            for i in values.iter() {
                let slots = &slots;
                s.spawn(move || {
                    slots.reserve().unwrap().insert(*i);
                });
            }
        });
        
        let mut stored = HashSet::new();
        for i in 0..values.len() {
            stored.insert(*slots.get(i as usize).unwrap());
        }
        assert_eq!( values, stored );

    }
}

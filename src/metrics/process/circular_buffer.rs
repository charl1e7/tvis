use std::fmt;

#[derive(Clone)]
pub struct CircularBuffer<T> {
    buffer: Vec<T>,
    write_pos: usize,
    len: usize,
    capacity: usize,
}

impl<T> CircularBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            write_pos: 0,
            len: 0,
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.len < self.capacity {
            self.buffer.push(item);
            self.len += 1;
        } else {
            self.buffer[self.write_pos] = item;
        }
        self.write_pos = (self.write_pos + 1) % self.capacity;
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let (head, tail) = if self.len < self.capacity {
            (0, self.len)
        } else {
            (self.write_pos, self.capacity)
        };

        self.buffer[head..]
            .iter()
            .chain(&self.buffer[..head])
            .take(tail)
    }

    pub fn as_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.iter().cloned().collect()
    }
}

impl<T: fmt::Debug + Clone + Default> fmt::Debug for CircularBuffer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

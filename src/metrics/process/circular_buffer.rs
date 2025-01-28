use std::fmt;

#[derive(Clone)]
pub struct CircularBuffer<T: Clone> {
    buffer: Vec<T>,
    write_pos: usize,
    len: usize,
    capacity: usize,
}

impl<T: Clone> CircularBuffer<T> {
    // Остальные методы без изменений
    pub fn new(capacity: usize) -> Self {
        let buffer = Vec::with_capacity(capacity);

        Self {
            buffer,
            write_pos: 0,
            len: 0,
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.len < self.capacity {
            self.len += 1;
        }

        self.buffer[self.write_pos] = item;
        self.write_pos = (self.write_pos + 1) % self.capacity;
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let start = if self.len < self.capacity {
            0
        } else {
            self.write_pos
        };

        (0..self.len).map(move |i| {
            let pos = (start + i) % self.capacity;
            &self.buffer[pos]
        })
    }

    pub fn as_slice(&self) -> &Vec<T> {
        &self.buffer
    }
}

impl<T: fmt::Debug + Clone> fmt::Debug for CircularBuffer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

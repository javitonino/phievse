use std::{
    cmp::{max, min},
    sync::{Arc, Mutex},
};

use log::Log;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug)]
pub struct StringRingBuffer<const S: usize> {
    data: [u8; S],
    read_position: usize,
    write_position: usize,
    size: usize,
}

impl<const S: usize> Default for StringRingBuffer<S> {
    fn default() -> Self {
        Self {
            data: [0; S],
            read_position: Default::default(),
            write_position: Default::default(),
            size: Default::default(),
        }
    }
}

impl<const S: usize> StringRingBuffer<S> {
    pub fn append(&mut self, message: &str) {
        if message.is_empty() || message.len() >= S {
            return;
        }
        let data_len = message.len() + 1;

        // Find write position if near the end of buffer
        if self.write_position + data_len > S {
            self.size = self.write_position;
            self.write_position = 0;
            self.read_position = 0;
        }

        // Advance read position so we don't overwrite it
        if self.write_position <= self.read_position
            && self.read_position < self.write_position + data_len
        {
            self.advance_read_position(self.write_position + data_len);
        }

        //Write our data
        self.data[self.write_position..self.write_position + data_len - 1]
            .copy_from_slice(message.as_bytes());
        self.data[self.write_position + data_len - 1] = 0;
        self.write_position += data_len;
        self.size = max(self.size, self.write_position);
    }

    fn advance_read_position(&mut self, minimum_position: usize) {
        while self.read_position < minimum_position {
            if let Some(len) = self.str_len(self.read_position) {
                self.read_position += len + 1;
            } else {
                self.read_position = 0;
                self.size = minimum_position;
                return;
            }
        }
    }

    fn str_len(&self, position: usize) -> Option<usize> {
        if position == self.size {
            return None;
        }
        let mut len = 0;

        while (position + len) < self.size && self.data[position + len] != 0 {
            len += 1;
        }

        Some(len)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        StringRingBufferIterator {
            buffer: self,
            position: min(self.read_position, self.size),
            wrapped: self.read_position < self.write_position,
        }
    }
}

struct StringRingBufferIterator<'a, const S: usize> {
    buffer: &'a StringRingBuffer<S>,
    position: usize,
    wrapped: bool,
}

impl<'a, const S: usize> Iterator for StringRingBufferIterator<'a, S> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.buffer.write_position && self.wrapped {
            return None;
        }

        if let Some(len) = self.buffer.str_len(self.position) {
            self.position += len + 1;
            Some(unsafe {
                std::str::from_utf8_unchecked(
                    &self.buffer.data[self.position - len - 1..self.position - 1],
                )
            })
        } else if self.wrapped {
            None
        } else {
            self.position = 0;
            self.wrapped = true;
            self.next()
        }
    }
}

#[derive(Default)]
pub struct RingBufferLogger {
    pub buffer: Arc<Mutex<Box<StringRingBuffer<32768>>>>,
}

impl Log for RingBufferLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        println!("[{}] {}", record.level(), record.args());
        self.buffer.lock().unwrap().append(
            format!(
                "{} [{}:{}] {}",
                OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                record.level(),
                record.target(),
                record.args()
            )
            .as_str(),
        );
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        let mut buffer: StringRingBuffer<100> = Default::default();
        buffer.append("Hola!");

        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["Hola!"]);
    }

    #[test]
    fn returns_empty_buffer() {
        let buffer: StringRingBuffer<100> = Default::default();
        assert_eq!(buffer.iter().next(), None);
    }

    #[test]
    fn does_not_save_empty_strings() {
        let mut buffer: StringRingBuffer<10> = Default::default();
        buffer.append("");
        assert_eq!(buffer.iter().next(), None);
    }

    #[test]
    fn does_not_save_overflowing_strings() {
        let mut buffer: StringRingBuffer<10> = Default::default();
        buffer.append("1234567890");
        assert_eq!(buffer.iter().next(), None);
    }

    #[test]
    fn full_buffer() {
        let mut buffer: StringRingBuffer<10> = Default::default();
        buffer.append("1234");
        buffer.append("5678");

        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["1234", "5678"]);

        buffer.append("A");
        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["5678", "A"]);

        buffer.append("B");
        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["5678", "A", "B"]);
    }

    #[test]
    fn introduces_gaps() {
        let mut buffer: StringRingBuffer<10> = Default::default();
        buffer.append("123");
        buffer.append("456");
        buffer.append("789");

        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["456", "789"]);

        // 456, 789, A should fit but doesn't due to internal distribution of data
        // but it should still work, even if we lose a message
        buffer.append("A");
        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["789", "A"]);

        buffer.append("B");
        let data: Vec<&str> = buffer.iter().collect();
        assert_eq!(data, vec!["789", "A", "B"]);
    }

    #[test]
    fn sequence_until_it_crashes_or_empty() {
        let mut buffer: StringRingBuffer<30> = Default::default();
        for i in 0..10000 {
            buffer.append(format!("{}", i).as_str());
            let contents = buffer.iter().collect::<Vec<&str>>();
            assert!(contents.len() >= min(i, 5));
        }
    }
}

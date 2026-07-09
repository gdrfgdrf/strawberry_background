use parking_lot::{Condvar, Mutex};
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Deref;
use std::sync::Arc;

pub struct SharedBuffer {
    pub data: Mutex<Vec<u8>>,
    pub eof: Mutex<bool>,
    pub condvar: Condvar,
    pub length: Mutex<Option<u64>>
}

pub struct StreamingReader {
    shared: Arc<SharedBuffer>,
    pos: usize,
}

impl StreamingReader {
    pub fn new(shared: Arc<SharedBuffer>) -> Self {
        Self { shared, pos: 0 }
    }

    pub fn buffered_len(&self) -> usize {
        self.shared.data.lock().len()
    }

    pub fn is_eof(&self) -> bool {
        *self.shared.eof.lock()
    }
    
    pub fn length(&self) -> Option<u64> {
        let length = self.shared.length.lock();
        length.clone()
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn wait_for_data(&mut self, min_bytes: usize) -> io::Result<()> {
        let mut data = self.shared.data.lock();
        while data.len() < min_bytes && !*self.shared.eof.lock() {
            self.shared.condvar.wait(&mut data);
        }
        if data.len() >= min_bytes {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data",
            ))
        }
    }
}

impl Read for StreamingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut data = self.shared.data.lock();
        loop {
            if self.pos < data.len() {
                let available = &data[self.pos..];
                let len = available.len().min(buf.len());
                buf[..len].copy_from_slice(&available[..len]);
                self.pos += len;
                return Ok(len);
            }
            if !self.shared.eof.lock().deref() {
                self.shared.condvar.wait(&mut data);
            } else {
                return Ok(0);
            }
        }
    }
}

impl Seek for StreamingReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let data = self.shared.data.lock();
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as usize,
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot seek from end while streaming",
                ));
            }
            SeekFrom::Current(offset) => ((self.pos as i64) + offset) as usize,
        };
        if new_pos > data.len() && !self.shared.eof.lock().deref() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "seek beyond received data",
            ));
        }
        self.pos = new_pos;
        Ok(self.pos as u64)
    }
}

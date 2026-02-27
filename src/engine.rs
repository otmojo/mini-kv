use anyhow::Result;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::record::Record;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    Always,
    Batch(usize),
    Periodic(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IoMode {
    Buffered,
    Direct,
}

/// Log-structured KV store core engine
/// 
/// # Crash Consistency
/// - `logical_index`: number of put() calls made
/// - `durable_index`: number of entries fsync'd to disk
/// - Invariant: `durable_index ≤ logical_index`
pub struct Engine {
    file: File,
    /// In-memory index: key -> file offset
    index: HashMap<Vec<u8>, u64>,
    /// Current write position (end of file)
    pos: u64,
    pub sync_mode: SyncMode,
    pub io_mode: IoMode,
    /// Write counter for batch mode
    write_count: usize,
    /// Last sync time for periodic mode
    last_sync: Instant,
    /// Total put() calls made (logical writes)
    logical_index: usize,
    /// Total entries fsync'd to disk (durable writes)
    durable_index: usize,
    /// Progress file for crash test harness
    progress_file: Option<File>,
}

impl Engine {
    /// Open or create a database with default settings (Always sync, Buffered IO)
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(path, SyncMode::Always, IoMode::Buffered)
    }

    /// Open with specified sync mode (Buffered IO)
    pub fn with_sync(path: impl AsRef<Path>, mode: SyncMode) -> Result<Self> {
        Self::with_config(path, mode, IoMode::Buffered)
    }

    /// Open with full configuration
    pub fn with_config(
        path: impl AsRef<Path>, 
        sync_mode: SyncMode, 
        io_mode: IoMode
    ) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        
        #[cfg(unix)]
        options.mode(0o600);  // Owner read/write only
        
        let file = options.open(&path)?;
        
        let mut engine = Engine {
            file,
            index: HashMap::new(),
            pos: 0,
            sync_mode,
            io_mode,
            write_count: 0,
            last_sync: Instant::now(),
            logical_index: 0,
            durable_index: 0,
            progress_file: None,
        };

        engine.recover()?;

        // Crash test harness: enable progress reporting
        if std::env::var("CRASH_TEST").is_ok() {
            let p_file = File::create("durable_progress.txt")?;
            engine.progress_file = Some(p_file);
            engine.update_progress_file()?;
        }
        
        Ok(engine)
    }

    /// Recover from existing log file
    /// Scans all records, rebuilds index, truncates partial writes
    fn recover(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        self.file.read_to_end(&mut buf)?;
        
        let mut curr_pos = 0;
        let mut count = 0;

        while curr_pos < buf.len() {
            match Record::decode(&buf[curr_pos..]) {
                Ok((record, size)) => {
                    self.index.insert(record.key, curr_pos as u64);
                    curr_pos += size;
                    count += 1;
                }
                Err(_) => break,  // Partial/corrupted record, truncate later
            }
        }

        self.pos = curr_pos as u64;
        self.logical_index = count;
        self.durable_index = count;  // Recovered data is durable by definition

        // Truncate partial writes at end of file
        if self.pos < buf.len() as u64 {
            self.file.set_len(self.pos)?;
        }
        
        Ok(())
    }

    /// Write a key-value pair
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let record = Record::new(key.clone(), value);
        let encoded = record.encode();
        let current_record_pos = self.pos;

        // 1. Write to file (may be buffered)
        self.file.write_all(&encoded)?;
        self.logical_index += 1;

        // 2. Determine if we need to sync based on mode
        let should_sync = match self.sync_mode {
            SyncMode::Always => true,
            SyncMode::Batch(n) => {
                self.write_count += 1;
                self.write_count >= n
            }
            SyncMode::Periodic(d) => self.last_sync.elapsed() >= d,
        };

        if should_sync {
            self.sync()?;
        }

        // 3. Update in-memory index (even if not yet durable)
        self.index.insert(key, current_record_pos);
        self.pos += encoded.len() as u64;
        
        Ok(())
    }

    /// Force sync to disk, making all writes up to now durable
    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_data()?;
        self.durable_index = self.logical_index;
        self.write_count = 0;
        self.last_sync = Instant::now();
        self.update_progress_file()?;
        Ok(())
    }

    /// Update progress file with current durable index (for crash testing)
    fn update_progress_file(&mut self) -> Result<()> {
        if let Some(file) = &mut self.progress_file {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            write!(file, "{}", self.durable_index)?;
            file.sync_data()?;  // Ensure parent process sees it
        }
        Ok(())
    }

    /// Check if key exists in index
    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.index.contains_key(key)
    }
}
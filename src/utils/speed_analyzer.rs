use std::time::SystemTime;

pub struct SpeedAnalyzer {
    start_time: Option<u128>,
    pub total: u64,
}

impl SpeedAnalyzer {
    pub fn new() -> Self {
        Self {
            start_time: None,
            total: 0,
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_micros(),
        );
    }

    pub fn add(&mut self, delta: u64) {
        self.total += delta;
    }

    pub fn speed(&self) -> f32 {
        if let Some(start_time) = self.start_time {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_micros();
            let time_delta = (now - start_time) / 1000000;
            return (self.total as f32) / (time_delta as f32);
        }
        0.0
    }
}

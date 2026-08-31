use std::time::Instant;

pub struct SpeedAnalyzer {
    start_time: Option<Instant>,
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
        self.start_time = Some(Instant::now());
    }

    pub fn add(&mut self, delta: u64) {
        self.total = self.total.saturating_add(delta);
    }

    pub fn speed(&self) -> f32 {
        match self.start_time {
            Some(start) => {
                let elapsed_secs = start.elapsed().as_secs_f32();
                if elapsed_secs <= 0.0 {
                    0.0
                } else {
                    self.total as f32 / elapsed_secs
                }
            }
            None => 0.0,
        }
    }
}

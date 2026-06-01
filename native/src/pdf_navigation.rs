use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub enum NavigationTarget {
    Markdown {
        path: String,
        line: usize,
        column: usize,
    },
    Pdf {
        path: String,
        page: u16,
        scroll_offset: f32,
        zoom: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationPoint {
    pub target: NavigationTarget,
    pub timestamp: u64,
}

impl NavigationPoint {
    pub fn new(target: NavigationTarget) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self { target, timestamp }
    }
}

#[derive(Debug, Clone)]
pub struct NavigationHistory {
    pub entries: Vec<NavigationPoint>,
    pub current_index: usize,
    pub max_entries: usize,
}

impl Default for NavigationHistory {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            current_index: 0,
            max_entries: 50,
        }
    }
}

impl NavigationHistory {
    pub fn push(&mut self, target: NavigationTarget) {
        // Truncate forward history if we are in the middle of the stack
        if !self.entries.is_empty() && self.current_index < self.entries.len() - 1 {
            self.entries.truncate(self.current_index + 1);
        }

        // Avoid duplicate consecutive targets
        if let Some(last) = self.entries.last() {
            if last.target == target {
                return;
            }
        }

        self.entries.push(NavigationPoint::new(target));
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
        self.current_index = self.entries.len().saturating_sub(1);
    }

    pub fn go_back(&mut self) -> Option<NavigationTarget> {
        if self.current_index > 0 && !self.entries.is_empty() {
            self.current_index -= 1;
            Some(self.entries[self.current_index].target.clone())
        } else {
            None
        }
    }

    pub fn go_forward(&mut self) -> Option<NavigationTarget> {
        if self.current_index + 1 < self.entries.len() {
            self.current_index += 1;
            Some(self.entries[self.current_index].target.clone())
        } else {
            None
        }
    }
}

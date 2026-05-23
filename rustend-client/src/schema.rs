#[derive(Debug, Clone, Default)]
pub struct IndexEntry {
    pub name:        String,
    pub object_type: String,
    pub json_path:   String,
}

#[derive(Debug, Clone)]
pub struct IndexSchema {
    pub version: u32,
    pub entries: Vec<IndexEntry>,
}

impl Default for IndexSchema {
    fn default() -> Self {
        Self { version: 1, entries: vec![] }
    }
}

impl IndexSchema {
    pub fn new() -> Self { Self::default() }

    pub fn version(mut self, v: u32) -> Self {
        self.version = v;
        self
    }

    pub fn add(
        mut self,
        name: impl Into<String>,
        object_type: impl Into<String>,
        json_path: impl Into<String>,
    ) -> Self {
        self.entries.push(IndexEntry {
            name:        name.into(),
            object_type: object_type.into(),
            json_path:   json_path.into(),
        });
        self
    }
}
